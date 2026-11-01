import { createReadStream } from 'node:fs';
import { readdir, lstat, readFile, mkdir, writeFile, rename } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import path from 'node:path';

const VERSION = '(0|[1-9]\\d*)\\.(0|[1-9]\\d*)\\.(0|[1-9]\\d*)';
const patterns = [
  [new RegExp(`^(?:Reqon|Getman)-Setup-(${VERSION})-win64\\.exe$`, 'i'), 'windows', 'Windows x64', 'EXE'],
  [new RegExp(`^Reqon-(${VERSION})-linux-x86_64\\.tar\\.gz$`, 'i'), 'linux', 'Linux x86-64', 'TAR.GZ'],
  [new RegExp(`^(?:reqon|getman)_(${VERSION})_astra1\\.8_amd64\\.deb$`, 'i'), 'astra', 'Astra Linux 1.8', 'DEB'],
];

export function parseAsset(name) {
  for (const [pattern, platform, platformLabel, format] of patterns) {
    const match = pattern.exec(name);
    if (match) return { version: match[1], platform, platformLabel, format, architecture: 'x64' };
  }
  return null;
}

export function compareVersions(a, b) {
  const left = a.split('.').map(BigInt);
  const right = b.split('.').map(BigInt);
  for (let i = 0; i < 3; i++) {
    if (left[i] !== right[i]) return left[i] > right[i] ? 1 : -1;
  }
  return 0;
}

export function sortReleases(releases) {
  return [...releases].sort((a, b) => compareVersions(b.version, a.version)
    || Date.parse(b.uploadedAt) - Date.parse(a.uploadedAt) || a.id.localeCompare(b.id));
}

export function mergeReleases(local, github) {
  // A published GitHub asset supersedes the local copy of the same platform/version.
  const remoteKeys = new Set(github.map(r => `${r.platform}/${r.version}`));
  return sortReleases([...github, ...local.filter(r => !remoteKeys.has(`${r.platform}/${r.version}`))]);
}

export function latestByPlatform(releases) {
  return Object.fromEntries(['windows', 'linux', 'astra'].map(platform =>
    [platform, sortReleases(releases.filter(r => r.platform === platform))[0] ?? null]));
}

export class LocalReleases {
  constructor(directory, notesDirectory) {
    this.directory = directory;
    this.notesDirectory = notesDirectory;
    this.cache = new Map();
  }
  async scan() {
    let entries;
    try { entries = await readdir(this.directory, { withFileTypes: true }); }
    catch (error) { if (error.code === 'ENOENT') return []; throw error; }
    const found = [];
    const live = new Set();
    for (const entry of entries) {
      const parsed = parseAsset(entry.name);
      if (!parsed || !entry.isFile()) continue;
      const file = path.join(this.directory, entry.name);
      const stat = await lstat(file);
      if (!stat.isFile() || stat.size === 0) continue;
      live.add(entry.name);
      const signature = `${stat.size}:${stat.mtimeMs}:${stat.ctimeMs}`;
      let cached = this.cache.get(entry.name);
      if (!cached || cached.signature !== signature) {
        const hash = createHash('sha256');
        for await (const chunk of createReadStream(file)) hash.update(chunk);
        const after = await lstat(file);
        if (`${after.size}:${after.mtimeMs}:${after.ctimeMs}` !== signature) continue;
        let notes = '';
        try {
          notes = await readFile(path.join(this.notesDirectory, `${parsed.platform === 'windows' ? 'windows' : 'linux'}-${parsed.version}.md`), 'utf8');
        } catch (error) { if (error.code !== 'ENOENT') throw error; }
        cached = { signature, release: {
          ...parsed, id: `local:${entry.name}`, filename: entry.name,
          size: stat.size, uploadedAt: stat.mtime.toISOString(),
          sha256: hash.digest('hex'), source: 'local', notes,
          url: `/files/${encodeURIComponent(entry.name)}`,
        } };
        this.cache.set(entry.name, cached);
      }
      found.push(cached.release);
    }
    for (const name of this.cache.keys()) if (!live.has(name)) this.cache.delete(name);
    return sortReleases(found);
  }
}

