import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import type { ReactNode } from 'react';
import { createApi } from './api';
import type { Receipt, Run, Session } from './model';
import { Result } from './Result';
import diagnosticExample from '../../docs/sre-workbench/examples/diagnostics-data.json';

// Navigation is outside this receipt-boundary test; the component and real client still run.
vi.mock('@tanstack/react-router', () => ({ Link: ({ children }: { children: ReactNode }) => <a>{children}</a> }));
afterEach(cleanup);

const receipt: Receipt = {
  spec_version: '0.1', request_id: 'request-fixture', run_id: 'run-fixture',
  operation: 'command.inspect', operation_version: 1,
  target: { kind: 'local', id: 'workstation' }, resolved_target: {},
  started_at: '2026-10-02T20:00:00Z', finished_at: '2026-10-02T20:00:00Z', duration_ms: 1,
  execution: { status: 'succeeded', child_exit_code: null, signal: null },
  effect_outcome: 'not_applicable', assessment: 'not_assessed',
  coverage: { state: 'not_applicable', scope: 'Inspection fixture', limitations: [] },
  output: { stdout: 'café 雪\n', stderr: '', stdout_truncated: false, stderr_truncated: false, encoding: 'utf-8' },
  data: {}, errors: [], sources: [], artifacts: [],
  effective_limits: { timeout_ms: 30000, max_output_bytes: 1048576 }, record_mode: 'never',
};
// Keep the large integer as source text: constructing it as a JS number would destroy the witness.
const originalText = '\n' + JSON.stringify(receipt).replace('"data":{}',
  '"data":{"root":{"inode":9007199254740993},"maximum_identity":18446744073709551615}') + '\n';
const run: Run = { id: 'inv-fixture', operation: 'command.inspect', check_label: 'Working tree status',
  root_id: 'checkout', target_id: null, state: 'terminal', submitted_at: receipt.started_at,
  execution_status: 'succeeded', receipt_url: '/api/v1/runs/inv-fixture/receipt' };
const session: Session = { api_version: '0.1', session_id: 'session-fixture', roots: [{ id: 'checkout', label: 'Fixture' }], targets: [],
  operations: [{ id: 'command.inspect', version: 1, profile: 'linux-read-v1' }],
  limits: { active_runs: 1, history_entries: 50, history_bytes: 16777216, max_request_bytes: 65536, max_submissions: 1024 } };

function mountResult(tab: 'output' | 'details', response = new Response(originalText, { headers: { 'content-type': 'application/json' } }), selectedRun = run) {
  const fetcher = vi.fn<typeof fetch>().mockResolvedValue(response);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(<QueryClientProvider client={client}><Result api={createApi('synthetic-token', fetcher)}
    run={selectedRun} session={{ ...session, targets: [{ id: 'fixture-web', label: 'Fixture service' }] }} tab={tab} onCancel={vi.fn()} cancelling={false} /></QueryClientProvider>);
  return { ...view, client, fetcher };
}

function blobText(blob: Blob) {
  return new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => typeof reader.result === 'string' ? resolve(reader.result) : reject(new Error('Expected text Blob'));
    reader.onerror = () => reject(new Error('Could not read downloaded Blob'));
    reader.readAsText(blob);
  });
}

