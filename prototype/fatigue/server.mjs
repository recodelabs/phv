import http from 'node:http';
import { readFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import { readVaultSnapshot } from './vault-adapter.mjs';
import { readPersonalCase } from './personal-case.mjs';

const assets = new Map([
  ['/', ['index.html', 'text/html; charset=utf-8']],
  ['/index.html', ['index.html', 'text/html; charset=utf-8']],
  ['/app.mjs', ['app.mjs', 'text/javascript; charset=utf-8']],
  ['/engine.mjs', ['engine.mjs', 'text/javascript; charset=utf-8']],
  ['/personal-engine.mjs', ['personal-engine.mjs', 'text/javascript; charset=utf-8']],
  ['/styles.css', ['styles.css', 'text/css; charset=utf-8']],
  ['/fixture.json', ['fixture.json', 'application/json; charset=utf-8']],
]);

// Serve only this demo's explicit assets, never the repository or patient_files.
export function createPreviewServer(options = {}) {
  if (options.personalCase && options.vaultPath) throw new Error('Choose either a synthetic vault or a personal demo.');
  return http.createServer(async (request, response) => {
    response.setHeader('Cache-Control', 'no-store');
    response.setHeader('X-Content-Type-Options', 'nosniff');
    response.setHeader('Referrer-Policy', 'no-referrer');
    response.setHeader('Content-Security-Policy', "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'");
    const host = request.headers.host;
    const allowed = host && /^(127\.0\.0\.1|localhost)(:\d+)?$/.test(host);
    const origin = request.headers.origin;
    if (!allowed || (origin && origin !== `http://${host}`)) {
      response.writeHead(403, { 'Content-Type': 'text/plain; charset=utf-8' });
      response.end('This preview accepts local requests only.');
      return;
    }
    if (!['GET', 'HEAD'].includes(request.method)) {
      response.writeHead(405, { Allow: 'GET, HEAD' });
      response.end();
      return;
    }
    let pathname;
    try {
      pathname = decodeURIComponent(new URL(request.url, `http://${host}`).pathname);
    } catch {
      response.writeHead(400);
      response.end('Invalid URL.');
      return;
    }
    if (pathname === '/favicon.ico') {
      response.writeHead(204);
      response.end();
      return;
    }
    if (pathname === '/personal-case' || pathname.startsWith('/personal-source/')) {
      if (!options.personalCase) { response.writeHead(404); response.end(); return; }
      try {
        const {fixture, files} = await readPersonalCase(options.personalCase);
        const sourceId = pathname.slice('/personal-source/'.length);
        const bytes = pathname === '/personal-case' ? Buffer.from(JSON.stringify(fixture)) : files.get(sourceId);
        if (!bytes) { response.writeHead(404); response.end(); return; }
        response.writeHead(200, {'Content-Type': pathname === '/personal-case' ? 'application/json; charset=utf-8' : 'application/pdf', 'Content-Length': bytes.length,
          ...(pathname === '/personal-case' ? {} : {'Content-Disposition': `inline; filename="report-${sourceId}.pdf"`})});
        response.end(request.method === 'HEAD' ? undefined : bytes);
      } catch {
        response.writeHead(409, {'Content-Type': 'application/json; charset=utf-8'});
        response.end(JSON.stringify({error: 'The private demo or its original PDFs changed or are unavailable. Check the configured manifest and source hashes, then reopen the reports.'}));
      }
      return;
    }
    if (pathname === '/preview-config' || pathname === '/vault-snapshot') {
      try {
        const value = pathname === '/preview-config' ? {vaultEnabled: Boolean(options.vaultPath), personalEnabled: Boolean(options.personalCase)} : await readVaultSnapshot(options);
        const bytes = Buffer.from(JSON.stringify(value));
        response.writeHead(200, {'Content-Type': 'application/json; charset=utf-8', 'Content-Length': bytes.length});
        response.end(request.method === 'HEAD' ? undefined : bytes);
      } catch {
        response.writeHead(409, {'Content-Type': 'application/json; charset=utf-8'});
        response.end(JSON.stringify({error: 'The synthetic vault could not be captured. Check its patient marker, sources and snapshot command, then retry.'}));
      }
      return;
    }
    const asset = assets.get(pathname);
    if (!asset) {
      response.writeHead(404, { 'Content-Type': 'text/plain; charset=utf-8' });
      response.end('Not found.');
      return;
    }
    try {
      const bytes = await readFile(new URL(asset[0], import.meta.url));
      response.writeHead(200, { 'Content-Type': asset[1], 'Content-Length': bytes.length });
      response.end(request.method === 'HEAD' ? undefined : bytes);
    } catch {
      response.writeHead(503, { 'Content-Type': 'text/plain; charset=utf-8' });
      response.end('A demo asset is unavailable. Restore the checkout files and retry.');
    }
  });
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const args = process.argv.slice(2), options = {};
    let port = Number(process.env.PHV_DEMO_PORT ?? 4280);
    for (let i = 0; i < args.length; i++) {
      if (args[i] === '--port' && args[i + 1]) port = Number(args[++i]);
      else if (args[i] === '--vault' && args[i + 1]) options.vaultPath = args[++i];
      else if (args[i] === '--phv-bin' && args[i + 1]) options.phvBinary = args[++i];
      else if (args[i] === '--personal-case' && args[i + 1]) options.personalCase = args[++i];
      else throw new Error('Usage: node server.mjs [--port 4280] [--vault SYNTHETIC_VAULT --phv-bin PHV_BINARY | --personal-case PRIVATE_MANIFEST]');
    }
    if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error('Choose a port between 1 and 65535.');
    const server = createPreviewServer(options);
    server.on('error', error => { console.error(`Could not start preview: ${error.code ?? error.message}`); process.exitCode = 1; });
    server.listen(port, '127.0.0.1', () => console.log(`Local fatigue ${options.personalCase ? 'personal lab demo' : 'synthetic preview'}: http://127.0.0.1:${port}/`));
    for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => server.close());
  } catch (error) { console.error(error.message); process.exitCode = 2; }
}
