import { execFileSync } from 'node:child_process';
import { openAsBlob } from 'node:fs';
import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { LocalReleases, compareVersions } from '../src/releases.mjs';

// Uses the existing Git credential helper. Credentials never go to disk or logs.
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const repository = 'Norwi4/reqon';
const apply = process.argv.includes('--publish');
const credential = Object.fromEntries(execFileSync('git', ['credential', 'fill'], {
  cwd: root, input: 'protocol=https\nhost=github.com\n\n', encoding: 'utf8',
  env: { ...process.env, GCM_INTERACTIVE: 'never', GIT_TERMINAL_PROMPT: '0' },
  stdio: ['pipe', 'pipe', 'pipe'],
}).trim().split(/\r?\n/).map(line => [line.slice(0, line.indexOf('=')), line.slice(line.indexOf('=') + 1)]));
if (!credential.password) throw new Error('GitHub credential is unavailable');
const headers = {
  Authorization: `Bearer ${credential.password}`, Accept: 'application/vnd.github+json',
  'X-GitHub-Api-Version': '2026-03-10', 'User-Agent': 'Reqon-release-publisher',
};
async function api(route, method = 'GET', data) {
  const response = await fetch(`https://api.github.com/repos/${repository}${route}`, {
    method, headers: { ...headers, ...(data ? { 'Content-Type': 'application/json' } : {}) },
    body: data ? JSON.stringify(data) : undefined, signal: AbortSignal.timeout(60000),
  });
  if (!response.ok) throw new Error(`${method} ${route}: HTTP ${response.status} ${(await response.text()).slice(0, 500)}`);
  return response.json();
}
const assets = await new LocalReleases(path.join(root, 'dist'), path.join(root, 'website/public/notes')).scan();
const versions = [...new Set(assets.map(a => a.version))].sort(compareVersions);
const existing = await api('/releases?per_page=100');
const target = (await api('/branches/main')).commit.sha;
console.log(JSON.stringify({ action: apply ? 'publish' : 'preview', repository, versions: versions.length, assets: assets.length, target }));
const report = [];
for (const version of versions) {
  const group = assets.filter(a => a.version === version).sort((a, b) => a.filename.localeCompare(b.filename));
  const tag = `v${version}`;
  let release = existing.find(r => r.tag_name === tag);
  const checksums = group.map(a => `${a.sha256}  ${a.filename}`).join('\n') + '\n';
  const notes = [...new Set(group.map(a => a.notes).filter(Boolean))].join('\n\n---\n\n');
  const body = `Готовые сборки Reqon ${version}.\n\n${group.map(a => `- ${a.platformLabel}: \`${a.filename}\``).join('\n')}\n\n${notes || 'Архивный выпуск из локального каталога сборок.'}\n\n## SHA-256\n\n\`\`\`text\n${checksums}\`\`\`\n\nУстановщики опубликованы из сохранённого архива. Автоматические архивы GitHub «Source code» содержат снимок репозитория и не гарантируют соответствие исходникам этой сборки.\n`;
  if (!apply) { console.log(JSON.stringify({ tag, files: group.map(a => a.filename), existing: !!release })); continue; }
  if (!release) release = await api('/releases', 'POST', { tag_name: tag, target_commitish: target, name: `Reqon ${version}`, body, draft: true, prerelease: false });
  async function upload(name, data, size, digest) {
    const found = release.assets.find(a => a.name === name);
    if (found) {
      if (found.size !== size || found.state !== 'uploaded' || (digest && found.digest !== `sha256:${digest}`)) throw new Error(`Existing asset differs: ${name}`);
      console.log(`Verified existing ${name}`); return found;
    }
    console.log(`Uploading ${name} (${size} bytes)`);
    const url = new URL(release.upload_url.split('{')[0]);
    if (url.hostname !== 'uploads.github.com' || url.protocol !== 'https:') throw new Error('Unexpected upload destination');
    url.searchParams.set('name', name);
    const response = await fetch(url, { method: 'POST', headers: { ...headers, 'Content-Type': 'application/octet-stream', 'Content-Length': String(size) }, body: data, signal: AbortSignal.timeout(900000) });
    if (!response.ok) throw new Error(`Upload ${name}: HTTP ${response.status} ${(await response.text()).slice(0, 300)}`);
    const result = await response.json();
    if (result.size !== size || result.state !== 'uploaded' || (digest && result.digest !== `sha256:${digest}`)) throw new Error(`Uploaded asset verification failed: ${name}`);
    release.assets.push(result);
    console.log(`Verified ${name}`);
    return result;
  }
  for (const asset of group) await upload(asset.filename, await openAsBlob(path.join(root, 'dist', asset.filename)), asset.size, asset.sha256);
  await upload('SHA256SUMS.txt', Buffer.from(checksums), Buffer.byteLength(checksums));
  if (release.draft) release = await api(`/releases/${release.id}`, 'PATCH', { draft: false, make_latest: version === versions.at(-1) ? 'true' : 'false' });
  report.push({ version, url: release.html_url, files: group.map(a => ({ name: a.filename, sha256: a.sha256, size: a.size })) });
  console.log(`Published ${tag}: ${release.html_url}`);
}
if (apply) {
  await mkdir(path.join(root, 'website/.cache'), { recursive: true });
  await writeFile(path.join(root, 'website/.cache/published-releases.json'), JSON.stringify(report, null, 2) + '\n');
  console.log(`DONE: ${versions.length} releases, ${assets.length} installers`);
}