export function normalizeGitHubReleases(releases, repository) {
  const result = [];
  for (const release of releases) {
    if (release.draft || release.prerelease || !release.published_at) continue;
    // Only explicit stable tags. Beta/RC/nightly tags can never become stable.
    const tagVersion = /^v?(\d+\.\d+\.\d+)$/.exec(release.tag_name)?.[1];
    if (!tagVersion) continue;
    for (const asset of release.assets ?? []) {
      const parsed = parseAsset(asset.name);
      if (!parsed || parsed.version !== tagVersion || asset.state !== 'uploaded' || !(asset.size > 0)) continue;
      let url;
      try { url = new URL(asset.browser_download_url); } catch { continue; }
      if (url.protocol !== 'https:' || url.host !== 'github.com' || url.username || url.password
        || !url.pathname.startsWith(`/${repository}/releases/download/`)) continue;
      const uploadedAt = asset.updated_at || asset.created_at || release.published_at;
      if (!Number.isFinite(Date.parse(uploadedAt))) continue;
      result.push({
        ...parsed, id: `github:${asset.id}`, filename: asset.name,
        size: asset.size, uploadedAt, source: 'github', url: url.href,
        sha256: /^sha256:[a-f0-9]{64}$/i.test(asset.digest ?? '') ? asset.digest.slice(7).toLowerCase() : null,
        notes: release.body || '',
      });
    }
  }
  return sortReleases(result);
}

export class GitHubReleases {
  constructor(repository, { fetchImpl = fetch, token = '', cacheFile } = {}) {
    if (!/^[\w.-]+\/[\w.-]+$/.test(repository)) throw new Error('Invalid GitHub repository');
    this.repository = repository;
    this.fetch = fetchImpl;
    this.token = token;
    this.cacheFile = cacheFile;
    this.checkedAt = 0;
    this.snapshot = null;
    this.inflight = null;
  }
  async pages(route) {
    const result = [];
    for (let page = 1; page <= 100; page++) {
      const response = await this.fetch(`https://api.github.com/repos/${this.repository}/${route}?per_page=100&page=${page}`, {
        headers: {
          Accept: 'application/vnd.github+json', 'User-Agent': 'Reqon-Website',
          'X-GitHub-Api-Version': '2026-03-10',
          ...(this.token ? { Authorization: `Bearer ${this.token}` } : {}),
        }, signal: AbortSignal.timeout(10000),
      });
      if (!response.ok) throw new Error(`GitHub HTTP ${response.status}`);
      const batch = await response.json();
      if (!Array.isArray(batch)) throw new Error('Invalid GitHub response');
      result.push(...batch);
      if (batch.length < 100) return result;
    }
    throw new Error('GitHub pagination limit reached');
  }
  async load() {
    if (this.inflight) return this.inflight;
    if (Date.now() - this.checkedAt < 300000 && this.snapshot) return this.snapshot;
    this.inflight = this.refresh();
    try { return await this.inflight; } finally { this.inflight = null; }
  }
  async refresh() {
    if (!this.snapshot && this.cacheFile) {
      try {
        const saved = JSON.parse(await readFile(this.cacheFile, 'utf8'));
        if (saved.repository === this.repository && Array.isArray(saved.raw)) {
          this.snapshot = { releases: normalizeGitHubReleases(saved.raw, this.repository), syncedAt: saved.syncedAt, stale: true };
        }
      } catch { /* A missing or invalid cache is replaced after the next successful sync. */ }
    }
    this.checkedAt = Date.now();
    try {
      const raw = await this.pages('releases');
      for (const release of raw) {
        if (!release.draft && !release.prerelease && release.assets?.length >= 30) {
          release.assets = await this.pages(`releases/${release.id}/assets`);
        }
      }
      const syncedAt = new Date().toISOString();
      this.snapshot = { releases: normalizeGitHubReleases(raw, this.repository), syncedAt, stale: false };
      if (this.cacheFile) {
        try {
          await mkdir(path.dirname(this.cacheFile), { recursive: true });
          await writeFile(`${this.cacheFile}.tmp`, JSON.stringify({ repository: this.repository, raw, syncedAt }));
          await rename(`${this.cacheFile}.tmp`, this.cacheFile);
        } catch { console.warn('Release cache could not be saved; keeping the in-memory snapshot.'); }
      }
    } catch {
      this.snapshot = { releases: [], syncedAt: null, ...this.snapshot, stale: true };
    }
    return this.snapshot;
  }
}

export class ReleaseCatalog {
  constructor(local, github = null) { this.local = local; this.github = github; this.inflight = null; }
  async load() {
    if (this.inflight) return this.inflight;
    this.inflight = (async () => {
      const [local, remote] = await Promise.all([this.local.scan(), this.github?.load()]);
      const releases = mergeReleases(local, remote?.releases ?? []);
      return {
        releases, latest: latestByPlatform(releases),
        githubStale: remote?.stale ?? false, syncedAt: remote?.syncedAt ?? null,
        repository: this.github?.repository ?? null,
      };
    })();
    try { return await this.inflight; } finally { this.inflight = null; }
  }
}
