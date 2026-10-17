import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { availableChecks, initialValues, makeSubmission, runScope, validateForm } from './model';
import type { Session } from './model';
import { CheckForm } from './CheckForm';
import { QueryResult } from './QueryResult';

afterEach(cleanup);
const session: Session = { api_version: '0.1', session_id: 'queries', roots: [], targets: [{ id: 'diagnostic', label: 'Diagnostic only' }],
 query_targets: [{ id: 'telemetry', label: 'Fixture telemetry' }],
 operations: [{ id: 'queries.list', version: 1, profile: 'installed-query-catalog-v1' }, { id: 'queries.describe', version: 1, profile: 'installed-query-catalog-v1' }, { id: 'query.run', version: 1, profile: 'saved-query-v1' }],
 limits: { active_runs: 1, history_entries: 50, history_bytes: 16777216, max_request_bytes: 65536, max_submissions: 1024 } };
const values = { ...initialValues, check: 'query-run' as const, target: 'telemetry', datasource: 'metrics-1', job: 'café\\worker"},other="value"} or vector(1) #' };
describe('saved query forms', () => {
 it('requires independent grants and query targets only for execution', () => {
  expect(availableChecks(session)).toEqual(['queries-list', 'queries-describe', 'query-run']);
  expect(availableChecks({ ...session, query_targets: [] })).toEqual(['queries-list', 'queries-describe']);
  expect(availableChecks({ ...session, operations: [session.operations[2]!] })).toEqual(['query-run']);
 });
 it('keeps catalog calls local and literal query inputs exact', () => {
  expect(makeSubmission({ ...values, check: 'queries-list' }, 'queries.list', 'list').inputs).toEqual({});
  const description = makeSubmission({ ...values, check: 'queries-describe' }, 'queries.describe', 'describe');
  expect(description.inputs).toEqual({ id: 'prometheus.up-by-job', version: 1 });
  expect(description).not.toHaveProperty('target_id');
  const request = makeSubmission(values, 'query.run', 'run');
  expect(request).toMatchObject({ target_id: 'telemetry', inputs: { id: 'prometheus.up-by-job', version: 1, datasource: 'metrics-1', parameters: { job: values.job }, window: { kind: 'relative', lookback_seconds: 300 } } });
  expect(request).not.toHaveProperty('root_id'); expect(request.limits).not.toHaveProperty('concurrency');
  expect(validateForm(values)).toEqual({});
 });
 it('retains explicit absolute window and a bare definition digest', () => {
  const input = { ...values, windowKind: 'absolute' as const, windowFrom: '2026-10-02T00:00:00.123Z', windowTo: '2026-10-02T01:00:00.123Z', definitionPin: 'a'.repeat(64) };
  expect(validateForm(input)).toEqual({});
  expect(makeSubmission(input, 'query.run', 'absolute').inputs).toMatchObject({ expected_definition_sha256: 'a'.repeat(64), window: { kind: 'absolute', from: input.windowFrom, to: input.windowTo } });
 });
 it('rejects invalid datasource, job bytes, digest and time bounds', () => {
  const errors = validateForm({ ...values, datasource: 'https://host', job: 'é'.repeat(65), definitionPin: `sha256:${'a'.repeat(64)}`, lookback: '86401' });
  for (const field of ['datasource', 'job', 'definitionPin', 'lookback'] as const) expect(errors[field]).toBeTruthy();
  for (const job of ['', 'job\u0085', '$__interval']) expect(validateForm({ ...values, job }).job).toBeTruthy();
  for (const windowTo of ['2026-10-02T00:00:00.1234Z', '2026-10-04T00:00:00Z', '2026-10-01T00:00:00Z', '2026-02-30T00:00:00Z']) expect(validateForm({ ...values, windowKind: 'absolute', windowFrom: '2026-10-02T00:00:00Z', windowTo }).windowTo).toBeTruthy();
 });
 it('submits through explicit query target and focuses required input', async () => {
  const submit = vi.fn(); const user = userEvent.setup();
  render(<CheckForm session={session} check="query-run" busy={false} submitting={false} onSubmit={submit} onSelection={vi.fn()} />);
  expect(screen.queryByLabelText('Workspace')).toBeNull(); expect(screen.queryByText('Diagnostic only')).toBeNull();
  expect(screen.getByLabelText('Query target')).toHaveProperty('value', 'telemetry');
  await user.click(screen.getByRole('button', { name: 'Run query' }));
  expect(document.activeElement).toBe(screen.getByLabelText('Datasource UID'));
  await user.type(screen.getByLabelText('Datasource UID'), 'metrics-1'); await user.type(screen.getByLabelText('Literal job'), values.job);
  await user.click(screen.getByRole('button', { name: 'Run query' }));
  expect(submit.mock.calls[0]?.[0]).toMatchObject({ operation: 'query.run', target_id: 'telemetry', inputs: { parameters: { job: values.job } } });
 });
 it('names query scope honestly in history', () => {
  expect(runScope({ id: 'run', operation: 'query.run', check_label: 'Saved query', state: 'running', submitted_at: '2026-10-02T00:00:00Z', root_id: null, target_id: 'telemetry', execution_status: null, receipt_url: null }, session)).toBe('Fixture telemetry');
 });
});
describe('saved query observations', () => {
 it('shows installed definition metadata and blind spots', () => {
  render(<QueryResult operation="queries.list" data={{ queries: [{ id: 'prometheus.up-by-job', version: 1, dialect: 'promql', definition_sha256: 'a'.repeat(64), purpose: 'Observe scrape samples.', units: 'dimensionless', expected_shape: 'time_series_frames', limitations: ['Empty results are not zero.'], parameter_schema: { required: ['job'] }, limits: { max_data_points: 1000 } }] }} />);
  expect(screen.getByText('a'.repeat(64))).toBeTruthy(); expect(screen.getByText('Empty results are not zero.')).toBeTruthy(); expect(screen.getByText('dimensionless')).toBeTruthy();
 });
 it('renders untrusted observations without HTML, links, or implicit health conclusions', () => {
  const { container } = render(<QueryResult operation="query.run" data={{ definition: { id: 'loki.logs-by-job', version: 1, sha256: 'a'.repeat(64) }, window: { from: '2026-10-02T00:00:00Z', to: '2026-10-02T00:01:00Z' }, response: { results: { A: { frames: [], line: '<img src=x onerror=alert(1)>\u001b[31m', link: 'https://untrusted.invalid' } } }, transport: { stage: 'complete' } }} />);
  expect(screen.getByText(/<img src=x onerror=alert\(1\)>/)).toBeTruthy(); expect(container.querySelector('img, a, script')).toBeNull();
  expect(screen.getByText(/Empty results are not zero/)).toBeTruthy(); expect(screen.getByRole('heading', { name: 'Resolved observation window' })).toBeTruthy();
 });
});
