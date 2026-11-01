import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, rm, utimes } from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import { createHash } from 'node:crypto';
import { parseAsset, compareVersions, sortReleases, latestByPlatform, normalizeGitHubReleases, mergeReleases, LocalReleases, GitHubReleases } from '../src/releases.mjs';

test('semantic ordering, stable names, platform detection, and no prereleases', () => {
  assert.equal(compareVersions('0.13.10', '0.13.9'), 1);
  assert.equal(compareVersions('1.0.0', '0.99.99'), 1);
  assert.equal(parseAsset('Reqon-Setup-0.13.8-win64.exe').version, '0.13.8');
  assert.equal(parseAsset('Getman-Setup-0.8.0-win64.exe').platform, 'windows');
  assert.equal(parseAsset('Reqon-0.13.4-linux-x86_64.tar.gz').platform, 'linux');
  assert.equal(parseAsset('getman_0.13.2_astra1.8_amd64.deb').platform, 'astra');
  for (const name of ['Reqon-Setup-0.14.0-beta-win64.exe', 'Reqon-windows.zip', 'Reqon-Setup-0.13.8-win64.exe.part', '../Reqon-Setup-0.13.8-win64.exe', 'Reqon-Setup-00.13.8-win64.exe']) assert.equal(parseAsset(name), null);
});

test('highest version wins, upload time breaks ties, platforms are independent', () => {
  const r = (id, version, uploadedAt, platform = 'windows') => ({ id, version, uploadedAt, platform });
  const releases = [r('a', '0.13.9', '2026-11-05'), r('b', '0.13.10', '2026-11-01'), r('c', '0.13.10', '2026-11-02'), r('d', '0.13.4', '2026-11-01', 'linux')];
  assert.equal(sortReleases(releases)[0].id, 'c');
  assert.equal(latestByPlatform(releases).windows.id, 'c');
  assert.equal(latestByPlatform(releases).linux.id, 'd');
  assert.equal(latestByPlatform(releases).astra, null);
});

const asset = (overrides = {}) => ({ id: 1, name: 'Reqon-Setup-0.13.8-win64.exe', state: 'uploaded', size: 100, updated_at: '2026-11-01T12:00:00Z', browser_download_url: 'https://github.com/Norwi4/reqon/releases/download/v0.13.8/Reqon-Setup-0.13.8-win64.exe', digest: `sha256:${'a'.repeat(64)}`, ...overrides });
const release = overrides => ({ tag_name: 'v0.13.8', draft: false, prerelease: false, published_at: '2026-11-01T11:00:00Z', assets: [asset()], body: 'Release notes', ...overrides });

test('GitHub excludes drafts, prereleases, invalid tags, pending assets and external URLs', () => {
  const valid = normalizeGitHubReleases([release()], 'Norwi4/reqon');
  assert.equal(valid.length, 1); assert.equal(valid[0].sha256, 'a'.repeat(64));
  assert.equal(valid[0].url, asset().browser_download_url);
  for (const overrides of [{ draft: true }, { prerelease: true }, { tag_name: 'v0.13.8-rc.1' }, { tag_name: 'v0.13.9' }, { published_at: null }, { assets: [asset({ state: 'new' })] }, { assets: [asset({ size: 0 })] }, { assets: [asset({ browser_download_url: 'https://evil.example/file.exe' })] }]) {
    assert.equal(normalizeGitHubReleases([release(overrides)], 'Norwi4/reqon').length, 0);
  }
});

test('published GitHub asset supersedes matching local copy without losing older versions', () => {
  const local = [{ id: 'local:a', platform: 'windows', version: '0.13.8', uploadedAt: '2026-11-02' }, { id: 'local:b', platform: 'windows', version: '0.13.7', uploadedAt: '2026-11-01' }];
  const remote = normalizeGitHubReleases([release()], 'Norwi4/reqon');
  const merged = mergeReleases(local, remote);
  assert.equal(merged.length, 2); assert.equal(merged[0].source, 'github'); assert.equal(merged[1].id, 'local:b');
});

test('local catalog detects additions/replacements/deletions and computes real SHA-256', async t => {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'reqon-releases-'));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const catalog = new LocalReleases(dir, dir);
  const name = 'Reqon-Setup-0.13.8-win64.exe';
  const file = path.join(dir, name);
  await writeFile(file, 'first');
  await writeFile(`${file}.sha256`, 'invalid old checksum');
  let data = await catalog.scan();
  assert.equal(data.length, 1);
  assert.equal(data[0].sha256, createHash('sha256').update('first').digest('hex'));
  await writeFile(file, 'replacement');
  await utimes(file, new Date(), new Date(Date.now() + 2000));
  data = await catalog.scan();
  assert.equal(data[0].size, 11); assert.equal(data[0].sha256, createHash('sha256').update('replacement').digest('hex'));
  await writeFile(path.join(dir, 'Reqon-Setup-0.13.10-win64.exe'), 'newest');
  await writeFile(path.join(dir, 'Reqon-Setup-0.14.0-beta-win64.exe'), 'beta');
  assert.equal((await catalog.scan())[0].version, '0.13.10');
  await rm(file);
  assert.equal((await catalog.scan()).length, 1);
});

test('GitHub follows pagination and keeps the last successful catalog after an outage', async () => {
  const calls = [];
  let failing = false;
  const github = new GitHubReleases('Norwi4/reqon', { fetchImpl: async url => {
    calls.push(url);
    if (failing) throw new Error('network unavailable');
    return { ok: true, json: async () => url.endsWith('page=1') ? Array.from({ length: 100 }, () => release({ assets: [] })) : [release()] };
  } });
  const first = await github.load();
  assert.equal(first.releases.length, 1); assert.equal(calls.length, 2); assert.equal(first.stale, false);
  failing = true; github.checkedAt = 0;
  const stale = await github.load();
  assert.equal(stale.stale, true); assert.equal(stale.releases[0].version, '0.13.8'); assert.equal(stale.syncedAt, first.syncedAt);
});

test('GitHub reports outage without inventing releases', async () => {
  const github = new GitHubReleases('Norwi4/reqon', { fetchImpl: async () => ({ ok: false, status: 403 }) });
  const data = await github.load(); assert.deepEqual(data.releases, []); assert.equal(data.stale, true);
});
