import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { CheckForm } from './CheckForm';
import type { Session } from './model';
import apiContract from '../../docs/ui/openapi.json';

afterEach(cleanup);
const session: Session = { api_version: '0.1', session_id: 'synthetic', roots: [{ id: 'checkout', label: 'Fixture checkout' }], targets: [],
  operations: [{ id: 'task.run', version: 1, tasks: ['error-budget'] }, { id: 'process.exec', version: 1, profile: 'linux-read-v1' }],
  limits: { active_runs: 1, history_entries: 50, history_bytes: 16777216, max_request_bytes: 65536, max_submissions: 1024 } };

describe('operator check forms', () => {
  it('focuses the first invalid field and never submits invalid arithmetic', async () => {
    const submit = vi.fn();
    render(<CheckForm session={session} check="budget" busy={false} submitting={false} onSubmit={submit} onSelection={vi.fn()} />);
    const user = userEvent.setup();
    await user.clear(screen.getByLabelText('Availability SLO (%)'));
    await user.type(screen.getByLabelText('Availability SLO (%)'), '100');
    await user.click(screen.getByRole('button', { name: 'Calculate budget' }));
    expect(submit).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(screen.getByLabelText('Availability SLO (%)'));
    expect(screen.getByLabelText('Availability SLO (%)').getAttribute('aria-invalid')).toBe('true');
  });
  it('offers only granted operations and preserves a typed literal search', async () => {
    const submit = vi.fn();
    render(<CheckForm session={session} check="search" busy={false} submitting={false} onSubmit={submit} onSelection={vi.fn()} />);
    const user = userEvent.setup();
    await user.type(screen.getByLabelText('Literal text'), '$(echo café); <script>');
    await user.type(screen.getByLabelText('Path (optional)'), 'literal λ.txt');
    await user.click(screen.getByRole('button', { name: 'Run check' }));
    expect(submit).toHaveBeenCalledTimes(1);
    expect(submit.mock.calls[0]?.[0]).toMatchObject({ operation: 'process.exec', root_id: 'checkout', inputs: { program: 'rg', args: ['-F', '-e', '$(echo café); <script>', '--', 'literal λ.txt'] } });
    expect(screen.queryByRole('button', { name: 'Inspect' })).toBeNull();
  });
  it('uses inspect for a session without execution permission', async () => {
    const submit = vi.fn();
    const inspected: Session = { ...session, operations: [{ id: 'command.inspect', version: 1, profile: 'linux-read-v1' }] };
    render(<CheckForm session={inspected} busy={false} submitting={false} onSubmit={submit} onSelection={vi.fn()} />);
    await userEvent.setup().click(screen.getByRole('button', { name: 'Inspect check' }));
    expect(submit.mock.calls[0]?.[0]).toMatchObject({ operation: 'command.inspect', inputs: { program: 'git', args: ['status', '--short'] } });
    expect(screen.queryByRole('option', { name: 'Error budget' })).toBeNull();
  });
  it('disables both inspection and execution while an earlier admission is unresolved', () => {
    render(<CheckForm session={{ ...session, operations: [...session.operations, { id: 'command.inspect', version: 1, profile: 'linux-read-v1' }] }} busy submitting={false} onSubmit={vi.fn()} onSelection={vi.fn()} />);
    expect(screen.getByRole<HTMLButtonElement>('button', { name: 'Run check' }).disabled).toBe(true);
    expect(screen.getByRole<HTMLButtonElement>('button', { name: 'Inspect' }).disabled).toBe(true);
  });
  it('shows granted aliases and the fixed checks with body capture initially off', async () => {
    const submit = vi.fn();
    const diagnosticSession = apiContract.components.schemas.Session.examples[0] as Session;
    render(<CheckForm session={diagnosticSession} check="diagnostics" busy={false} submitting={false} onSubmit={submit} onSelection={vi.fn()} />);
    expect(screen.getByLabelText('Diagnostic target')).toHaveProperty('value', 'fixture-web');
    expect(screen.queryByLabelText('Workspace')).toBeNull();
    expect(screen.getByLabelText('Capture response body')).toHaveProperty('checked', false);
    expect(screen.getByText('DNS → TCP → TLS → HTTP, plus an independent local process observation.')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Inspect' })).toBeNull();
    await userEvent.setup().click(screen.getByRole('button', { name: 'Run diagnostics' }));
    expect(submit.mock.calls[0]?.[0]).toMatchObject({ operation: 'diagnostics.run', target_id: 'fixture-web',
      inputs: { id: 'connectivity', version: 1, input: { capture_body: false, on_failure: 'continue' } },
      limits: { max_output_bytes: 16_384, concurrency: 2 } });
    expect(submit.mock.calls[0]?.[0]).not.toHaveProperty('root_id');
  });
  it('validates the diagnostic ceiling and sends explicit bounded choices', async () => {
    const submit = vi.fn();
    const diagnosticSession = apiContract.components.schemas.Session.examples[0] as Session;
    render(<CheckForm session={diagnosticSession} check="diagnostics" busy={false} submitting={false} onSubmit={submit} onSelection={vi.fn()} />);
    const user = userEvent.setup();
    await user.click(screen.getByLabelText('Capture response body'));
    await user.selectOptions(screen.getByLabelText('On a failed check'), 'stop');
    await user.selectOptions(screen.getByLabelText('Concurrent checks'), '1');
    await user.click(screen.getByText('Run limits'));
    const timeout = screen.getByLabelText('Timeout (seconds)');
    await user.clear(timeout);
    await user.type(timeout, '61');
    await user.click(screen.getByRole('button', { name: 'Run diagnostics' }));
    expect(submit).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(timeout);
    expect(screen.getByText('Use 0.001–60 seconds, to the nearest millisecond.')).toBeTruthy();
    await user.clear(timeout);
    await user.type(timeout, '60');
    const output = screen.getByLabelText('Output allowance (bytes)');
    await user.clear(output);
    await user.type(output, '1024');
    await user.click(screen.getByRole('button', { name: 'Run diagnostics' }));
    expect(submit.mock.calls[0]?.[0]).toMatchObject({ inputs: { input: { capture_body: true, on_failure: 'stop' } },
      limits: { timeout_ms: 60_000, max_output_bytes: 1024, concurrency: 1 } });
  });
  it('explains an unavailable diagnostic grant without offering it', () => {
    render(<CheckForm session={session} busy={false} submitting={false} onSubmit={vi.fn()} onSelection={vi.fn()} />);
    expect(screen.queryByRole('option', { name: 'Diagnostics' })).toBeNull();
    expect(screen.getByText('Diagnostics require a new session with explicitly granted targets.')).toBeTruthy();
  });
});
