import assert from 'node:assert/strict';
import http from 'node:http';
import net from 'node:net';
import test from 'node:test';
import { AUTH_HEADER, createRelay } from './relay.mjs';

const TOKEN = 'a'.repeat(64);

async function listen(server) {
  await new Promise((resolve, reject) => {
    server.once('listening', resolve);
    server.once('error', reject);
    server.listen(0, '127.0.0.1');
  });
  return server.address().port;
}

function startHttpTarget() {
  const seen = [];
  const target = http.createServer((request, response) => {
    seen.push({ headers: { ...request.headers }, method: request.method, path: request.url });
    response.writeHead(200, { 'content-type': 'text/plain' });
    response.end('ok');
  });
  return { target, seen, port: null };
}

function startTcpTarget() {
  const received = [];
  const target = net.createServer((socket) => {
    socket.once('data', (chunk) => received.push(chunk.toString('latin1')));
    socket.end('HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\r\n');
  });
  return { target, received, port: null };
}

async function get(port, { host = 'qa-preview:8080', token, path = '/' } = {}) {
  return await new Promise((resolve, reject) => {
    const headers = { host, 'connection': 'close' };
    if (token !== undefined) headers[AUTH_HEADER] = token;
    const request = http.request({ host: '127.0.0.1', port, path, method: 'GET', headers }, (response) => {
      let body = '';
      response.on('data', (chunk) => (body += chunk));
      response.on('end', () => resolve({ status: response.statusCode, body }));
    });
    request.on('error', reject);
    request.setTimeout(3_000, () => request.destroy(new Error('probe timed out')));
    request.end();
  });
}

function upgrade(port, { host = 'qa-preview:8080', token } = {}) {
  return new Promise((resolve, reject) => {
    const socket = net.connect(port, '127.0.0.1', () => {
      const headers = [`Host: ${host}`, 'Upgrade: websocket', 'Connection: Upgrade'];
      if (token !== undefined) headers.push(`${AUTH_HEADER}: ${token}`);
      socket.write(['GET / HTTP/1.1', ...headers, '', ''].join('\r\n'));
    });
    let payload = '';
    socket.once('data', (chunk) => {
      payload += chunk.toString('latin1');
      socket.end();
      resolve(payload);
    });
    socket.on('error', (error) => reject(error));
    socket.setTimeout(3_000, () => socket.destroy(new Error('upgrade probe timed out')));
  });
}

test('historic relay without a token keeps the host-check-only behavior', async () => {
  const { target, seen } = startHttpTarget();
  target.port = await listen(target);
  const relay = createRelay({ previewHost: 'qa-preview:8080', targetHost: '127.0.0.1', targetPort: target.port });
  const port = await listen(relay);
  try {
    assert.equal((await get(port, { host: 'elsewhere:9' })).status, 421);
    const admitted = await get(port);
    assert.equal(admitted.status, 200);
    assert.equal(admitted.body, 'ok');
    assert.equal(seen.at(-1).headers.host, `127.0.0.1:${target.port}`);
  } finally {
    relay.close();
    target.close();
  }
});

test('token relay rejects anonymous, wrong-token, wrong-length and misdirected callers', async () => {
  const { target } = startHttpTarget();
  target.port = await listen(target);
  const relay = createRelay({ previewHost: 'qa-preview:8080', targetHost: '127.0.0.1', targetPort: target.port, token: TOKEN });
  const port = await listen(relay);
  try {
    assert.equal((await get(port, {})).status, 421);
    assert.equal((await get(port, { token: 'b'.repeat(64) })).status, 421);
    assert.equal((await get(port, { token: 'b'.repeat(16) })).status, 421);
    assert.equal((await get(port, { host: 'qa-preview:8081', token: TOKEN })).status, 421);
  } finally {
    relay.close();
    target.close();
  }
});

test('token relay admits the bearer and scrubs the token from the forwarded request', async () => {
  const { target, seen } = startHttpTarget();
  target.port = await listen(target);
  const relay = createRelay({ previewHost: 'qa-preview:8080', targetHost: '127.0.0.1', targetPort: target.port, token: TOKEN });
  const port = await listen(relay);
  try {
    const admitted = await get(port, { token: TOKEN, path: '/settings?page=1' });
    assert.equal(admitted.status, 200);
    assert.equal(admitted.body, 'ok');
    assert.equal(seen.at(-1).path, '/settings?page=1');
    assert.equal(AUTH_HEADER in seen.at(-1).headers, false);
  } finally {
    relay.close();
    target.close();
  }
});

test('websocket upgrades demand the token and forward the protocol headers', async () => {
  const { target, received } = startTcpTarget();
  target.port = await listen(target);
  const relay = createRelay({ previewHost: 'qa-preview:8080', targetHost: '127.0.0.1', targetPort: target.port, token: TOKEN });
  const port = await listen(relay);
  try {
    assert.match(await upgrade(port, {}), /^HTTP\/1\.1 421/);
    assert.match(await upgrade(port, { token: 'b'.repeat(64) }), /^HTTP\/1\.1 421/);
    const accepted = await upgrade(port, { token: TOKEN });
    assert.match(accepted, /^HTTP\/1\.1 101/);
    assert.equal(received.length, 1);
    assert.match(received[0], /^GET \/ HTTP\//);
    assert.match(received[0], /Upgrade: websocket/i);
    assert.doesNotMatch(received[0], /x-koolade-qa-relay-token/i);
  } finally {
    relay.close();
    target.close();
  }
});
