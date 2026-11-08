import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { availableChecks, initialValues, makeSubmission, validateForm } from './model';
import type { Session } from './model';
import { CheckForm } from './CheckForm';
import savedOpen from '../../docs/evidence/offline-2026-10-11/commands/open.stdout.json';
import partialOpen from '../../docs/evidence/offline-2026-10-11/commands/open-limited.stdout.json';
import omittedReport from '../../docs/evidence/reports-2026-10-11/commands/limited.stdout.json';
import contextExample from '../../tests/fixtures/context/unique.json';
import { InvestigationResult } from './InvestigationResult';

afterEach(cleanup);
const session: Session = { api_version: '0.1', session_id: 'offline', roots: [], targets: [],
 operations: [ { id: 'context.resolve', version: 1, profile: 'offline-context-v1' },
 ...(['evidence.list', 'evidence.open', 'evidence.replay', 'evidence.compare', 'report.create'] as const).map(id => ({ id, version: 1 as const, profile: 'verified-evidence-v1' as const })) ],
 limits: { active_runs: 1, history_entries: 50, history_bytes: 16777216, max_request_bytes: 65536, max_submissions: 1024 } };
const before = 'run-1-ab-c'; const after = 'run-2-ab-c'; const pin = `sha256:${'a'.repeat(64)}`;
describe('offline investigation admission', () => {
 it('intersects each independent operation grant without needing roots or targets', () => {
  expect(availableChecks(session)).toEqual(['context', 'evidence-list', 'evidence-open', 'evidence-replay', 'evidence-compare', 'evidence-report']);
  expect(availableChecks({ ...session, operations: [session.operations[2]!] })).toEqual(['evidence-open']);
 });
 it('replay has an independent form and sends only its reference and bounds', async () => {
  const replayOnly: Session = { ...session, operations: [{ id: 'evidence.replay', version: 1, profile: 'verified-evidence-v1' }] };
  expect(availableChecks(replayOnly)).toEqual(['evidence-replay']);
  const user = userEvent.setup(); const submit = vi.fn();
  render(<CheckForm session={replayOnly} check="evidence-replay" busy={false} submitting={false} onSubmit={submit} onSelection={vi.fn()} />);
  expect(screen.queryByLabelText('Workspace')).toBeNull();
  await user.click(screen.getByRole('button', { name: 'Run check' }));
  expect(document.activeElement).toBe(screen.getByLabelText('Evidence ID'));
  await user.type(screen.getByLabelText('Evidence ID'), before);
  await user.clear(screen.getByLabelText('Maximum detail items'));
  await user.type(screen.getByLabelText('Maximum detail items'), '1');
  await user.click(screen.getByRole('button', { name: 'Run check' }));
  expect(submit.mock.calls[0]?.[0]).toMatchObject({ operation: 'evidence.replay', operation_version: 1, inputs: { id: before }, limits: { timeout_ms: 30000, max_output_bytes: 16384, max_items: 1 } });
 });
 it('sends exact direct inputs and only operation-specific limits', () => {
  expect(makeSubmission({ ...initialValues, check: 'context', service: 'orders', environment: 'test' }, 'context.resolve', 'id')).toEqual({ submission_id: 'id', operation_version: 1, operation: 'context.resolve', inputs: { service: 'orders', environment: 'test' }, limits: { timeout_ms: 30000, max_output_bytes: 16384 } });
  expect(makeSubmission({ ...initialValues, check: 'evidence-list' }, 'evidence.list', 'id').inputs).toEqual({});
  expect(makeSubmission({ ...initialValues, check: 'evidence-open', bundleId: before, bundlePin: pin }, 'evidence.open', 'id')).toMatchObject({ inputs: { id: before, expected_manifest_sha256: pin }, limits: { max_items: 100 } });
  expect(makeSubmission({ ...initialValues, check: 'evidence-compare', beforeId: before, afterId: after }, 'evidence.compare', 'id').inputs).toEqual({ before: { id: before }, after: { id: after }, normalization: 'exact-window-v1' });
  const report = makeSubmission({ ...initialValues, check: 'evidence-report', reportId1: before, reportPin1: pin }, 'report.create', 'id');
  expect(report.inputs).toEqual({ bundles: [{ id: before, expected_manifest_sha256: pin }], format: 'markdown' });
  expect(report.limits).not.toHaveProperty('max_items'); expect(report).not.toHaveProperty('root_id'); expect(report).not.toHaveProperty('target_id');
 });
 it.each(['../evidence', '/tmp/bundle', 'null', '', 'run-1-AB-c'])('rejects invalid reference %s without guessing', bundleId => {
  expect(validateForm({ ...initialValues, check: 'evidence-open', bundleId }).bundleId).toBeTruthy();
 });
 it('rejects malformed pins, invalid bounds, and report selection gaps', () => {
  const errors = validateForm({ ...initialValues, check: 'evidence-open', bundleId: before, bundlePin: 'null', maxItems: '0' }); expect(errors.bundlePin).toBeTruthy(); expect(errors.maxItems).toBeTruthy();
  expect(validateForm({ ...initialValues, check: 'evidence-report', reportPin2: pin }).reportId2).toBeTruthy();
  expect(validateForm({ ...initialValues, check: 'context', service: 'null', environment: '' }).environment).toBeTruthy();
 });
 it('bounds detail counts, output and report selections without sending nulls', () => {
  for (const maxItems of ['0', '1001', 'null', '1.5', 'Infinity']) expect(validateForm({ ...initialValues, check: 'evidence-open', bundleId: before, maxItems }).maxItems).toBeTruthy();
  for (const outputLimit of ['1023', '1048577', 'null']) expect(validateForm({ ...initialValues, check: 'evidence-list', outputLimit }).outputLimit).toBeTruthy();
  expect(validateForm({ ...initialValues, check: 'evidence-report', reportId1: before, reportId2: before }).reportId2).toBeTruthy();
  const submission = makeSubmission({ ...initialValues, check: 'evidence-report', reportId1: before, reportId2: after, reportId3: 'run-3-ab-c', reportId4: 'run-4-ab-c', reportFormat: 'json', reportTitle: 'Incident notes' }, 'report.create', 'four');
  expect(submission.inputs).toEqual({ bundles: [{ id: before }, { id: after }, { id: 'run-3-ab-c' }, { id: 'run-4-ab-c' }], format: 'json', title: 'Incident notes' });
  expect(JSON.stringify(submission)).not.toContain('null');
 });
 it('focuses invalid evidence ID and submits a labeled inspection', async () => {
  const submit = vi.fn(); const user = userEvent.setup();
  render(<CheckForm session={session} check="evidence-open" busy={false} submitting={false} onSubmit={submit} onSelection={vi.fn()} />);
  expect(screen.queryByLabelText('Workspace')).toBeNull();
  await user.click(screen.getByRole('button', { name: 'Run check' }));
  expect(document.activeElement).toBe(screen.getByLabelText('Evidence ID'));
  await user.type(screen.getByLabelText('Evidence ID'), before);
  await user.click(screen.getByRole('button', { name: 'Run check' }));
  expect(submit.mock.calls[0]?.[0]).toMatchObject({ operation: 'evidence.open', inputs: { id: before } });
 });
});
describe('historical evidence presentation', () => {
 it.each(['evidence.open', 'evidence.replay', 'evidence.compare'])('renders prototype-named keys in %s nested evidence labels as inert text', operation => {
  const evidenceLabels: unknown = JSON.parse('{"__proto__":"proto label value","constructor":"constructor label value","toString":"toString label value","job":"orders"}');
  const { container } = render(<InvestigationResult operation={operation} data={{
    state: 'complete', bundle: { id: before, digest: pin }, observation: { window: { from: '2026-10-01T00:00:00Z', to: '2026-10-01T01:00:00Z' } },
    compatibility: { state: 'compatible', reasons: [] }, before: { bundle: { id: before } }, after: { bundle: { id: after } },
    checks: [{ id: 'metrics', state: 'complete', summary: { samples_added: 1 }, details: [{ labels: evidenceLabels, value_delta: 1 }] }],
  }} />);
  expect(screen.getByText('proto label value')).toBeTruthy();
  expect(screen.getByText('constructor label value')).toBeTruthy();
  expect(screen.getByText('toString label value')).toBeTruthy();
  expect(screen.getByText('orders')).toBeTruthy();
  const fieldLabels = [...container.querySelectorAll('dt')].map(item => item.textContent);
  expect(fieldLabels).toContain('  proto  '); expect(fieldLabels).toContain('Constructor'); expect(fieldLabels).toContain('ToString');
  expect(screen.getByText('Samples added')).toBeTruthy(); expect(screen.getByText('Value delta')).toBeTruthy();
 });

 it.each([savedOpen, partialOpen])('renders retained core open receipt windows, digests and check bounds', receipt => {
  render(<InvestigationResult operation="evidence.open" data={receipt.data} />);
  expect(screen.getByText(receipt.data.bundle.digest)).toBeTruthy();
  expect(screen.getByText('Original observation window and capture')).toBeTruthy();
  expect(screen.getByRole('heading', { name: 'Metrics' })).toBeTruthy(); expect(screen.getByRole('heading', { name: 'Logs' })).toBeTruthy();
  expect(screen.getAllByText('Omitted detail items').length).toBeGreaterThan(0);
 });
 it('renders an omitted report honestly with no content download', () => {
  render(<InvestigationResult operation="report.create" data={omittedReport.data} />);
  expect(screen.getByText(/The report rendering was omitted/)).toBeTruthy();
  expect(screen.queryByRole('button', { name: 'Download report' })).toBeNull();
 });
 it('shows stale context ownership, missing bindings and source text without links', () => {
  const { container } = render(<InvestigationResult operation="context.resolve" data={{ record: contextExample.records[0], resolution: { state: 'stale', usable: false, age_seconds: 90000, max_age_days: 1, missing_required: ['target'] }, source: { evidence_state: 'sourced', revision: 'fixture', observed_at: '2026-10-01T00:00:00Z' } }} />);
  expect(screen.getByText(/Context is unusable for binding/)).toBeTruthy(); expect(screen.getByText('Age (seconds)')).toBeTruthy(); expect(screen.getByText('Missing required bindings')).toBeTruthy(); expect(container.querySelector('a')).toBeNull();
 });
 it('lists saved core run IDs, protection, timestamps and static errors', () => {
  render(<InvestigationResult operation="evidence.list" data={{ runs: [{ id: before, state: 'interrupted', protected: true, created_at_unix_ms: 1000, finalized_at_unix_ms: null, bytes: 100, error: 'manifest_invalid' }] }} />);
  expect(screen.getByText(before)).toBeTruthy(); expect(screen.getByText('Protected')).toBeTruthy(); expect(screen.getByText('manifest_invalid')).toBeTruthy(); expect(screen.getByText('1970-01-01T00:00:01.000Z')).toBeTruthy();
 });
 it('shows incompatibility, original windows, provenance changes and metric/log deltas as text', () => {
  const { container } = render(<InvestigationResult operation="evidence.compare" data={{ state: 'partial', normalization: 'window-offset-v1', before: { window: { from: 'original-window' } }, after: {}, compatibility: { state: 'incompatible', reasons: ['window mismatch'] }, provenance_changes: ['query revision changed'], checks: [{ id: 'metrics', state: 'partial', summary: { samples_added: 2 }, details: [{ value: '<img src=x onerror=alert(1)>' }] }, { id: 'logs', state: 'missing', reasons: ['No retained receipt'] }] }} />);
  expect(screen.getByText('window mismatch')).toBeTruthy(); expect(screen.getByText('original-window')).toBeTruthy(); expect(screen.getByText('Samples added')).toBeTruthy(); expect(screen.getByText('No retained receipt')).toBeTruthy();
  expect(container.querySelector('img')).toBeNull(); expect(screen.getByText('<img src=x onerror=alert(1)>')).toBeTruthy();
 });
 it.each(['markdown', 'json'] as const)('renders %s reports as inert plain text and downloads content with a fixed filename', async format => {
  const content = format === 'markdown' ? '# Report\n<script>bad()</script>' : '{"untrusted":"<script>bad()</script>"}'; const blobs: Blob[] = [];
  Object.defineProperty(URL, 'createObjectURL', { configurable: true, value: vi.fn((blob: Blob) => { blobs.push(blob); return 'blob:report'; }) });
  const revoke = vi.fn(); Object.defineProperty(URL, 'revokeObjectURL', { configurable: true, value: revoke });
  const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function(this: HTMLAnchorElement) { expect(this.download).toBe(format === 'markdown' ? 'workbench-evidence-report.md' : 'workbench-evidence-report.json'); });
  const { container } = render(<InvestigationResult operation="report.create" data={{ format, report: { state: 'partial', title: 'Historical report', knowledge_proposals: [{ title: 'Untrusted suggestion', review_status: 'pending' }] }, rendering: { state: 'complete', content, media_type: 'text/markdown' } }} />);
  expect(container.querySelector('script')).toBeNull(); expect(container.querySelector('pre')?.textContent).toBe(content);
  expect(screen.getByText(/Pending proposals are unapproved/)).toBeTruthy();
  await userEvent.setup().click(screen.getByRole('button', { name: 'Download report' }));
  expect(blobs[0]?.type).toBe(format === 'markdown' ? 'text/markdown' : 'application/json');
  const downloaded = await new Promise<string>((resolve, reject) => { const reader = new FileReader(); reader.onload = () => typeof reader.result === 'string' ? resolve(reader.result) : reject(new Error('Expected report text')); reader.onerror = reject; reader.readAsText(blobs[0]!); });
  expect(downloaded).toBe(content); expect(click).toHaveBeenCalledOnce();
  await new Promise(resolve => setTimeout(resolve, 1100)); expect(revoke).toHaveBeenCalledWith('blob:report'); click.mockRestore();
 });
});