describe('original receipt representation', () => {
  it('downloads the original large integer identities and exact response text', async () => {
    const blobs: Blob[] = [];
    const createObjectURL = vi.fn((blob: Blob) => { blobs.push(blob); return 'blob:receipt-fixture'; });
    Object.defineProperty(URL, 'createObjectURL', { configurable: true, value: createObjectURL });
    Object.defineProperty(URL, 'revokeObjectURL', { configurable: true, value: vi.fn() });
    vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => undefined);
    const { fetcher } = mountResult('output');
    await userEvent.setup().click(await screen.findByRole('button', { name: 'Download result' }));
    const blob = blobs[0];
    if (!blob) throw new Error('No download Blob was created');
    const text = await blobText(blob);
    expect(text).toContain('9007199254740993');
    expect(text).toContain('18446744073709551615');
    expect(text).toBe(originalText);
    expect(new TextEncoder().encode(text)).toEqual(new TextEncoder().encode(originalText));
    expect(fetcher).toHaveBeenCalledTimes(1);
  });

  it('renders complete JSON from the original text without integer rounding or reformatting', async () => {
    const { container } = mountResult('details');
    await screen.findByText('Complete result JSON');
    const text = container.querySelector('.raw-details pre')?.textContent;
    expect(text).toContain('9007199254740993');
    expect(text).toContain('18446744073709551615');
    expect(text).toBe(originalText);
  });

  it('discards the selected receipt cache when its view is removed', async () => {
    const { client, unmount } = mountResult('output');
    await screen.findByRole('button', { name: 'Download result' });
    expect(client.getQueryCache().findAll({ queryKey: ['receipt'] })).toHaveLength(1);
    unmount();
    await waitFor(() => expect(client.getQueryCache().findAll({ queryKey: ['receipt'] })).toHaveLength(0));
  });

  it('escapes direction controls in the view while preserving original download bytes and integers', async () => {
    const controlledText = originalText.replace('café 雪', 'café \u202e\u2066 雪');
    const blobs: Blob[] = [];
    Object.defineProperty(URL, 'createObjectURL', { configurable: true, value: (blob: Blob) => { blobs.push(blob); return 'blob:controls'; } });
    Object.defineProperty(URL, 'revokeObjectURL', { configurable: true, value: vi.fn() });
    vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => undefined);
    const { container } = mountResult('details', new Response(controlledText, { headers: { 'content-type': 'application/json' } }));
    await screen.findByText('Complete result JSON');
    const text = container.querySelector('.raw-details pre')?.textContent;
    expect(text).toContain('\\u202e\\u2066');
    expect(text).not.toContain('\u202e');
    expect(text).not.toContain('\u2066');
    expect(text).toContain('9007199254740993');
    await userEvent.setup().click(screen.getByRole('button', { name: 'Download result' }));
    expect(await blobText(blobs[0]!)).toBe(controlledText);
  });

  it('keeps typed problem responses on the error path, with no downloadable receipt', async () => {
    const problem = { type: 'urn:agenticsre:problem:evicted', title: 'Result evicted', status: 410,
      detail: 'This result is no longer retained.', request_id: 'http-fixture' };
    mountResult('output', new Response(JSON.stringify(problem), { status: 410, headers: { 'content-type': 'application/problem+json' } }));
    await screen.findByRole('heading', { name: 'This result is no longer retained' });
    expect(screen.queryByRole('button', { name: 'Download result' })).toBeNull();
  });

  it('does not retain malformed successful response text as a receipt', async () => {
    const { client } = mountResult('details', new Response('{ malformed JSON', { headers: { 'content-type': 'application/json' } }));
    await screen.findByRole('heading', { name: 'Result unavailable' });
    expect(client.getQueryData(['receipt', run.id])).toBeUndefined();
    expect(screen.queryByText('Complete result JSON')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Download result' })).toBeNull();
  });
});

const diagnosticRun: Run = { ...run, operation: 'diagnostics.run', check_label: 'Connectivity diagnostics', root_id: null, target_id: 'fixture-web' };
function diagnosticReceipt(data = diagnosticExample, status: Receipt['execution']['status'] = 'succeeded') {
  return { ...receipt, operation: 'diagnostics.run', target: { kind: 'connection', id: 'fixture-web' },
    data, output: { ...receipt.output, stdout: '' }, execution: { ...receipt.execution, status },
    coverage: { state: status === 'succeeded' ? 'complete' : 'partial', scope: 'Five actual diagnostic checks', limitations: ['Service health is not assessed.'] },
    effective_limits: { timeout_ms: 30_000, max_output_bytes: 16_384, concurrency: 2 } };
}
function mountDiagnostic(tab: 'output' | 'details', data = diagnosticExample, status: Receipt['execution']['status'] = 'succeeded') {
  return mountResult(tab, new Response(JSON.stringify(diagnosticReceipt(data, status)), { headers: { 'content-type': 'application/json' } }), diagnosticRun);
}

