// Publish one verified desktop release using the existing Git credential helper.
// Credentials are kept in memory and never written to disk or logs.
import { execFileSync } from 'node:child_process';
import { createReadStream, openAsBlob } from 'node:fs';
import { readFile, stat, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const version = process.argv[2];
if (!/^\d+\.\d+\.\d+$/.test(version ?? '')) throw new Error('Expected version x.y.z');
const publish = process.argv.includes('--publish');
const windowsOnly = process.argv.includes('--windows-only');
const repository = 'Norwi4/reqon';
function git(args, input) {
  return execFileSync('git', args, { cwd: root, input, encoding: 'utf8',
    env: { ...process.env, GCM_INTERACTIVE: 'never', GIT_TERMINAL_PROMPT: '0' },
    stdio: ['pipe', 'pipe', 'pipe'] }).trim();
}
if (git(['status', '--porcelain'])) throw new Error('Commit the source changes before publishing');
const target = git(['rev-parse', 'HEAD']);
const body = await readFile(path.join(root, `docs/release-${version}.md`), 'utf8');
const filenames = [
  `Reqon-Setup-${version}-win64.exe`,
  `Reqon-${version}-windows-x86_64.zip`,
  ...(!windowsOnly ? [`Reqon-${version}-linux-x86_64.tar.gz`, `reqon_${version}_astra1.8_amd64.deb`] : []),
];
const assets = [];
for (const name of filenames) {
  const file = path.join(root, 'dist', name);
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  assets.push({ name, file, size: (await stat(file)).size, digest: hash.digest('hex') });
}
const sums = assets.map(a => `${a.digest}  ${a.name}`).join('\n') + '\n';
const checksumPath = path.join(root, 'dist', `SHA256SUMS-${version}.txt`);
await writeFile(checksumPath, sums);
assets.push({ name: 'SHA256SUMS.txt', file: checksumPath, size: Buffer.byteLength(sums),
  digest: createHash('sha256').update(sums).digest('hex') });
console.log(JSON.stringify({ version, target, publish, assets: assets.map(({ file, ...a }) => a) }, null, 2));
if (!publish) process.exit(0);

const credentials = Object.fromEntries(git(['credential', 'fill'], 'protocol=https\nhost=github.com\n\n')
  .split(/\r?\n/).map(line => [line.slice(0, line.indexOf('=')), line.slice(line.indexOf('=') + 1)]));
if (!credentials.password) throw new Error('GitHub credential is unavailable');
const headers = { Authorization: `Bearer ${credentials.password}`, Accept: 'application/vnd.github+json',
  'X-GitHub-Api-Version': '2026-03-10', 'User-Agent': 'Reqon-release-publisher' };
async function api(route, method = 'GET', data) {
  const response = await fetch(`https://api.github.com/repos/${repository}${route}`, {
    method, headers: { ...headers, ...(data ? { 'Content-Type': 'application/json' } : {}) },
    body: data ? JSON.stringify(data) : undefined, signal: AbortSignal.timeout(60000),
  });
  if (!response.ok) throw new Error(`${method} ${route}: HTTP ${response.status}`);
  return response.json();
}
if ((await api('/branches/main')).commit.sha !== target) throw new Error('Push this commit to main first');
const tag = `v${version}`;
let release = (await api('/releases?per_page=100')).find(r => r.tag_name === tag);
if (release && !release.draft) throw new Error('This version is already published');
if (release && release.target_commitish !== target) throw new Error('Draft targets a different commit');
if (!release) release = await api('/releases', 'POST', {
  tag_name: tag, target_commitish: target, name: `Reqon ${version}`, body, draft: true, prerelease: false,
});
else release = await api(`/releases/${release.id}`, 'PATCH', { body });
for (const asset of assets) {
  let result = release.assets.find(a => a.name === asset.name);
  if (!result) {
    console.log(`Uploading ${asset.name} (${asset.size} bytes)`);
    const url = new URL(release.upload_url.split('{')[0]);
    if (url.hostname !== 'uploads.github.com' || url.protocol !== 'https:') throw new Error('Unexpected upload destination');
    url.searchParams.set('name', asset.name);
    const response = await fetch(url, { method: 'POST',
      headers: { ...headers, 'Content-Type': 'application/octet-stream', 'Content-Length': String(asset.size) },
      body: await openAsBlob(asset.file), signal: AbortSignal.timeout(900000) });
    if (!response.ok) throw new Error(`Upload ${asset.name}: HTTP ${response.status}`);
    result = await response.json();
  }
  if (result.size !== asset.size || result.state !== 'uploaded' || result.digest !== `sha256:${asset.digest}`)
    throw new Error(`Uploaded asset verification failed: ${asset.name}`);
  console.log(`Verified ${asset.name}`);
}
release = await api(`/releases/${release.id}`, 'PATCH', { draft: false, make_latest: 'true' });
console.log(`Published ${release.html_url}`);
