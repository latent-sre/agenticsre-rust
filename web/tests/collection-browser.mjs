// Private installed-binary browser mission; Python owns the real HTTPS fixture.
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { createInterface } from 'node:readline';
import { readFile } from 'node:fs/promises';
import path from 'node:path';

const require = createRequire(path.join(process.env.WORKBENCH_BROWSER_NODE_MODULES, 'package.json'));
const { chromium } = require('playwright');
const lines = createInterface({ input: process.stdin })[Symbol.asyncIterator]();
const read = async () => JSON.parse((await lines.next()).value);
const rpc = async request => { process.stdout.write(JSON.stringify(request) + '\n'); return read(); };
const config = await read();
const browser = await chromium.launch({ executablePath: process.env.WORKBENCH_BROWSER_EXECUTABLE, headless: true, args: ['--no-sandbox', '--disable-dev-shm-usage'] });
const context = await browser.newContext({ viewport: { width: 1440, height: 1100 }, colorScheme: 'light' });
const page = await context.newPage();
const errors = []; const external = []; const submissions = []; const contracts = [];
page.on('response', response => {
  const pathname = new URL(response.url()).pathname;
  if (response.status() >= 400) return;
  const schema = pathname === '/api/v1/session' ? 'Session' : pathname === '/api/v1/runs' && response.request().method() === 'POST' || /^\/api\/v1\/runs\/[^/]+$/u.test(pathname) ? 'RunSummary' : null;
  if (schema) contracts.push(response.json().then(value => ({ schema, value })));
});
page.on('pageerror', error => errors.push(error.name));
page.on('request', request => {
  if (!request.url().startsWith(new URL(config.launch).origin)) external.push(request.url());
  if (request.method() === 'POST' && request.url().endsWith('/api/v1/runs')) submissions.push(request.postDataJSON());
});
const screenshot = async name => { await page.evaluate(() => window.scrollTo(0, 0)); await page.screenshot({ path: path.join(config.output, name + '.png'), fullPage: true }); };
const finished = async (name, action) => {
  const responsePromise = page.waitForResponse(response => response.request().method() === 'GET' && response.url().endsWith('/receipt'));
  await action(); const response = await responsePromise; assert.equal(response.status(), 200);
  const raw = await response.text(); const receipt = JSON.parse(raw);
  await page.getByLabel('Check result').locator('header.result-heading').getByText(receipt.execution.status.replaceAll('_', ' '), { exact: true }).waitFor();
  await rpc({ action: 'record', name, receipt });
  return { raw, receipt };
};
const download = async expected => {
  const event = page.waitForEvent('download'); await page.getByRole('button', { name: 'Download result', exact: true }).click();
  assert.equal(await readFile(await (await event).path(), 'utf8'), expected);
};
try {
  await page.goto(config.launch, { waitUntil: 'networkidle' });
  assert.equal(new URL(page.url()).hash, '');
  await page.getByRole('heading', { name: 'Workspace checks', exact: true }).waitFor();
  const checks = page.getByLabel('Check', { exact: true });
  assert.deepEqual((await checks.locator('option').evaluateAll(items => items.map(item => item.value))).sort(),
    ['evidence-collect', 'evidence-open', 'evidence-packs-describe', 'evidence-packs-list']);
  await checks.selectOption('evidence-packs-list');
  const catalog = await finished('catalog', () => page.getByRole('button', { name: 'Inspect catalog', exact: true }).click());
  assert.equal(catalog.receipt.data.packs.length, 1); assert.deepEqual(await rpc({ action: 'ledger' }), []);
  await page.getByLabel('Evidence pack catalog result').getByText(catalog.receipt.data.packs[0].sha256, { exact: true }).waitFor();
  await screenshot('catalog-light');
  await checks.selectOption('evidence-packs-describe');
  const description = await finished('describe', () => page.getByRole('button', { name: 'Inspect catalog', exact: true }).click());
  assert.deepEqual(await rpc({ action: 'ledger' }), []);
  await checks.selectOption('evidence-collect');
  assert.equal(await page.getByLabel('Collection target').inputValue(), 'fixture');
  assert.equal(await page.getByLabel('Workspace', { exact: true }).count(), 0);
  const before = submissions.length;
  await page.getByRole('button', { name: 'Collect and save', exact: true }).click();
  assert.equal(await page.getByLabel('Service', { exact: true }).evaluate(element => document.activeElement === element), true);
  assert.equal(submissions.length, before);
  const job = '<img src="https://untrusted.invalid" onerror="alert(1)">';
  const fillCollection = async () => {
    for (const [label, value] of [['Service', 'fixture-web'], ['Environment', 'dev'], ['Metrics datasource UID', 'metrics-1'], ['Metrics job', job], ['Logs datasource UID', 'logs-1'], ['Logs job', job]]) {
      await page.getByLabel(label, { exact: true }).fill(value);
    }
  };
  await fillCollection();
  await page.getByLabel('Pack SHA-256 (optional)', { exact: true }).fill(description.receipt.data.pack.sha256);
  const collected = await finished('collected', () => page.getByRole('button', { name: 'Collect and save', exact: true }).click());
  assert.equal(collected.receipt.execution.status, 'succeeded');
  assert.equal(collected.receipt.data.capture.state, 'complete');
  const ledger = await rpc({ action: 'ledger' }); assert.equal(ledger.length, 6);
  assert.equal(ledger[2].body.queries[0].expr, 'up{job=' + JSON.stringify(job) + '}');
  assert.equal(ledger[5].body.queries[0].expr, '{job=' + JSON.stringify(job) + '}');
  assert.equal(ledger[2].body.from, ledger[5].body.from); assert.equal(ledger[2].body.to, ledger[5].body.to);
  await page.getByLabel('Evidence collection result').getByText(collected.receipt.data.bundle.id, { exact: true }).waitFor();
  await download(collected.raw); await screenshot('collection-light');
  await page.getByRole('link', { name: 'Run history' }).filter({ visible: true }).click();
  await page.getByLabel('Session history').getByRole('button').filter({ hasText: 'Collect evidence' }).click();
  await page.getByRole('heading', { name: 'Saved bundle' }).waitFor();
  await download(collected.raw);
  await checks.selectOption('evidence-open');
  await page.locator('#bundleId').fill(collected.receipt.data.bundle.id);
  await page.locator('#bundlePin').fill(collected.receipt.data.bundle.digest);
  const opened = await finished('opened', () => page.getByRole('button', { name: 'Run check', exact: true }).click());
  assert.equal(opened.receipt.data.observation.manifest_state, 'sealed');
  assert.equal((await rpc({ action: 'ledger' })).length, 6);
  assert.equal(await page.getByLabel('Offline investigation result').locator('img, a, script, iframe').count(), 0);
  await checks.selectOption('evidence-collect'); await fillCollection();
  await rpc({ action: 'fixture', fail_kind: 'prometheus' });
  await page.getByLabel('On a failed check').selectOption('stop');
  const partial = await finished('partial', () => page.getByRole('button', { name: 'Collect and save', exact: true }).click());
  assert.notEqual(partial.receipt.execution.status, 'succeeded');
  assert.equal(partial.receipt.data.steps[1].status, 'omitted'); assert.ok(partial.receipt.data.bundle);
  assert.equal((await rpc({ action: 'ledger' })).length, 9);
  await page.getByRole('button', { name: 'Dark', exact: true }).filter({ visible: true }).click();
  await screenshot('partial-dark');
  await page.setViewportSize({ width: 390, height: 844 }); await screenshot('partial-mobile-dark');
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
  await page.setViewportSize({ width: 1440, height: 1100 });
  await rpc({ action: 'fixture', block: true });
  await page.getByRole('button', { name: 'Collect and save', exact: true }).click();
  await page.getByRole('button', { name: 'Cancel check', exact: true }).waitFor();
  assert.equal(await page.getByRole('button', { name: 'Collect and save', exact: true }).isDisabled(), true);
  const limit = Date.now() + 5000;
  while ((await rpc({ action: 'ledger' })).length !== 12) { assert.ok(Date.now() < limit); await page.waitForTimeout(20); }
  const cancelled = await finished('cancelled', () => page.getByRole('button', { name: 'Cancel check', exact: true }).click());
  assert.equal(cancelled.receipt.execution.status, 'cancelled'); assert.equal((await rpc({ action: 'ledger' })).length, 12);
  assert.equal(cancelled.receipt.data.steps[1].status, 'omitted');
  assert.deepEqual(errors, []); assert.deepEqual(external, []);
  assert.deepEqual(await page.evaluate(() => ({ local: { ...localStorage }, session: { ...sessionStorage } })), { local: { 'agenticsre.theme': 'dark' }, session: {} });
  assert.deepEqual(await context.cookies(), []);
  for (const submission of submissions) await rpc({ action: 'contract', schema: 'RunSubmission', value: submission });
  for (const result of await Promise.all(contracts)) await rpc({ action: 'contract', ...result });
  await rpc({ action: 'done' });
} finally { await browser.close(); await lines.return(); }
