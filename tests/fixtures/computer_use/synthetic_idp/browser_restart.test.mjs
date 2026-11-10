import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { chromium } from 'playwright';
import { startSyntheticIdp, SYNTHETIC_IDENTITY } from './server.mjs';

test('synthetic password and MFA stay inside browser and profile resumes after process restart', async () => {
  const fixture = await startSyntheticIdp();
  const profile = await mkdtemp(join(tmpdir(), 'koolade-personal-auth-'));
  let context;
  try {
    context = await chromium.launchPersistentContext(profile, {
      headless: true,
      args: ['--no-sandbox', '--disable-dev-shm-usage', '--disable-background-networking'],
    });
    let page = await context.newPage();
    page.on('request', (request) => {
      assert.equal(new URL(request.url()).hostname, '127.0.0.1', 'fixture browser must stay local');
    });
    await page.goto(`${fixture.origin}/login`);
    await page.getByLabel('Email').fill(SYNTHETIC_IDENTITY.username);
    await page.getByLabel('Password').fill(SYNTHETIC_IDENTITY.password);
    await page.getByRole('button', { name: 'Continue' }).click();
    await page.getByRole('heading', { name: 'Verify synthetic sign in' }).waitFor();

    await page.getByLabel('One-time code').fill('111111');
    await page.getByRole('button', { name: 'Verify' }).click();
    await page.getByText('Code rejected').waitFor();
    await page.goto(`${fixture.origin}/account`);
    assert.equal(await page.getByRole('status').textContent(), 'Sign in required');

    await page.goto(`${fixture.origin}/login`);
    await page.getByLabel('Email').fill(SYNTHETIC_IDENTITY.username);
    await page.getByLabel('Password').fill(SYNTHETIC_IDENTITY.password);
    await page.getByRole('button', { name: 'Continue' }).click();
    await page.getByLabel('One-time code').fill(SYNTHETIC_IDENTITY.mfaCode);
    await page.getByRole('button', { name: 'Verify' }).click();
    await page.getByRole('heading', { name: 'Fixture account' }).waitFor();

    await context.close();
    context = await chromium.launchPersistentContext(profile, {
      headless: true,
      args: ['--no-sandbox', '--disable-dev-shm-usage', '--disable-background-networking'],
    });
    page = await context.newPage();
    page.on('request', (request) => {
      assert.equal(new URL(request.url()).hostname, '127.0.0.1', 'fixture browser must stay local');
    });
    await page.goto(`${fixture.origin}/account`);
    await page.getByRole('heading', { name: 'Fixture account' }).waitFor();
    assert.equal(await page.getByRole('status').textContent(), 'Authenticated by synthetic IdP');
  } finally {
    await context?.close();
    await fixture.close();
    await rm(profile, { recursive: true, force: true });
  }
});
