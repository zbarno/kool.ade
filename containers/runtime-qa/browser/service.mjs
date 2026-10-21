import { createHash, randomUUID } from 'node:crypto';
import process from 'node:process';

const ACTION_TIMEOUT_MS = 15_000;

function fail(message) {
  throw new Error(message);
}

function validServiceAction(action) {
  const methods = new Set(['GET', 'POST', 'PUT', 'PATCH', 'DELETE']);
  if (!action || action.action !== 'api_request' || !methods.has(action.method) ||
      (action.method === 'GET' && action.json_body != null) ||
      typeof action.path !== 'string' || !action.path.startsWith('/') || action.path.startsWith('//') ||
      action.path.length > 2048 || /[?#\\\0]/.test(action.path) || /[\x00-\x1f\x7f]/.test(action.path) ||
      action.path.split('/').some((part) => part === '.' || part === '..') ||
      !Number.isInteger(action.expected_status) || action.expected_status < 100 || action.expected_status > 599 ||
      (action.json_body != null && (typeof action.json_body !== 'string' || action.json_body.length > 16 * 1024)) ||
      (action.expected_body_contains != null && (typeof action.expected_body_contains !== 'string' || action.expected_body_contains.length === 0 || action.expected_body_contains.length > 4096))) {
    return false;
  }
  if (action.json_body != null) {
    try { JSON.parse(action.json_body); } catch { return false; }
  }
  return true;
}

async function limitedBody(response) {
  if (!response.body) return Buffer.alloc(0);
  const reader = response.body.getReader();
  const chunks = [];
  let length = 0;
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    length += value.byteLength;
    if (length > 1024 * 1024) {
      await reader.cancel();
      fail('API response exceeded the one MiB evidence limit');
    }
    chunks.push(Buffer.from(value));
  }
  return Buffer.concat(chunks, length);
}

export async function runService(request) {
  if (!request.actions.every(validServiceAction)) fail('API action is not allowed by service QA policy');
  const deadline = Date.now() + request.timeout_ms;
  const actions = [];
  const failedRequests = [];
  let outcome = 'completed';
  let errorText = null;
  for (let index = 0; index < request.actions.length; index += 1) {
    const action = request.actions[index];
    const url = new URL(action.path, `${request.origin}/`);
    if (url.origin !== request.origin || url.protocol !== 'http:' || url.username || url.password) {
      fail('API request escaped its app-owned relay origin');
    }
    const evidenceId = randomUUID();
    try {
      const remaining = deadline - Date.now();
      if (remaining <= 0) fail('service QA run timed out');
      const response = await fetch(url, {
        method: action.method,
        headers: action.json_body == null ? {} : { 'content-type': 'application/json' },
        body: action.json_body ?? undefined,
        redirect: 'manual',
        signal: AbortSignal.timeout(Math.min(ACTION_TIMEOUT_MS, remaining)),
      });
      if (response.status >= 300 && response.status < 400) fail('API response attempted a redirect');
      const bytes = await limitedBody(response);
      const body = bytes.toString('utf8');
      const bodyMatches = action.expected_body_contains == null || body.includes(action.expected_body_contains);
      const passed = response.status === action.expected_status && bodyMatches;
      actions.push({
        index,
        action: action.action,
        args_redacted: { method: action.method, path: action.path },
        pre_state: { url: request.origin, title: 'request pending' },
        post_state: { url: url.href, title: `status=${response.status}; sha256=${createHash('sha256').update(bytes).digest('hex')}; body_match=${bodyMatches}` },
        evidence_ids: [evidenceId],
        response_status: response.status,
        response_sha256: createHash('sha256').update(bytes).digest('hex'),
        body_matches: bodyMatches,
        status: passed ? 'observed' : 'failed',
      });
    } catch (error) {
      outcome = 'failed';
      errorText = String(error?.message ?? error).slice(0, 4096);
      failedRequests.push(`${action.method} ${action.path}: ${errorText.slice(0, 256)}`);
      actions.push({
        index,
        action: action.action,
        args_redacted: { method: action.method, path: action.path },
        pre_state: { url: request.origin, title: 'request pending' },
        post_state: { url: url.href, title: `request_error=${errorText}` },
        evidence_ids: [evidenceId],
        status: 'failed',
      });
      break;
    }
  }
  process.stdout.write(`${JSON.stringify({
    schema_version: 1,
    qa_run_id: request.qa_run_id,
    scenario_id: request.scenario_id,
    task_uid: request.task_uid,
    code_sha: request.code_sha,
    build_sha256: request.build_sha256,
    approved_spec_hash: request.approved_spec_hash,
    driver_profile_version: request.driver_profile_version,
    outcome,
    origin: request.origin,
    viewport: request.viewport,
    driver_version: `node-${process.version}`,
    actions,
    screenshots: [],
    console_errors: [],
    failed_requests: failedRequests,
    policy_violation: null,
    ...(errorText ? { error: errorText } : {}),
  })}\n`);
  if (outcome === 'failed') process.exitCode = 1;
}