describe('diagnostic observations', () => {
  it('shows each core outcome and keeps the original service name apart from aliases', async () => {
    const data = structuredClone(diagnosticExample);
    data.steps[0]!.observation.cname_chains!.a.push('alias.workbench.test');
    mountDiagnostic('output', data);
    await screen.findByRole('heading', { name: 'DNS lookup' });
    for (const name of ['TCP connection', 'TLS validation', 'HTTP response', 'Local process']) {
      expect(screen.getByRole('heading', { name })).toBeTruthy();
    }
    expect(screen.getAllByText('Configured service / TLS name').length).toBeGreaterThan(0);
    expect(screen.getByText('diagnostics.workbench.test → alias.workbench.test')).toBeTruthy();
    expect(screen.getByText('DNS transport requests')).toBeTruthy();
    expect(screen.getByText('Requests handed to the transport. A closed stream can reject one before transmission.')).toBeTruthy();
    expect(screen.getByText('Fixture service')).toBeTruthy();
    expect(screen.queryByText('Numerical input')).toBeNull();
  });
  it('retains partial, cancelled and timed-out omissions with reasons and absent timing', async () => {
    const data = structuredClone(diagnosticExample);
    Object.assign(data.steps[0]!, { state: 'failed', reason: 'dns_address_denied' });
    Object.assign(data.steps[0]!.observation, { addresses: ['127.0.0.2'], selected_peer: null });
    for (const step of data.steps.slice(1, 4)) {
      Object.assign(step, { state: 'blocked', attempted: false, reason: 'prerequisite_failed',
        duration_ms: null, started_offset_ms: null, observed_at: null, observation: {} });
    }
    const { container } = mountDiagnostic('output', data, 'partial');
    await screen.findByText('dns_address_denied');
    expect(screen.getAllByText(/^Not run · Elapsed:/u)).toHaveLength(3);
    expect(screen.getAllByText('prerequisite_failed')).toHaveLength(3);
    expect(container.querySelectorAll('.diagnostic-step')).toHaveLength(5);
    const blocked = container.querySelectorAll('.diagnostic-step')[1];
    expect(blocked?.textContent).toContain('Elapsed: Not recorded');
    expect(blocked?.textContent).not.toContain('Elapsed: 0 ms');
    expect(screen.getByRole('heading', { name: 'Local process' })).toBeTruthy();
  });
  it('describes denied DNS addresses as observations and preserves the absent selected peer', async () => {
    const data = structuredClone(diagnosticExample);
    // Mirrors the retained dns_address_denied receipt: resolver/name facts remain, no peer is selected.
    Object.assign(data.steps[0]!, { state: 'failed', reason: 'dns_address_denied' });
    Object.assign(data.steps[0]!.observation, { addresses: ['127.0.0.2'], selected_peer: null });
    for (const step of data.steps.slice(1, 4)) {
      Object.assign(step, { state: 'blocked', attempted: false, reason: 'prerequisite_failed',
        duration_ms: null, started_offset_ms: null, observed_at: null, observation: {} });
    }
    mountDiagnostic('output', data, 'partial');
    await screen.findByText('dns_address_denied');
    const dnsView = screen.getByLabelText('DNS lookup');
    expect(within(dnsView).queryByText('No observation returned.')).toBeNull();
    expect(within(dnsView).getByText('Observation details below.')).toBeTruthy();
    expect(within(dnsView).queryByText('Accepted service addresses')).toBeNull();
    expect(within(dnsView).getByText('Observed service addresses').nextElementSibling?.textContent).toBe('127.0.0.2');
    expect(within(dnsView).getByText('Selected service peer').nextElementSibling?.textContent).toBe('Not returned');
    expect(dnsView.textContent).toContain('failed');
  });
  it('describes a refused TCP peer as an attempt and retains the failed connection facts', async () => {
    const data = structuredClone(diagnosticExample);
    // tcp_connect_failed retains exactly peer/hostname/port, without a successful local socket.
    Object.assign(data.steps[1]!, { state: 'failed', reason: 'tcp_connect_failed',
      observation: { peer: '127.0.0.1:8443', hostname: 'diagnostics.workbench.test', port: 8443 } });
    for (const step of data.steps.slice(2, 4)) {
      Object.assign(step, { state: 'blocked', attempted: false, reason: 'prerequisite_failed',
        duration_ms: null, started_offset_ms: null, observed_at: null, observation: {} });
    }
    mountDiagnostic('output', data, 'partial');
    await screen.findByText('tcp_connect_failed');
    const tcpView = screen.getByLabelText('TCP connection');
    expect(within(tcpView).queryByText('Connected peer: 127.0.0.1:8443')).toBeNull();
    expect(within(tcpView).getByText('Attempted service peer: 127.0.0.1:8443')).toBeTruthy();
    expect(within(tcpView).getByText('Service peer').nextElementSibling?.textContent).toBe('127.0.0.1:8443');
    expect(within(tcpView).getByText('Configured service / TLS name').nextElementSibling?.textContent).toBe('diagnostics.workbench.test');
    expect(within(tcpView).queryByText('Local socket')).toBeNull();
    expect(tcpView.textContent).toContain('failed');
  });
  it.each(['cancelled', 'timed_out'] as const)('keeps %s as the terminal outcome with completed observations', async status => {
    const data = structuredClone(diagnosticExample);
    Object.assign(data.steps[0]!, { state: status, reason: status, observation: {} });
    const { container } = mountDiagnostic('output', data, status);
    await screen.findByRole('heading', { name: 'Local process' });
    expect(container.querySelector('.result-heading .badge')?.textContent).toContain(status.replace('_', ' '));
    expect(container.querySelectorAll('.diagnostic-step')).toHaveLength(5);
    expect(container.textContent).not.toMatch(/healthy|unhealthy/iu);
  });
  it('renders body bytes and hostile headers inertly with capture and processing flags', async () => {
    const data = structuredClone(diagnosticExample);
    const hostile = '<script>window.__body_executed=1</script><img src="https://example.invalid">\u001b[31m\u202e';
    const encoded = btoa(String.fromCharCode(...new TextEncoder().encode(hostile)));
    const capture = data.steps[3]!.observation.body_capture!;
    Object.assign(capture, { requested: true, content: encoded, captured_bytes: new TextEncoder().encode(hostile).length, truncated: true });
    data.steps[3]!.observation.headers!['content-type'] = hostile;
    const { container } = mountDiagnostic('output', data, 'partial');
    await screen.findByText('Captured body (untrusted base64)');
    expect(screen.getByText(encoded)).toBeTruthy();
    expect(container.textContent).toContain('\\u001b[31m\\u202e');
    expect(container.textContent).toContain('Redaction applied');
    expect(container.textContent).toContain('Decompressed');
    expect(container.querySelectorAll('.diagnostic-step a, .diagnostic-step img, .diagnostic-step script, .diagnostic-step iframe')).toHaveLength(0);
    expect(window).not.toHaveProperty('__body_executed');
  });
  it('shows source, config, CA, pack and actual vantage identities in details', async () => {
    mountDiagnostic('details');
    await screen.findByRole('heading', { name: 'Diagnostic provenance' });
    expect(screen.getByText('Configuration SHA-256')).toBeTruthy();
    expect(screen.getByText('Additional CA SHA-256')).toBeTruthy();
    expect(screen.getByText('Pack SHA-256')).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Observer vantage' })).toBeTruthy();
    expect(screen.getByText(diagnosticExample.pack.sha256)).toBeTruthy();
  });
  it('renders full PID and UID identities without locale grouping or rounding', async () => {
    const data = structuredClone(diagnosticExample);
    const process = data.steps[4]!.observation;
    process.pid = 4194304;
    process.uid = 4294967295;
    process.uids = { real: 4294967295, effective: 4294967295, saved: 4294967295, filesystem: 4294967295 };
    process.start_time_ticks = '18446744073709551615';
    mountDiagnostic('output', data);
    await screen.findByRole('heading', { name: 'Local process' });
    const processView = screen.getByLabelText('Local process');
    expect(within(processView).getAllByText('PID', { exact: true }).map(label => label.nextElementSibling?.textContent)).toContain('4194304');
    expect(screen.getAllByText('4294967295', { exact: true })).toHaveLength(5);
    expect(screen.getByText('18446744073709551615', { exact: true })).toBeTruthy();
  });
});
