import { createHash, randomUUID } from 'node:crypto';
import { readFileSync } from 'node:fs';
import process from 'node:process';
import { chromium } from 'playwright';

const MAX_REQUEST_BYTES = 256 * 1024;
const MAX_STEPS = 512;
const MAX_SCREENSHOTS = 64;
const MAX_SCREENSHOT_BYTES = 8 * 1024 * 1024;
const MAX_TOTAL_EVIDENCE_BYTES = 64 * 1024 * 1024;
const ACTION_TIMEOUT_MS = 15_000;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const SHA256 = /^[0-9a-f]{64}$/i;
const GIT_OID = /^(?:[0-9a-f]{40}|[0-9a-f]{64})$/i;

function fail(message) {
  throw new Error(message);
}

function browserVersion(browser) {
  try {
    return typeof browser?.version === 'function' ? browser.version() : 'unknown';
  } catch {
    return 'unknown';
  }
}

function readRequest() {
  const input = readFileSync('/run/koolade-runtime-qa/request.json');
  if (input.length === 0 || input.length > MAX_REQUEST_BYTES) fail('request size is outside bounds');
  const request = JSON.parse(input.toString('utf8'));
  if (request?.schema_version !== 1 || !UUID.test(request.qa_run_id) || !UUID.test(request.scenario_id) ||
      !UUID.test(request.task_uid) || !GIT_OID.test(request.code_sha) || !SHA256.test(request.build_sha256) ||
      !SHA256.test(request.approved_spec_hash) || typeof request.driver_profile_version !== 'string' ||
      request.driver_profile_version.length > 128 || typeof request.origin !== 'string' ||
      !Array.isArray(request.actions) || request.actions.length === 0 || request.actions.length > MAX_STEPS ||
      !Number.isInteger(request.viewport?.width) || !Number.isInteger(request.viewport?.height) ||
      request.viewport.width < 320 || request.viewport.width > 3840 ||
      request.viewport.height < 240 || request.viewport.height > 2160 ||
      !Number.isInteger(request.max_screenshots) || request.max_screenshots < 1 || request.max_screenshots > MAX_SCREENSHOTS ||
      !Number.isInteger(request.max_evidence_bytes) || request.max_evidence_bytes < 1024 || request.max_evidence_bytes > MAX_TOTAL_EVIDENCE_BYTES ||
      !Number.isInteger(request.timeout_ms) || request.timeout_ms < 1000 || request.timeout_ms > 600_000) {
    fail('browser QA request is malformed or exceeds bounds');
  }
  const origin = new URL(request.origin);
  if (origin.protocol !== 'http:' || origin.username || origin.password || origin.pathname !== '/' ||
      origin.search || origin.hash || origin.hostname !== 'qa-preview' || origin.port !== '8080') {
    fail('preview origin must be the one app-owned relay origin');
  }
  return { ...request, origin: origin.origin };
}

function allowedUrl(raw, origin) {
  let parsed;
  try { parsed = new URL(raw, `${origin}/`); } catch { return false; }
  return parsed.origin === origin && parsed.protocol === 'http:' && !parsed.username && !parsed.password;
}

function targetUrl(raw, origin) {
  let parsed;
  try { parsed = new URL(raw); } catch { return null; }
  if (parsed.protocol === 'http:' && parsed.hostname === 'qa-service' && !parsed.port &&
      !parsed.username && !parsed.password) return new URL(`${parsed.pathname}${parsed.search}${parsed.hash}`, `${origin}/`).href;
  return allowedUrl(raw, origin) ? parsed.href : null;
}

function validateLocator(locator) {
  if (!locator || typeof locator !== 'object') return false;
  if (locator.locator === 'role') return typeof locator.role === 'string' && locator.role.length <= 64 && typeof locator.name === 'string' && locator.name.length <= 512;
  if (locator.locator === 'test_id') return typeof locator.test_id === 'string' && locator.test_id.length <= 512;
  if (locator.locator === 'label') return typeof locator.label === 'string' && locator.label.length <= 512;
  return false;
}

function validateAction(action, origin) {
  if (!action || typeof action !== 'object' || typeof action.action !== 'string') fail('invalid driver action');
  const text = (...keys) => keys.every((key) => typeof action[key] === 'string' && action[key].length <= 4096 && !action[key].includes('\0'));
  switch (action.action) {
    case 'browser_open': case 'browser_assert_url': return text('url') && targetUrl(action.url, origin) !== null;
    case 'browser_click': case 'browser_wait_for_visible': case 'browser_scroll': return validateLocator(action.locator);
    case 'browser_fill': case 'browser_select': return validateLocator(action.locator) && text('value');
    case 'browser_press': return validateLocator(action.locator) && text('key');
    case 'browser_assert_text': return validateLocator(action.locator) && text('expected_text');
    case 'browser_set_viewport': return Number.isInteger(action.width) && Number.isInteger(action.height) && action.width >= 320 && action.width <= 3840 && action.height >= 240 && action.height <= 2160;
    case 'browser_screenshot': return true;
    default: return false;
  }
}

