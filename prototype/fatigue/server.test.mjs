import test from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import { once } from 'node:events';
import { createPreviewServer } from './server.mjs';

async function withServer(run) {
  const server = createPreviewServer();
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  try { await run(`http://127.0.0.1:${server.address().port}`); }
  finally { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); }
}

test('preview serves only demo assets with correct module types and no caching', async () => {
  await withServer(async base => {
    for (const [path, type] of [['/', 'text/html'], ['/engine.mjs', 'text/javascript'], ['/fixture.json', 'application/json']]) {
      const response = await fetch(base + path);
      assert.equal(response.status, 200);
      assert.ok(response.headers.get('content-type').startsWith(type));
      assert.equal(response.headers.get('cache-control'), 'no-store');
    }
    for (const path of ['/patient_files/', '/.git/config', '/server.mjs', '/%2e%2e/package.json']) {
      assert.equal((await fetch(base + path)).status, 404, path);
    }
  });
});

test('preview rejects foreign origins and write requests', async () => {
  await withServer(async base => {
    assert.equal((await fetch(base, { headers: { Origin: 'https://example.invalid' } })).status, 403);
    assert.equal((await fetch(base, { method: 'POST', body: 'not stored' })).status, 405);
    const response = await fetch(base, { method: 'HEAD' });
    assert.equal(response.status, 200);
    assert.equal(await response.text(), '');
  });
});

test('preview rejects a host header unrelated to loopback', async () => {
  await withServer(async base => {
    const status = await new Promise((resolve, reject) => {
      const req = http.get(base, { headers: { Host: 'example.invalid' } }, response => {
        response.resume();
        response.once('end', () => resolve(response.statusCode));
      });
      req.on('error', reject);
    });
    assert.equal(status, 403);
  });
});
