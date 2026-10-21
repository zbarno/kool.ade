import assert from 'node:assert/strict';
import test from 'node:test';
import { runService } from './service.mjs';

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
