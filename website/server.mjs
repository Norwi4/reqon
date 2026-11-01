import http from 'node:http';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createReadStream } from 'node:fs';
import { lstat, readFile } from 'node:fs/promises';
import { pipeline } from 'node:stream/promises';
import { LocalReleases, GitHubReleases, ReleaseCatalog, parseAsset } from './src/releases.mjs';

const root = path.dirname(fileURLToPath(import.meta.url));
const mime = { '.html': 'text/html; charset=utf-8', '.css': 'text/css; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.svg': 'image/svg+xml', '.png': 'image/png', '.webp': 'image/webp', '.ico': 'image/x-icon', '.ttf': 'font/ttf', '.txt': 'text/plain; charset=utf-8', '.xml': 'application/xml; charset=utf-8', '.json': 'application/json; charset=utf-8', '.md': 'text/plain; charset=utf-8' };

export function parseRange(value, size) {
  if (!value) return null;
  const match = /^bytes=(\d*)-(\d*)$/.exec(value);
  if (!match || (!match[1] && !match[2]) || size === 0) return false;
  let start = match[1] ? Number(match[1]) : Math.max(0, size - Number(match[2]));
  let end = match[1] ? (match[2] ? Math.min(Number(match[2]), size - 1) : size - 1) : size - 1;
  if (!Number.isSafeInteger(start) || !Number.isSafeInteger(end) || start > end || start >= size || start < 0) return false;
  return { start, end };
}

