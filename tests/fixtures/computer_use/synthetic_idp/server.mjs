import { createServer } from 'node:http';
import { randomBytes } from 'node:crypto';

export const SYNTHETIC_IDENTITY = Object.freeze({
  username: 'fixture-owner@example.test',
  password: 'synthetic-only-password-7f3c',
  mfaCode: '000731',
});

const LOGIN_PAGE = `<!doctype html>
<html lang="en"><meta charset="utf-8"><title>Synthetic sign in</title>
<main><h1>Synthetic identity provider</h1><form method="post" action="/login">
<label>Email <input name="username" type="email" autocomplete="username" required></label>
<label>Password <input name="password" type="password" autocomplete="current-password" required></label>
<button>Continue</button></form><p role="status">Synthetic fixture only</p></main></html>`;

const MFA_PAGE = `<!doctype html>
<html lang="en"><meta charset="utf-8"><title>Synthetic MFA</title>
<main><h1>Verify synthetic sign in</h1><p role="status">MFA required</p>
<form method="post" action="/mfa"><label>One-time code <input name="code" inputmode="numeric" autocomplete="one-time-code" required></label>
<button>Verify</button></form><p id="error" role="alert"></p></main></html>`;

function send(response, status, body, headers = {}) {
  response.writeHead(status, {
    'content-type': 'text/html; charset=utf-8',
    'cache-control': 'no-store',
    'content-length': Buffer.byteLength(body),
    ...headers,
  });
  response.end(body);
}

function cookie(request, name) {
  const value = request.headers.cookie ?? '';
  return value.split(';').map((part) => part.trim())
    .find((part) => part.startsWith(`${name}=`))?.slice(name.length + 1);
}

async function form(request) {
  let body = '';
  for await (const chunk of request) {
    body += chunk;
    if (body.length > 4096) throw new Error('form too large');
  }
  return new URLSearchParams(body);
}

export async function startSyntheticIdp() {
  const pending = new Set();
  const authenticated = new Set();
  const server = createServer(async (request, response) => {
    try {
      const url = new URL(request.url ?? '/', 'http://127.0.0.1');
      if (request.method === 'GET' && url.pathname === '/login') {
        send(response, 200, LOGIN_PAGE);
      } else if (request.method === 'POST' && url.pathname === '/login') {
        const submitted = await form(request);
        if (submitted.get('username') !== SYNTHETIC_IDENTITY.username
          || submitted.get('password') !== SYNTHETIC_IDENTITY.password) {
          send(response, 401, `${LOGIN_PAGE}<p role="alert">Synthetic credentials rejected</p>`);
          return;
        }
        const challenge = randomBytes(24).toString('hex');
        pending.add(challenge);
        send(response, 200, MFA_PAGE, {
          'set-cookie': `fixture_challenge=${challenge}; HttpOnly; SameSite=Strict; Path=/`,
        });
      } else if (request.method === 'POST' && url.pathname === '/mfa') {
        const submitted = await form(request);
        const challenge = cookie(request, 'fixture_challenge');
        if (!challenge || !pending.has(challenge)
          || submitted.get('code') !== SYNTHETIC_IDENTITY.mfaCode) {
          send(response, 401, `${MFA_PAGE.replace('</main>', '<p role="alert">Code rejected</p></main>')}`);
          return;
        }
        pending.delete(challenge);
        const session = randomBytes(24).toString('hex');
        authenticated.add(session);
        send(response, 303, '', {
          location: '/account',
          'set-cookie': [
            'fixture_challenge=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0',
            `fixture_session=${session}; HttpOnly; SameSite=Strict; Path=/; Max-Age=3600`,
          ],
        });
      } else if (request.method === 'GET' && url.pathname === '/account') {
        if (!authenticated.has(cookie(request, 'fixture_session'))) {
          send(response, 401, '<!doctype html><p role="status">Sign in required</p>');
          return;
        }
        send(response, 200, '<!doctype html><main><h1>Fixture account</h1><p role="status">Authenticated by synthetic IdP</p></main>');
      } else {
        send(response, 404, '<!doctype html><p>Not found</p>');
      }
    } catch {
      send(response, 400, '<!doctype html><p>Invalid fixture request</p>');
    }
  });
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const address = server.address();
  return {
    origin: `http://127.0.0.1:${address.port}`,
    close: () => new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve())),
  };
}
