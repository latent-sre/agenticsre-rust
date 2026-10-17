// Real installed UI acceptance. Run through tools/test-ui-sandbox.sh with a cached browser.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { once } from 'node:events';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const root = process.cwd();
const binary = path.resolve(process.argv[2] ?? 'target/investigation-ui/save');
const output = path.resolve(process.argv[3] ?? 'target/investigation-ui/browser');
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const beforeId = 'run-9-18dd60cacef0724e-1';
const afterId = 'run-16-18dd60cad194352b-1';
const shiftedId = 'run-23-18dd60cad423c07a-1';
const operations = ['context.resolve', 'evidence.list', 'evidence.open', 'evidence.compare', 'report.create'];
const cases = [];
const receipts = [];
const screenshots = [];
const children = [];
const hostile = '<img src="https://example.invalid/x" onerror="window.investigationExecuted=1">';
let browser;
const scratch = await fs.mkdtemp(path.join(os.tmpdir(), 'investigation-browser-'));
await fs.mkdir(output, { recursive: true });

async function snapshot(directory, base = directory) {
  const entries = [];
  for (const name of (await fs.readdir(directory)).sort()) {
    const file = path.join(directory, name);
    const info = await fs.lstat(file);
    if (info.isDirectory()) entries.push(...await snapshot(file, base));
    else if (info.isFile()) entries.push({ path: path.relative(base, file), sha256: sha(await fs.readFile(file)), bytes: info.size, mode: info.mode & 0o777 });
    else throw Error('Unexpected non-regular fixture/source entry: ' + path.relative(base, file));
  }
  return entries;
}
async function sourceSnapshot() {
  const entries = [];
  for (const name of ['crates', 'web/src', 'web/dist', 'docs/sre-workbench/schemas', 'tests/fixtures/context']) {
    entries.push(...await snapshot(path.join(root, name), root));
  }
  for (const name of ['Cargo.toml', 'Cargo.lock', 'web/package.json', 'web/package-lock.json', 'docs/ui/openapi.json', 'docs/investigation-ui.md', 'web/tests/investigations.mjs', 'tools/test-ui-sandbox.sh']) {
    const bytes = await fs.readFile(name);
    entries.push({ path: name, sha256: sha(bytes), bytes: bytes.length });
  }
  return entries;
}
async function privateCopy(source, destination) {
  await fs.mkdir(destination, { mode: 0o700 });
  for (const entry of await fs.readdir(source, { withFileTypes: true })) {
    const from = path.join(source, entry.name), to = path.join(destination, entry.name);
    if (entry.isDirectory()) await privateCopy(from, to);
    else {
      assert(entry.isFile());
      await fs.copyFile(from, to);
      await fs.chmod(to, 0o600);
    }
  }
}
async function startServer(args) {
  const child = spawn(binary, args, { stdio: ['ignore', 'pipe', 'pipe'] });
  children.push(child);
  let stderr = '';
  child.stderr.on('data', value => { stderr += value; });
  const link = await new Promise((resolve, reject) => {
    let text = '';
    const timer = setTimeout(() => reject(Error('Server launch timed out')), 10000);
    child.once('error', error => { clearTimeout(timer); reject(error); });
    child.once('exit', code => { clearTimeout(timer); reject(Error('Server startup exit ' + code + ': ' + stderr)); });
    child.stdout.on('data', value => {
      text += value;
      const match = text.match(/^(http:\/\/127\.0\.0\.1:\d+\/)#token=([A-Za-z0-9_-]{43})\r?\n/mu);
      if (match) { clearTimeout(timer); resolve({ origin: match[1].slice(0, -1), token: match[2] }); }
    });
  });
  const request = async (method, route, body, extra = {}) => {
    const headers = { Authorization: 'Bearer ' + link.token, ...(method === 'GET' ? {} : { Origin: link.origin, 'Content-Type': 'application/json' }), ...extra };
    const response = await fetch(link.origin + route, { method, headers, body: body === undefined ? undefined : JSON.stringify(body), signal: AbortSignal.timeout(15000) });
    return { status: response.status, text: await response.text() };
  };
  return { ...link, request };
}
function payload(operation, inputs, limits = {}) {
  return { submission_id: randomUUID(), operation, operation_version: 1, inputs, limits: { timeout_ms: 30000, max_output_bytes: 1048576, ...limits } };
}
async function terminal(server, id) {
  for (let attempt = 0; attempt < 300; attempt++) {
    const response = await server.request('GET', '/api/v1/runs/' + id);
    assert.equal(response.status, 200);
    const run = JSON.parse(response.text);
    if (run.state === 'terminal') return run;
    await new Promise(resolve => setTimeout(resolve, 50));
  }
  throw Error('Run exceeded bounded acceptance deadline');
}
async function retain(server, id, name) {
  const run = await terminal(server, id);
  const response = await server.request('GET', run.receipt_url);
  assert.equal(response.status, 200);
  const result = JSON.parse(response.text);
  assert.equal(result.record_mode, 'never');
  assert.equal(result.assessment, 'not_assessed');
  const file = name + '.receipt.json';
  await fs.writeFile(path.join(output, file), response.text);
  receipts.push({ name, file, sha256: sha(response.text), operation: result.operation, status: result.execution.status });
  return { result, text: response.text, run };
}
async function runApi(server, operation, inputs, name, limits) {
  const body = payload(operation, inputs, limits);
  const admitted = await server.request('POST', '/api/v1/runs', body);
  assert.equal(admitted.status, 202, admitted.text);
  const record = await retain(server, JSON.parse(admitted.text).id, name);
  const duplicate = await server.request('POST', '/api/v1/runs', body);
  assert.equal(duplicate.status, 202);
  assert.equal(JSON.parse(duplicate.text).id, record.run.id);
  const conflict = await server.request('POST', '/api/v1/runs', { ...body, limits: { ...body.limits, timeout_ms: 29999 } });
  assert.equal(conflict.status, 409);
  return record;
}
function passed(name) { cases.push({ name, status: 'passed' }); console.log('PASS ' + name); }

try {
  const sourcesBefore = await sourceSnapshot();
  const binaryBytes = await fs.readFile(binary);
  const binaryHash = sha(binaryBytes);
  assert(binaryBytes.length < 64 * 1024 * 1024, 'binary within offline launcher bound');
  const source = path.join(scratch, 'context.json'), store = path.join(scratch, 'store');
  const contextFixture = JSON.parse(await fs.readFile('tests/fixtures/context/unique.json', 'utf8'));
  contextFixture.records[0].last_reviewed = new Date().toISOString().replace(/\.\d{3}Z/u, 'Z');
  contextFixture.records[0].owners = [hostile];
  await fs.writeFile(source, JSON.stringify(contextFixture), { mode: 0o600 });
  const sourceBefore = await fs.readFile(source);
  await privateCopy('docs/evidence/evidence-comparison-2026-10-10/installed-r2/store', store);
  const storeBefore = await snapshot(store);
  const server = await startServer(['--context-source', source, '--context-max-age-days', '30', '--state-dir', store, 'ui', 'serve', ...operations.flatMap(op => ['--allow', op])]);
  const sessionReply = await server.request('GET', '/api/v1/session');
  assert.equal(sessionReply.status, 200);
  const session = JSON.parse(sessionReply.text);
  assert.deepEqual(session.operations.map(op => op.id).sort(), [...operations].sort());
  assert.equal(session.roots.length, 0); assert.equal(session.targets.length, 0);
  assert(!sessionReply.text.includes(scratch));
  await fs.writeFile(path.join(output, 'session.json'), sessionReply.text);
  const { chromium } = await import(pathToFileURL(path.join(process.env.WORKBENCH_BROWSER_NODE_MODULES, 'playwright/index.mjs')));
  browser = await chromium.launch({ executablePath: process.env.WORKBENCH_BROWSER_EXECUTABLE, headless: true, args: ['--no-sandbox', '--disable-dev-shm-usage'] });
  const browserContext = await browser.newContext({ viewport: { width: 1440, height: 1100 }, colorScheme: 'light', acceptDownloads: true });
  const page = await browserContext.newPage();
  page.setDefaultTimeout(12000);
  const browserErrors = [], remoteRequests = [];
  page.on('pageerror', error => browserErrors.push(error.message));
  page.on('request', request => { if (!request.url().startsWith(server.origin + '/') && !request.url().startsWith('blob:')) remoteRequests.push(request.url()); });
  await page.goto(server.origin + '/#token=' + server.token);
  await page.locator('#check').waitFor();
  assert(!page.url().includes('token='));
  assert.deepEqual(await page.locator('#check option').evaluateAll(nodes => nodes.map(node => node.value)), ['context', 'evidence-list', 'evidence-open', 'evidence-compare', 'evidence-report']);
  const resultPanel = page.getByLabel('Offline investigation result');
  async function screenshot(name, fullPage = true) {
    const file = name + '.png';
    await page.screenshot({ path: path.join(output, file), fullPage });
    screenshots.push(file);
  }
  async function runBrowser(check, fields, name, keyboard = false) {
    await page.locator('#check').selectOption(check);
    for (const [id, value] of Object.entries(fields)) {
      if (id === 'normalization' || id === 'reportFormat') await page.locator('#' + id).selectOption(value);
      else await page.locator('#' + id).fill(value);
    }
    await page.locator('details.advanced').evaluate(node => { node.open = true; });
    await page.locator('#outputLimit').fill('1048576');
    const replyPromise = page.waitForResponse(response => response.request().method() === 'POST' && new URL(response.url()).pathname === '/api/v1/runs');
    const button = page.getByRole('button', { name: 'Run check', exact: true });
    if (keyboard) { await button.focus(); await button.press('Enter'); } else await button.click();
    const reply = await replyPromise;
    assert.equal(reply.status(), 202, await reply.text());
    const run = await reply.json();
    const record = await retain(server, run.id, name);
    await page.waitForFunction(id => new URL(location.href).searchParams.get('run') === id, run.id);
    await page.getByRole('button', { name: 'Download result', exact: true }).waitFor();
    await resultPanel.waitFor();
    assert((await resultPanel.innerText()).includes('Historical / offline evidence'));
    assert.equal(await page.locator('select#root').count(), 0);
    assert.equal(await page.locator('#target').count(), 0);
    assert.equal(await resultPanel.locator('img,script,iframe').count(), 0);
    assert.equal(await page.evaluate(() => window.investigationExecuted), undefined);
    return record;
  }
  async function download(buttonName, filename) {
    const event = page.waitForEvent('download');
    await page.getByRole('button', { name: buttonName, exact: true }).click();
    const item = await event;
    await item.saveAs(path.join(output, filename));
    return { name: item.suggestedFilename(), bytes: await fs.readFile(path.join(output, filename)) };
  }
  const context = await runBrowser('context', { service: 'checkout', environment: 'dev' }, 'browser-context', true);
  assert.equal(context.result.execution.status, 'succeeded');
  assert.equal(context.result.data.resolution.usable, true);
  assert((await resultPanel.innerText()).includes(hostile));
  assert.equal(await resultPanel.locator('a').count(), 0);
  await screenshot('context-light');
  passed('Context keyboard flow; pinned source, freshness, ownership and hostile text remain inert');
  const list = await runBrowser('evidence-list', {}, 'browser-list');
  assert.equal(list.result.execution.status, 'succeeded');
  assert(list.result.data.runs.some(run => run.id === beforeId));
  passed('Saved evidence list exposes historical storage state and IDs');
  const manifestPin = 'sha256:' + sha(await fs.readFile(path.join(store, 'runs', beforeId, 'manifest.json')));
  const opened = await runBrowser('evidence-open', { bundleId: beforeId, bundlePin: manifestPin }, 'browser-open');
  assert.equal(opened.result.execution.status, 'succeeded');
  assert((await resultPanel.innerText()).includes('2026-10-02'));
  const canonical = await download('Download result', 'browser-open.download.json');
  assert.equal(canonical.bytes.toString(), opened.text);
  await screenshot('evidence-light');
  passed('Verified pinned inspection; canonical browser receipt download matches API bytes exactly');
  const compared = await runBrowser('evidence-compare', { beforeId, afterId }, 'browser-compare');
  assert.equal(compared.result.execution.status, 'succeeded');
  await page.getByRole('button', { name: 'Dark', exact: true }).first().click();
  assert.equal(await page.locator('html').getAttribute('data-theme'), 'dark');
  await screenshot('compare-dark');
  const incompatible = await runBrowser('evidence-compare', { afterId: shiftedId }, 'browser-incompatible');
  assert.equal(incompatible.result.execution.status, 'partial');
  assert((await resultPanel.innerText()).toLowerCase().includes('incompatible'));
  passed('Comparable metric/log deltas and incompatible original windows remain distinct');
  const report = await runBrowser('evidence-report', { reportId1: beforeId, reportPin1: manifestPin, reportId2: afterId, reportTitle: 'Historical handoff <script>window.investigationExecuted=1</script>' }, 'browser-report');
  assert.equal(report.result.execution.status, 'succeeded');
  assert.equal(report.result.data.rendering.state, 'complete');
  const markdown = await download('Download report', 'browser-report.download.md');
  assert.equal(markdown.name, 'workbench-evidence-report.md');
  assert.equal(markdown.bytes.toString(), report.result.data.rendering.content);
  assert.equal(await page.locator('.report-preview').textContent(), report.result.data.rendering.content);
  assert.equal(await page.locator('.report-preview a,.report-preview img').count(), 0);
  await page.setViewportSize({ width: 390, height: 844 });
  assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), 'mobile horizontal overflow');
  await page.locator('.report-preview').scrollIntoViewIfNeeded();
  await screenshot('report-mobile-dark', false);
  passed('Plain-text report preview and explicit Markdown download; dark mobile view has no horizontal overflow');
  await page.setViewportSize({ width: 1440, height: 1100 });
  const jsonReport = await runBrowser('evidence-report', { reportFormat: 'json' }, 'browser-report-json');
  const jsonDownload = await download('Download report', 'browser-report.download.json');
  assert.equal(jsonDownload.name, 'workbench-evidence-report.json');
  assert.equal(jsonDownload.bytes.toString(), jsonReport.result.data.rendering.content);
  assert.deepEqual(JSON.parse(jsonDownload.bytes), jsonReport.result.data.report);
  passed('Explicit JSON report download matches the retained report');
  await page.getByRole('link', { name: /Run history/u }).first().click();
  await page.getByLabel('Session history').getByRole('button', { name: /Inspect evidence/u }).click();
  await page.getByRole('button', { name: 'Download result', exact: true }).waitFor();
  const reopened = await download('Download result', 'history-open.download.json');
  assert.equal(reopened.bytes.toString(), opened.text);
  passed('Session history reopens the original invocation without recomputation');

  assert.equal((await server.request('GET', '/api/v1/session', undefined, { Authorization: '' })).status, 401);
  assert.equal((await server.request('POST', '/api/v1/runs', payload('evidence.list', {}), { Origin: 'https://example.invalid' })).status, 403);
  assert.equal((await server.request('POST', '/api/v1/runs', payload('evidence.delete', { id: beforeId }))).status, 403);
  assert.equal((await server.request('POST', '/api/v1/runs', payload('context.resolve', { service: 'checkout', environment: 'dev', path: source }))).status, 422);
  assert.equal((await server.request('POST', '/api/v1/runs', { ...payload('evidence.list', {}), record: 'always' })).status, 422);
  passed('Real API authentication/origin, independent grants and request authority override refusals');
  const badPin = await runApi(server, 'evidence.open', { id: beforeId, expected_manifest_sha256: 'sha256:' + '0'.repeat(64) }, 'api-bad-pin');
  assert.notEqual(badPin.result.execution.status, 'succeeded');
  const missing = await runApi(server, 'evidence.open', { id: 'run-1-0000000000000000-1' }, 'api-missing-bundle');
  assert.notEqual(missing.result.execution.status, 'succeeded');
  const bounded = await runApi(server, 'evidence.open', { id: beforeId }, 'api-bounded-open', { max_items: 1 });
  assert.equal(bounded.result.execution.status, 'partial');
  passed('Manifest pin mismatch, missing evidence and bounded partial inspection retain honest receipts; dedup/conflicts preserved');
  assert.deepEqual(await fs.readFile(source), sourceBefore);
  assert.deepEqual(await snapshot(store), storeBefore);
  await fs.writeFile(path.join(output, 'store-before.json'), JSON.stringify(storeBefore, null, 2) + '\n');
  await fs.writeFile(path.join(output, 'store-after.json'), JSON.stringify(await snapshot(store), null, 2) + '\n');
  passed('All browser/API observations leave source and retained store bytes/modes unchanged');

  await fs.writeFile(source, JSON.stringify({ ...contextFixture, source: { ...contextFixture.source, revision: 'replaced' } }));
  const sourceSwap = await runApi(server, 'context.resolve', { service: 'checkout', environment: 'dev' }, 'api-source-changed');
  assert.notEqual(sourceSwap.result.execution.status, 'succeeded');
  await fs.rename(store, store + '-original');
  await privateCopy(store + '-original', store);
  const storeSwap = await runApi(server, 'evidence.list', {}, 'api-store-replaced');
  assert.notEqual(storeSwap.result.execution.status, 'succeeded');
  assert.deepEqual(await snapshot(store + '-original'), storeBefore);
  assert.deepEqual(await snapshot(store), storeBefore);
  passed('Changed context bytes and same-content replacement store are refused by existing grants');

  const corruptStore = path.join(scratch, 'corrupt-store');
  await privateCopy(store, corruptStore);
  await fs.writeFile(path.join(corruptStore, 'runs', beforeId, 'manifest.json'), '{');
  const corruptBefore = await snapshot(corruptStore);
  const corruptServer = await startServer(['--state-dir', corruptStore, 'ui', 'serve', '--allow', 'evidence.open']);
  const corrupt = await runApi(corruptServer, 'evidence.open', { id: beforeId }, 'api-corrupt-bundle');
  assert.notEqual(corrupt.result.execution.status, 'succeeded');
  assert.deepEqual(await snapshot(corruptStore), corruptBefore);
  passed('Corrupt historical manifest is rejected without repair or store mutation');

  const staleSource = path.join(scratch, 'stale-context.json');
  const staleFixture = structuredClone(contextFixture);
  staleFixture.records[0].last_reviewed = '2020-01-01T00:00:00Z';
  await fs.writeFile(staleSource, JSON.stringify(staleFixture), { mode: 0o600 });
  const staleServer = await startServer(['--context-source', staleSource, '--context-max-age-days', '30', 'ui', 'serve', '--allow', 'context.resolve']);
  const stale = await runApi(staleServer, 'context.resolve', { service: 'checkout', environment: 'dev' }, 'api-stale-context');
  assert.equal(stale.result.data.resolution.usable, false);
  assert.notEqual(stale.result.execution.status, 'succeeded');
  passed('Stale context retains its provenance and cannot claim usable bindings');

  const empty = await startServer(['ui', 'serve']);
  const emptyPage = await browserContext.newPage();
  await emptyPage.goto(empty.origin + '/#token=' + empty.token);
  await emptyPage.getByRole('heading', { name: 'No checks are granted' }).waitFor();
  assert.equal(await emptyPage.locator('#check').count(), 0);
  assert.equal((await empty.request('POST', '/api/v1/runs', payload('evidence.list', {}))).status, 403);
  await emptyPage.close();
  passed('Ungranting all operations removes runnable browser controls and denies direct API requests');
  assert.deepEqual(browserErrors, []);
  assert.deepEqual(remoteRequests, []);
  assert.equal(sha(await fs.readFile(binary)), binaryHash);
  assert.deepEqual(await sourceSnapshot(), sourcesBefore);
  passed('No browser script errors/remote requests; source and installed binary identities stable throughout acceptance');
  await fs.writeFile(path.join(output, 'source-manifest.json'), JSON.stringify(sourcesBefore, null, 2) + '\n');
  await fs.writeFile(path.join(output, 'summary.json'), JSON.stringify({ status: 'passed', binary: { sha256: binaryHash, bytes: binaryBytes.length }, source_manifest_sha256: sha(JSON.stringify(sourcesBefore, null, 2) + '\n'), browser_version: browser.version(), cases, receipts, screenshots }, null, 2) + '\n');
} finally {
  if (browser) await browser.close();
  for (const child of children) {
    if (child.exitCode !== null || child.signalCode !== null) continue;
    child.kill('SIGTERM');
    const timeout = setTimeout(() => child.kill('SIGKILL'), 5000);
    await once(child, 'exit');
    clearTimeout(timeout);
  }
  await fs.rm(scratch, { recursive: true, force: true });
}
