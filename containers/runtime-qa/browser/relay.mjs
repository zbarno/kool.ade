import http from 'node:http';
import net from 'node:net';
import process from 'node:process';

const TARGET_HOST = 'qa-task';
const PREVIEW_HOST = 'qa-preview:8080';
const TARGET_PORT = Number(process.env.QA_TARGET_PORT);
const HEALTH_PATH = process.env.QA_HEALTH_PATH ?? '/';
const STARTUP_TIMEOUT_MS = 60_000;
const MAX_REQUEST_BYTES = 16 * 1024 * 1024;

if (!Number.isInteger(TARGET_PORT) || TARGET_PORT < 1024 || TARGET_PORT > 65535 ||
    !HEALTH_PATH.startsWith('/') || HEALTH_PATH.startsWith('//') || HEALTH_PATH.includes('\0')) {
  throw new Error('invalid app-owned QA relay configuration');
}

function validRequest(request) {
  return request.headers.host === PREVIEW_HOST &&
    typeof request.url === 'string' && request.url.startsWith('/') &&
    !request.url.startsWith('//') && !request.url.includes('\0');
}

function headersForTarget(headers) {
  const safe = { ...headers, host: `${TARGET_HOST}:${TARGET_PORT}` };
  for (const name of ['connection', 'proxy-connection', 'proxy-authorization', 'keep-alive', 'te', 'trailer', 'transfer-encoding', 'upgrade']) {
    delete safe[name];
  }
  return safe;
}

const relay = http.createServer({ maxHeaderSize: 16 * 1024 }, (request, response) => {
  if (!validRequest(request)) {
    response.writeHead(421, { connection: 'close' }).end();
    return;
  }
  if (request.url === '/__koolade_health') {
    response.writeHead(204).end();
    return;
  }
  let size = 0;
  request.on('data', (chunk) => {
    size += chunk.length;
    if (size > MAX_REQUEST_BYTES) request.destroy(new Error('preview request body exceeds relay limit'));
  });
  const target = http.request({
    host: TARGET_HOST,
    port: TARGET_PORT,
    method: request.method,
    path: request.url,
    headers: headersForTarget(request.headers),
    timeout: 60_000,
  }, (upstream) => {
    response.writeHead(upstream.statusCode ?? 502, upstream.headers);
    upstream.pipe(response);
  });
  target.on('timeout', () => target.destroy(new Error('preview service timed out')));
  target.on('error', () => {
    if (!response.headersSent) response.writeHead(502);
    response.end();
  });
  request.pipe(target);
});

relay.on('upgrade', (request, client, head) => {
  if (!validRequest(request) || request.headers.upgrade?.toLowerCase() !== 'websocket') {
    client.end('HTTP/1.1 421 Misdirected Request\r\nConnection: close\r\n\r\n');
    return;
  }
  const upstream = net.connect({ host: TARGET_HOST, port: TARGET_PORT });
  upstream.once('connect', () => {
    const headers = headersForTarget(request.headers);
    headers.connection = 'Upgrade';
    headers.upgrade = 'websocket';
    const lines = [`${request.method} ${request.url} HTTP/${request.httpVersion}`];
    for (const [name, value] of Object.entries(headers)) {
      if (Array.isArray(value)) value.forEach((entry) => lines.push(`${name}: ${entry}`));
      else if (value !== undefined) lines.push(`${name}: ${value}`);
    }
    upstream.write(`${lines.join('\r\n')}\r\n\r\n`);
    if (head.length) upstream.write(head);
    client.pipe(upstream).pipe(client);
  });
  upstream.on('error', () => client.destroy());
  client.on('error', () => upstream.destroy());
});

function waitForTaskService(deadline) {
  return new Promise((resolve, reject) => {
    let settled = false;
    let activeProbe;
    let lastFailure = 'no health response';
    let deadlineTimer;
    const finish = (error) => {
      if (settled) return;
      settled = true;
      clearTimeout(deadlineTimer);
      if (error) activeProbe?.destroy();
      if (error) reject(error);
      else resolve();
    };
    deadlineTimer = setTimeout(() => {
      finish(new Error(`task preview did not become ready: ${lastFailure}`));
    }, Math.max(0, deadline - Date.now()));
    const check = () => {
      if (settled) return;
      const probe = http.get({ host: TARGET_HOST, port: TARGET_PORT, path: HEALTH_PATH, timeout: 2_000 }, (response) => {
        response.resume();
        if (response.statusCode && response.statusCode < 500) finish();
        else retry(`HTTP ${response.statusCode ?? 'unknown'}`);
      });
      activeProbe = probe;
      let retried = false;
      const retry = (reason) => {
        if (retried || settled) return;
        retried = true;
        lastFailure = reason;
        probe.destroy();
        if (!settled) setTimeout(check, 250);
      };
      probe.on('error', (error) => retry(error.code ?? error.message));
      probe.on('timeout', () => retry('health probe timed out'));
    };
    check();
  });
}

await waitForTaskService(Date.now() + STARTUP_TIMEOUT_MS);
relay.listen(8080, '0.0.0.0');
