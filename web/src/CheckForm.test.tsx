import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { CheckForm } from './CheckForm';
import type { Session } from './model';

afterEach(cleanup);
const session: Session = { api_version: '0.1', session_id: 'synthetic', roots: [{ id: 'checkout', label: 'Fixture checkout' }],
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
});
