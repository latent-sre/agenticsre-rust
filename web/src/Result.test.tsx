import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import type { ReactNode } from 'react';
import { createApi } from './api';
import type { Receipt, Run, Session } from './model';
import { Result } from './Result';

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
  root_id: 'checkout', state: 'terminal', submitted_at: receipt.started_at,
  execution_status: 'succeeded', receipt_url: '/api/v1/runs/inv-fixture/receipt' };
const session: Session = { api_version: '0.1', session_id: 'session-fixture', roots: [{ id: 'checkout', label: 'Fixture' }],
  operations: [{ id: 'command.inspect', version: 1, profile: 'linux-read-v1' }],
  limits: { active_runs: 1, history_entries: 50, history_bytes: 16777216, max_request_bytes: 65536, max_submissions: 1024 } };

function mountResult(tab: 'output' | 'details', response = new Response(originalText, { headers: { 'content-type': 'application/json' } })) {
  const fetcher = vi.fn<typeof fetch>().mockResolvedValue(response);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(<QueryClientProvider client={client}><Result api={createApi('synthetic-token', fetcher)}
    run={run} session={session} tab={tab} onCancel={vi.fn()} cancelling={false} /></QueryClientProvider>);
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
