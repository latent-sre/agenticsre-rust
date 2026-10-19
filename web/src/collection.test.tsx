import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { availableChecks, initialValues, makeSubmission, validateForm } from './model';
import type { Session } from './model';
import { CheckForm } from './CheckForm';
import { CollectionResult } from './CollectionResult';

afterEach(cleanup);
const session: Session = { api_version: '0.1', session_id: 'collection', roots: [], targets: [],
  query_targets: [{ id: 'query-only', label: 'Query only' }],
  collection_targets: [{ id: 'fixture', label: 'Collection fixture' }],
  operations: [
    { id: 'evidence.packs.list', version: 1, profile: 'installed-evidence-catalog-v1' },
    { id: 'evidence.packs.describe', version: 1, profile: 'installed-evidence-catalog-v1' },
    { id: 'evidence.collect', version: 1, profile: 'evidence-collection-v1' },
  ], limits: { active_runs: 1, history_entries: 50, history_bytes: 16777216, max_request_bytes: 65536, max_submissions: 1024 } };
const values = { ...initialValues, check: 'evidence-collect' as const, target: 'fixture', service: 'checkout', environment: 'dev',
  metricsDatasource: 'metrics-1', metricsJob: 'café"literal', logsDatasource: 'logs-1', logsJob: 'web', packPin: 'a'.repeat(64) };

describe('browser evidence collection', () => {
  it('renders pack digests offline and partial saved evidence as inert text', () => {
    const { rerender, container } = render(<CollectionResult operation="evidence.packs.describe" data={{ pack: { id: 'service-queries', sha256: 'a'.repeat(64) } }} />);
    expect(screen.getByText('a'.repeat(64))).toBeTruthy();
    rerender(<CollectionResult operation="evidence.collect" data={{ bundle: { id: 'run-1-ab-c', digest: `sha256:${'b'.repeat(64)}` }, capture: { state: 'partial' }, steps: [{ id: 'logs', status: 'cancelled', message: '<img src="https://untrusted.invalid" onerror="alert(1)">' }], window: { from: 'start', to: 'end' } }} />);
    expect(screen.getByText('run-1-ab-c')).toBeTruthy();
    expect(screen.getByText('partial')).toBeTruthy();
    expect(screen.getAllByText('cancelled', { exact: true })[0]).toBeTruthy();
    expect(container.querySelector('img, script, iframe, a')).toBeNull();
    rerender(<CollectionResult operation="evidence.collect" data={{ bundle: null }} />);
    expect(screen.getByText(/No sealed bundle reference returned/)).toBeTruthy();
    expect(screen.queryByText('run-1-ab-c')).toBeNull();
  });
  it('requires independent collection authority instead of reusing query targets', () => {
    expect(availableChecks(session)).toEqual(['evidence-packs-list', 'evidence-packs-describe', 'evidence-collect']);
    expect(availableChecks({ ...session, collection_targets: [] })).toEqual(['evidence-packs-list', 'evidence-packs-describe']);
    expect(availableChecks({ ...session, operations: [session.operations[2]!] })).toEqual(['evidence-collect']);
  });
  it('submits literal fixed-pack inputs and keeps catalog requests target-free', () => {
    expect(validateForm(values)).toEqual({});
    expect(makeSubmission(values, 'evidence.collect', 'run')).toMatchObject({
      operation: 'evidence.collect', target_id: 'fixture', inputs: {
        service: 'checkout', environment: 'dev', pack: { id: 'service-queries', version: 1 },
        metrics: { datasource: 'metrics-1', parameters: { job: 'café"literal' } },
        logs: { datasource: 'logs-1', parameters: { job: 'web' } },
        expected_pack_sha256: 'a'.repeat(64), window: { kind: 'relative', lookback_seconds: 300 }, on_failure: 'continue',
      },
    });
    const list = makeSubmission({ ...values, check: 'evidence-packs-list' }, 'evidence.packs.list', 'list');
    expect(list.inputs).toEqual({}); expect(list).not.toHaveProperty('target_id');
    const collect = makeSubmission(values, 'evidence.collect', 'run');
    expect(collect).not.toHaveProperty('record'); expect(collect.inputs).not.toHaveProperty('state_dir');
    expect(collect.limits).not.toHaveProperty('concurrency');
  });
  it('validates each source selector, job, pin and whole-pack deadline', () => {
    const errors = validateForm({ ...values, service: '', metricsDatasource: 'https://example.invalid', logsJob: '$__interval', packPin: 'wrong', timeout: '61' });
    for (const field of ['service', 'metricsDatasource', 'logsJob', 'packPin', 'timeout'] as const) expect(errors[field]).toBeTruthy();
    expect(validateForm({ ...values, windowKind: 'absolute', windowFrom: '2026-10-02T00:00:00Z', windowTo: '2026-10-01T00:00:00Z' }).windowTo).toBeTruthy();
  });
  it('shows the store effect and submits via the granted collection target', async () => {
    const submit = vi.fn(); const user = userEvent.setup();
    render(<CheckForm session={session} check="evidence-collect" busy={false} submitting={false} onSubmit={submit} onSelection={vi.fn()} />);
    expect(screen.queryByLabelText('Workspace')).toBeNull();
    expect(screen.queryByText('Query only')).toBeNull();
    expect(screen.getByLabelText('Collection target')).toHaveProperty('value', 'fixture');
    expect(screen.getByText(/saves.*private evidence store/i)).toBeTruthy();
    for (const [label, value] of [['Service', 'checkout'], ['Environment', 'dev'], ['Metrics datasource UID', 'metrics-1'], ['Metrics job', 'web'], ['Logs datasource UID', 'logs-1'], ['Logs job', 'web']]) {
      await user.type(screen.getByLabelText(label!), value!);
    }
    await user.click(screen.getByRole('button', { name: 'Collect and save' }));
    expect(submit.mock.calls[0]?.[0]).toMatchObject({ operation: 'evidence.collect', target_id: 'fixture', inputs: { service: 'checkout' } });
  });
});
