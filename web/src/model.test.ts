import { describe, expect, it } from 'vitest';
import { availableChecks, commandInput, formatNumber, initialValues, makeSubmission, plainText, validateForm } from './model';
import type { Session } from './model';
import apiContract from '../../docs/ui/openapi.json';

describe('check admission', () => {
  it('preserves literal metacharacters, spaces, Unicode and a -- pattern as single arguments', () => {
    const values = { ...initialValues, check: 'search' as const, root: 'checkout', path: 'src/雪 $(literal).txt', pattern: '--' };
    expect(validateForm(values)).toEqual({});
    expect(commandInput(values)).toEqual({ program: 'rg', args: ['-F', '-e', '--', '--', values.path] });
  });
  it.each(['../secret', '/etc/passwd', 'src//file', 'src/', './.git/config', 'nested/.git/HEAD'])('refuses unsupported search path %s', path => {
    expect(validateForm({ ...initialValues, root: 'checkout', check: 'search', pattern: 'x', path }).path).toBeTruthy();
  });
  it.each(['', 'Infinity', 'NaN', '100', '0', '-1', '0x10'])('refuses invalid SLO %s', slo => {
    expect(validateForm({ ...initialValues, check: 'budget', slo }).slo).toBeTruthy();
  });
  it('admits exact decimal milliseconds despite binary64 multiplication rounding', () => {
    const values = { ...initialValues, check: 'budget' as const, timeout: '1.001' };
    expect(validateForm(values)).toEqual({});
    expect(makeSubmission(values, 'task.run', 'id').limits.timeout_ms).toBe(1001);
    expect(validateForm({ ...values, timeout: '1.0001' }).timeout).toBeTruthy();
  });
  it('creates only the typed numerical task, no root or caller authority', () => {
    const request = makeSubmission({ ...initialValues, check: 'budget' }, 'task.run', 'stable-id');
    expect(request).toEqual({ submission_id: 'stable-id', operation: 'task.run', operation_version: 1,
      inputs: { id: 'error-budget', version: 1, input: { slo: 99.9, window_days: 28, bad_minutes: 20 } },
      limits: { timeout_ms: 30_000, max_output_bytes: 1_048_576 } });
  });
  it('offers diagnostics only with the granted operation and a target', () => {
    const session = apiContract.components.schemas.Session.examples[0] as Session;
    expect(availableChecks(session)).toEqual(['diagnostics']);
    expect(availableChecks({ ...session, operations: [] })).toEqual([]);
    expect(availableChecks({ ...session, targets: [] })).toEqual([]);
  });
  it('builds a fixed diagnostic request with separate alias and effective limits', () => {
    const values = { ...initialValues, check: 'diagnostics' as const, target: 'fixture-web', root: 'checkout' };
    expect(validateForm(values)).toEqual({});
    expect(makeSubmission(values, 'diagnostics.run', 'stable-id')).toEqual({
      submission_id: 'stable-id', operation: 'diagnostics.run', operation_version: 1,
      target_id: 'fixture-web', inputs: { id: 'connectivity', version: 1,
        input: { capture_body: false, on_failure: 'continue' } },
      limits: { timeout_ms: 30_000, max_output_bytes: 16_384, concurrency: 2 },
    });
  });
  it('preserves explicit capture, stop and serial choices only on diagnostics', () => {
    const values = { ...initialValues, check: 'diagnostics' as const, target: 'fixture-web',
      captureBody: true, onFailure: 'stop' as const, concurrency: 1 as const, outputLimit: '1024' };
    expect(makeSubmission(values, 'diagnostics.run', 'same-intent')).toMatchObject({
      inputs: { input: { capture_body: true, on_failure: 'stop' } },
      limits: { max_output_bytes: 1024, concurrency: 1 },
    });
    const command = makeSubmission({ ...values, check: 'status', root: 'checkout' }, 'process.exec', 'command');
    expect(command).not.toHaveProperty('target_id');
    expect(command.limits).not.toHaveProperty('concurrency');
    expect(command).toMatchObject({ root_id: 'checkout', limits: { max_output_bytes: 1_048_576 } });
    const budget = makeSubmission({ ...values, check: 'budget' }, 'task.run', 'budget');
    expect(budget).not.toHaveProperty('target_id');
    expect(budget.limits).not.toHaveProperty('concurrency');
  });
  it('requires an alias and enforces the diagnostic deadline and effective output bounds', () => {
    const values = { ...initialValues, check: 'diagnostics' as const, target: 'fixture-web' };
    expect(validateForm({ ...values, target: '' }).target).toBe('Select a granted diagnostic target.');
    expect(validateForm({ ...values, timeout: '60' })).toEqual({});
    expect(validateForm({ ...values, timeout: '60.001' }).timeout).toContain('60 seconds');
    expect(validateForm({ ...values, outputLimit: '1024' })).toEqual({});
    for (const outputLimit of ['1023', '16385', '1.5', '0x1000', 'Infinity']) {
      expect(validateForm({ ...values, outputLimit }).outputLimit).toBeTruthy();
    }
    expect(validateForm({ ...values, check: 'budget', timeout: '300' })).toEqual({});
  });
  it('exposes escape and directional controls without interpreting output', () => {
    expect(plainText('\x1b[31m<b>failed</b>\u202e\n')).toBe('\\u001b[31m<b>failed</b>\\u202e\n');
  });
  it('does not display a tiny positive budget as zero', () => {
    expect(formatNumber(0.000000004032)).not.toBe('0');
    expect(formatNumber(40.32)).toBe('40.32');
    expect(formatNumber(-0)).toBe('0');
  });
});