function locatorFor(page, locator) {
  if (locator.locator === 'role') {
    return page.getByRole(locator.role, { name: locator.name, exact: true });
  }
  if (locator.locator === 'test_id') {
    return page.getByTestId(locator.test_id);
  }
  if (locator.locator === 'label') return page.getByLabel(locator.label, { exact: true });
  fail('unsupported locator type');
}

async function main() {
  const request = readRequest();
  if (request.actions.some((action) => !validateAction(action, request.origin))) fail('action is not allowed by browser QA policy');
  let browser = null;
  let context = null;
  let page = null;
  const consoleMessages = [];
  const failedRequests = [];
  const actionTrace = [];
  const screenshots = [];
  let screenshotBytes = 0;
  let policyViolation = null;
  let activeAction = null;
  let activeBefore = null;
  try {
    browser = await chromium.launch({ headless: true, args: ['--no-sandbox', '--disable-dev-shm-usage', '--no-first-run', '--no-default-browser-check', '--disable-background-networking', '--disable-extensions', '--renderer-process-limit=2'] });
    context = await browser.newContext({ viewport: request.viewport, locale: request.locale ?? 'en-US', timezoneId: request.timezone ?? 'UTC', colorScheme: request.color_scheme === 'dark' ? 'dark' : 'light', acceptDownloads: false, serviceWorkers: 'block' });
    page = await context.newPage();
    page.on('console', (message) => { if (message.type() === 'error' && consoleMessages.length < 128) consoleMessages.push(message.text().slice(0, 1024)); });
    page.on('pageerror', (error) => { if (consoleMessages.length < 128) consoleMessages.push(error.message.slice(0, 1024)); });
    page.on('requestfailed', (requestEvent) => { if (failedRequests.length < 128) failedRequests.push(`${requestEvent.method()} ${requestEvent.url().slice(0, 1024)}: ${(requestEvent.failure()?.errorText ?? 'failed').slice(0, 256)}`); });
    page.on('response', (response) => {
      const location = response.headers().location;
      if (!location) return;
      let destination;
      try { destination = new URL(location, response.url()).href; } catch { return; }
      if (!allowedUrl(destination, request.origin)) {
        policyViolation = `redirect escaped preview origin: ${destination}`;
      }
    });
    page.on('websocket', (socket) => { if (!allowedUrl(socket.url(), request.origin)) policyViolation = `websocket escaped preview origin: ${socket.url()}`; });
    await context.routeWebSocket('**/*', (socket) => {
      if (!allowedUrl(socket.url(), request.origin)) {
        policyViolation = `websocket escaped preview origin: ${socket.url()}`;
        socket.close({ code: 1008, reason: 'QA origin policy denied websocket' });
      } else {
        socket.connectToServer();
      }
    });
    await context.route('**/*', async (route) => {
      const url = route.request().url();
      if (!allowedUrl(url, request.origin)) {
        policyViolation = `request escaped preview origin: ${url}`;
        await route.abort('blockedbyclient');
      } else {
        await route.continue();
      }
    });
    page.setDefaultTimeout(Math.min(ACTION_TIMEOUT_MS, request.timeout_ms));
    const deadline = Date.now() + request.timeout_ms;
    for (let index = 0; index < request.actions.length; index += 1) {
      if (Date.now() >= deadline) fail('browser QA run timed out');
      page.setDefaultTimeout(Math.min(ACTION_TIMEOUT_MS, deadline - Date.now()));
      const action = request.actions[index];
      const before = { url: page.url(), title: await page.title().catch(() => '') };
      activeAction = { action, index };
      activeBefore = before;
      const target = action.locator ? locatorFor(page, action.locator) : null;
      switch (action.action) {
        case 'browser_open': await page.goto(targetUrl(action.url, request.origin), { waitUntil: 'domcontentloaded', timeout: Math.min(ACTION_TIMEOUT_MS, deadline - Date.now()) }); break;
        case 'browser_click': await target.click(); break;
        case 'browser_fill': await target.fill(action.value); break;
        case 'browser_press': await target.press(action.key); break;
        case 'browser_select': await target.selectOption(action.value); break;
        case 'browser_wait_for_visible': await target.waitFor({ state: 'visible' }); break;
        case 'browser_assert_text': {
          await target.getByText(action.expected_text, { exact: false }).waitFor({ state: 'visible', timeout: ACTION_TIMEOUT_MS });
          const actual = await target.innerText();
          if (!actual.includes(action.expected_text)) fail(`assertion failed: expected text was not present for action ${index}`);
          break;
        }
        case 'browser_assert_url': if (!allowedUrl(page.url(), request.origin) || new URL(page.url()).href !== targetUrl(action.url, request.origin)) fail(`URL assertion failed for action ${index}`); break;
        case 'browser_scroll': await target.scrollIntoViewIfNeeded(); break;
        case 'browser_set_viewport': {
          const width = action.width; const height = action.height;
          if (!Number.isInteger(width) || !Number.isInteger(height) || width < 320 || width > 3840 || height < 240 || height > 2160) fail('viewport is outside bounds');
          await page.setViewportSize({ width, height }); break;
        }
        case 'browser_screenshot': break;
        default: fail('unsupported action reached dispatcher');
      }
      const screenshotIds = [];
      if (action.action === 'browser_screenshot' || action.screenshot_checkpoint === true || action.action.startsWith('browser_assert_')) {
        if (screenshots.length >= request.max_screenshots) fail('screenshot count exceeds quota');
        const bytes = await page.screenshot({ type: 'png', fullPage: false, animations: 'disabled', timeout: Math.min(ACTION_TIMEOUT_MS, deadline - Date.now()) });
        screenshotBytes += bytes.length;
        if (bytes.length > MAX_SCREENSHOT_BYTES || screenshotBytes > request.max_evidence_bytes || screenshotBytes > MAX_TOTAL_EVIDENCE_BYTES) fail('screenshots exceed evidence quota');
        const id = randomUUID();
        screenshots.push({ id, sha256: createHash('sha256').update(bytes).digest('hex'), width: page.viewportSize().width, height: page.viewportSize().height, mime: 'image/png', bytes_base64: bytes.toString('base64') });
        screenshotIds.push(id);
      }
      actionTrace.push({ index, action: action.action, args_redacted: action.action === 'browser_fill' ? { locator: action.locator, value: '[redacted]' } : { locator: action.locator }, pre_state: before, post_state: { url: page.url(), title: await page.title().catch(() => '') }, screenshot_ids: screenshotIds, status: 'observed' });
      activeAction = null;
      activeBefore = null;
      if (policyViolation) fail(policyViolation);
    }
    const result = { schema_version: 1, qa_run_id: request.qa_run_id, scenario_id: request.scenario_id, task_uid: request.task_uid, code_sha: request.code_sha, build_sha256: request.build_sha256, approved_spec_hash: request.approved_spec_hash, driver_profile_version: request.driver_profile_version, outcome: 'completed', origin: request.origin, viewport: page.viewportSize(), driver_version: `playwright-${browserVersion(browser)}`, actions: actionTrace, screenshots, console_errors: consoleMessages, failed_requests: failedRequests, policy_violation: policyViolation };
    process.stdout.write(`${JSON.stringify(result)}\n`);
  } catch (error) {
    let failureScreenshotId = null;
    if (page && screenshots.length < request.max_screenshots && page.isClosed() === false) {
      try {
        const bytes = await page.screenshot({ type: 'png', fullPage: false, animations: 'disabled', timeout: 2_000 });
        screenshotBytes += bytes.length;
        if (bytes.length <= MAX_SCREENSHOT_BYTES && screenshotBytes <= request.max_evidence_bytes && screenshotBytes <= MAX_TOTAL_EVIDENCE_BYTES) {
          failureScreenshotId = randomUUID();
          screenshots.push({ id: failureScreenshotId, sha256: createHash('sha256').update(bytes).digest('hex'), width: page.viewportSize().width, height: page.viewportSize().height, mime: 'image/png', bytes_base64: bytes.toString('base64'), captured_on_failure: true });
        }
      } catch { /* The failure report still records the screenshot capture failure. */ }
    }
    if (activeAction) {
      const { action, index } = activeAction;
      actionTrace.push({ index, action: action.action, args_redacted: action.action === 'browser_fill' ? { locator: action.locator, value: '[redacted]' } : { locator: action.locator }, pre_state: activeBefore, post_state: { url: page?.url() ?? request.origin, title: await page?.title().catch(() => '') ?? '' }, screenshot_ids: failureScreenshotId ? [failureScreenshotId] : [], status: 'failed' });
    }
    const result = { schema_version: 1, qa_run_id: request.qa_run_id, scenario_id: request.scenario_id, task_uid: request.task_uid, code_sha: request.code_sha, build_sha256: request.build_sha256, approved_spec_hash: request.approved_spec_hash, driver_profile_version: request.driver_profile_version, outcome: 'failed', origin: request.origin, viewport: page?.viewportSize() ?? request.viewport, driver_version: `playwright-${browserVersion(browser)}`, actions: actionTrace, screenshots, console_errors: consoleMessages, failed_requests: failedRequests, policy_violation: policyViolation, error: String(error?.message ?? error).slice(0, 4096) };
    process.stdout.write(`${JSON.stringify(result)}\n`);
    process.exitCode = 1;
  } finally {
    if (context) await context.close().catch(() => {});
    if (browser) await browser.close().catch(() => {});
  }
}

main().catch((error) => {
  process.stderr.write(`${String(error?.message ?? error).slice(0, 4096)}\n`);
  process.exitCode = 2;
});
