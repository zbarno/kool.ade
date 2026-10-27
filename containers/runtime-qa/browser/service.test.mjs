import assert from 'node:assert/strict';
import test from 'node:test';
import { runService } from './service.mjs';
import { AUTH_HEADER } from './relay.mjs';

test('service driver authorizes API requests through the preview relay', async (t) => {
  const token = 'a'.repeat(64);
  const previousToken = process.env.QA_RELAY_TOKEN;
  process.env.QA_RELAY_TOKEN = token;
  let receivedHeaders;
  t.mock.method(globalThis, 'fetch', async (_url, options) => {
    receivedHeaders = new Headers(options.headers);
    return new Response('{"saved":true}', { status: 200 });
  });
  try {
    await runService({
      origin: 'http://qa-preview:8080',
      timeout_ms: 5_000,
      actions: [{
        action: 'api_request',
        method: 'POST',
        path: '/save',
        json_body: '{"message":"synthetic"}',
        expected_status: 200,
        expected_body_contains: '"saved":true',
      }],
    });
  } finally {
    if (previousToken === undefined) delete process.env.QA_RELAY_TOKEN;
    else process.env.QA_RELAY_TOKEN = previousToken;
  }
  assert.equal(receivedHeaders.get(AUTH_HEADER), token);
});

for (const path of [
  '/./private',
  '/%2e/private',
  '/../private',
  '/%2e%2e/private',
  '/.%2e/private',
  '/%2e./private',
  '/%2E%2e/private',
]) {
  test(`rejects URL-normalized dot segment ${path}`, async () => {
    await assert.rejects(
      runService({
        actions: [{
          action: 'api_request',
          method: 'GET',
          path,
          expected_status: 200,
        }],
      }),
      /API action is not allowed by service QA policy/,
    );
  });
}