export function createWebsite({ publicDir = path.join(root, 'public'), releaseDir = path.resolve(root, '../dist'), catalog, siteUrl = '' } = {}) {
  catalog ??= new ReleaseCatalog(new LocalReleases(releaseDir, path.join(publicDir, 'notes')));
  const respond = (res, code, body, type = 'text/plain; charset=utf-8', method = 'GET') => {
    const data = Buffer.from(body);
    res.writeHead(code, { 'Content-Type': type, 'Content-Length': data.length });
    res.end(method === 'HEAD' ? undefined : data);
  };
  return http.createServer(async (req, res) => {
    res.setHeader('X-Content-Type-Options', 'nosniff');
    res.setHeader('Referrer-Policy', 'strict-origin-when-cross-origin');
    res.setHeader('X-Frame-Options', 'DENY');
    res.setHeader('Content-Security-Policy', "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self'; font-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'self'; frame-ancestors 'none'; form-action 'self'");
    const method = req.method;
    if (!['GET', 'HEAD'].includes(method)) { res.setHeader('Allow', 'GET, HEAD'); return respond(res, 405, 'Method not allowed', undefined, method); }
    try {
      // Validate before URL normalization, so encoded and literal traversal are both rejected.
      const pathname = decodeURIComponent(req.url.split('?')[0]);
      if (pathname.includes('\\') || pathname.includes('\0') || pathname.split('/').some(p => p === '..' || p.startsWith('.'))) return respond(res, 400, 'Invalid path', undefined, method);
      if (pathname === '/healthz') return respond(res, 200, 'ok', undefined, method);
      if (pathname === '/api/releases') {
        const data = await catalog.load();
        res.setHeader('Cache-Control', 'no-store');
        return respond(res, data.githubStale && !data.releases.length ? 503 : 200, JSON.stringify(data), mime['.json'], method);
      }
      if (pathname.startsWith('/download/latest/')) {
        const platform = pathname.slice('/download/latest/'.length);
        const release = (await catalog.load()).latest[platform];
        if (!release) return respond(res, 404, 'Release not available', undefined, method);
        res.writeHead(302, { Location: release.url, 'Cache-Control': 'no-store' });
        return res.end();
      }
      if (pathname.startsWith('/files/')) {
        const filename = pathname.slice(7);
        const checksum = filename.endsWith('.sha256');
        const binaryName = checksum ? filename.slice(0, -7) : filename;
        if (!parseAsset(binaryName)) return respond(res, 404, 'File not found', undefined, method);
        // Local files remain available even if an equivalent GitHub asset was published.
        const release = (await catalog.local.scan()).find(item => item.filename === binaryName);
        if (!release) return respond(res, 404, 'File not found', undefined, method);
        res.setHeader('Content-Disposition', `attachment; filename="${filename}"`);
        if (checksum) return respond(res, 200, `${release.sha256}  ${binaryName}\n`, mime['.txt'], method);
        const file = path.join(releaseDir, binaryName);
        const stat = await lstat(file);
        if (!stat.isFile() || stat.size !== release.size || stat.mtime.toISOString() !== release.uploadedAt) return respond(res, 409, 'Release changed; please retry', undefined, method);
        const etag = `"${release.sha256}"`;
        res.setHeader('ETag', etag);
        res.setHeader('Accept-Ranges', 'bytes');
        res.setHeader('Cache-Control', 'no-cache');
        res.setHeader('Last-Modified', stat.mtime.toUTCString());
        if (!req.headers.range && req.headers['if-none-match'] === etag) { res.writeHead(304); return res.end(); }
        const range = parseRange(!req.headers['if-range'] || req.headers['if-range'] === etag ? req.headers.range : null, stat.size);
        if (range === false) { res.setHeader('Content-Range', `bytes */${stat.size}`); return respond(res, 416, 'Range not satisfiable', undefined, method); }
        if (range) res.setHeader('Content-Range', `bytes ${range.start}-${range.end}/${stat.size}`);
        res.writeHead(range ? 206 : 200, { 'Content-Type': 'application/octet-stream', 'Content-Length': range ? range.end - range.start + 1 : stat.size });
        if (method === 'HEAD') return res.end();
        return await pipeline(createReadStream(file, range || {}), res);
      }
      if (pathname === '/robots.txt') return respond(res, 200, `User-agent: *\nAllow: /\nDisallow: /api/\nDisallow: /files/\n${siteUrl ? `Sitemap: ${siteUrl}/sitemap.xml\n` : ''}`, mime['.txt'], method);
      if (pathname === '/sitemap.xml' && siteUrl) {
        const pages = ['/', '/features/', '/download/', '/docs/', '/changelog/', '/about/'];
        return respond(res, 200, `<?xml version="1.0" encoding="UTF-8"?><urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">${pages.map(p => `<url><loc>${siteUrl}${p}</loc></url>`).join('')}</urlset>`, mime['.xml'], method);
      }
      const relative = pathname.replace(/^\/+/, '');
      let file = path.resolve(publicDir, relative || 'index.html');
      if (!file.startsWith(path.resolve(publicDir) + path.sep)) return respond(res, 400, 'Invalid path', undefined, method);
      let stat;
      try {
        stat = await lstat(file);
        if (stat.isDirectory()) {
          if (!pathname.endsWith('/')) { res.writeHead(308, { Location: `${pathname}/` }); return res.end(); }
          file = path.join(file, 'index.html'); stat = await lstat(file);
        }
        if (!stat.isFile()) throw new Error('Not a regular file');
      } catch {
        return respond(res, 404, await readFile(path.join(publicDir, '404.html'), 'utf8'), mime['.html'], method);
      }
      res.setHeader('Cache-Control', ['.html', '.css', '.js'].includes(path.extname(file)) ? 'no-cache' : 'public, max-age=3600');
      res.writeHead(200, { 'Content-Type': mime[path.extname(file)] || 'application/octet-stream', 'Content-Length': stat.size });
      if (method === 'HEAD') return res.end();
      await pipeline(createReadStream(file), res);
    } catch (error) {
      if (res.headersSent) { res.destroy(); return; }
      if (error instanceof URIError) return respond(res, 400, 'Invalid path', undefined, method);
      console.error('Request failed:', error.code || error.name);
      respond(res, 500, 'Service temporarily unavailable', undefined, method);
    }
  });
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const releaseDir = path.resolve(process.env.RELEASES_DIR || path.join(root, '../dist'));
  const repository = process.env.GITHUB_REPO ?? 'Norwi4/reqon';
  const mode = process.env.RELEASE_SOURCE || 'hybrid';
  if (!['local', 'github', 'hybrid'].includes(mode)) throw new Error('RELEASE_SOURCE must be local, github or hybrid');
  const local = new LocalReleases(releaseDir, path.join(root, 'public/notes'));
  if (mode === 'github') local.scan = async () => [];
  const github = mode !== 'local' ? new GitHubReleases(repository, { token: process.env.GITHUB_TOKEN, cacheFile: path.join(root, '.cache/github-releases.json') }) : null;
  const catalog = new ReleaseCatalog(local, github);
  const siteUrl = process.env.SITE_URL ? new URL(process.env.SITE_URL).origin : '';
  const server = createWebsite({ releaseDir, catalog, siteUrl });
  const host = process.env.HOST || '127.0.0.1';
  const port = Number(process.env.PORT || 4173);
  server.listen(port, host, () => console.log(`Reqon website: http://${host}:${port}\nReleases: ${mode} (${repository})`));
  for (const signal of ['SIGTERM', 'SIGINT']) process.on(signal, () => server.close(() => process.exit(0)));
}
