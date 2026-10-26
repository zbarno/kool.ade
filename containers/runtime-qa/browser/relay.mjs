import http from 'node:http';
import net from 'node:net';
import { timingSafeEqual } from 'node:crypto';
import { pathToFileURL } from 'node:url';
import process from 'node:process';

export const DEFAULT_MAX_REQUEST_BYTES = 16 * 1024 * 1024;
export const AUTH_HEADER = 'x-koolade-qa-relay-token';

function validShape(request, previewHost) {
  return request.headers.host === previewHost &&
    typeof request.url === 'string' && request.url.startsWith('/') &&
    !request.url.startsWith('//') && !request.url.includes('\0');
}

function tokenAuthorized(request, token) {
  if (!token) return true;
  const presented = request.headers[AUTH_HEADER];
  if (typeof presented !== 'string' || presented.length !== token.length) return false;
  try {
    return timingSafeEqual(Buffer.from(presented, 'utf8'), Buffer.from(token, 'utf8'));
  } catch {
    return false;
  }
}

function headersForTarget(headers, targetHost, targetPort) {
  const safe = { ...headers, host: `${targetHost}:${targetPort}` };
  for (const name of ['connection', 'proxy-connection', 'proxy-authorization', 'keep-alive', 'te', 'trailer', 'transfer-encoding', 'upgrade']) {
    delete safe[name];
  }
  delete safe[AUTH_HEADER];
  return safe;
}

/// Factory for the app-owned preview relay. When `token` is set, every
/// proxied request and websocket upgrade must present it, binding the relay
/// to the single driver container that received the run-scoped secret rather
/// than to a hostname and numeric port alone. An empty token reproduces the
/// historic behavior so older published images keep interoping.
export function createRelay({ previewHost, targetHost, targetPort, token = '', maxRequestBytes = DEFAULT_MAX_REQUEST_BYTES }) {
  const relay = http.createServer({ maxHeaderSize: 16 * 1024 }, (request, response) => {
    if (!validShape(request, previewHost)) {
      response.writeHead(421, { connection: 'close' }).end();
      return;
    }
    // The liveness path answers without the run token so the relay's own
    // healthcheck keeps working; it exposes no data, only 204.
    if (request.url === '/__koolade_health') {
      response.writeHead(204).end();
      return;
    }
    if (!tokenAuthorized(request, token)) {
      response.writeHead(421, { connection: 'close' }).end();
      return;
    }
    let size = 0;
    request.on('data', (chunk) => {
      size += chunk.length;
      if (size > maxRequestBytes) request.destroy(new Error('preview request body exceeds relay limit'));
    });
    const target = http.request({
      host: targetHost,
      port: targetPort,
      method: request.method,
      path: request.url,
      headers: headersForTarget(request.headers, targetHost, targetPort),
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
    if (!validShape(request, previewHost) || !tokenAuthorized(request, token) ||
        request.headers.upgrade?.toLowerCase() !== 'websocket') {
      client.end('HTTP/1.1 421 Misdirected Request\r\nConnection: close\r\n\r\n');
      return;
    }
    const upstream = net.connect({ host: targetHost, port: targetPort });
    upstream.once('connect', () => {
      const headers = headersForTarget(request.headers, targetHost, targetPort);
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
  return relay;
}

function waitForTaskService({ targetHost, targetPort, healthPath, deadline }) {
  return new Promise((resolve, reject) => {
    let settled = false;
    let activeProbe;
    let lastFailure = 'no health response';
    const finish = (error) => {
      if (settled) return;
      settled = true;
      if (error) activeProbe?.destroy();
      if (error) reject(error);
      else resolve();
    };
    const deadlineTimer = setTimeout(() => {
      finish(new Error(`task preview did not become ready: ${lastFailure}`));
    }, Math.max(0, deadline - Date.now()));
    const check = () => {
      if (settled) return;
      const probe = http.get({ host: targetHost, port: targetPort, path: healthPath, timeout: 2_000 }, (response) => {
        response.resume();
        if (response.statusCode && response.statusCode < 500) {
          clearTimeout(deadlineTimer);
          finish();
        } else retry(`HTTP ${response.statusCode ?? 'unknown'}`);
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

const isEntry = process.argv[1] !== undefined && import.meta.url === pathToFileURL(process.argv[1]).href;
if (isEntry) {
  const TARGET_HOST = 'qa-task';
  const PREVIEW_HOST = 'qa-preview:8080';
  const TARGET_PORT = Number(process.env.QA_TARGET_PORT);
  const HEALTH_PATH = process.env.QA_HEALTH_PATH ?? '/';
  const TOKEN = process.env.QA_RELAY_TOKEN ?? '';
  const STARTUP_TIMEOUT_MS = 60_000;
  if (!Number.isInteger(TARGET_PORT) || TARGET_PORT < 1024 || TARGET_PORT > 65535 ||
      !HEALTH_PATH.startsWith('/') || HEALTH_PATH.startsWith('//') || HEALTH_PATH.includes('\0')) {
    throw new Error('invalid app-owned QA relay configuration');
  }
  const relay = createRelay({
    previewHost: PREVIEW_HOST,
    targetHost: TARGET_HOST,
    targetPort: TARGET_PORT,
    token: TOKEN,
  });
  void relay.listen(8080, '0.0.0.0');
  void waitForTaskService({
    targetHost: TARGET_HOST,
    targetPort: TARGET_PORT,
    healthPath: HEALTH_PATH,
    deadline: Date.now() + STARTUP_TIMEOUT_MS,
  }).catch((error) => {
    process.stderr.write(`${error?.message ?? error}\n`);
    process.exitCode = 1;
  });
}
