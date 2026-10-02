import { describe, expect, it } from 'vitest';
import { commandInput, formatNumber, initialValues, makeSubmission, plainText, validateForm } from './model';

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
  it('exposes escape and directional controls without interpreting output', () => {
    expect(plainText('\x1b[31m<b>failed</b>\u202e\n')).toBe('\\u001b[31m<b>failed</b>\\u202e\n');
  });
  it('does not display a tiny positive budget as zero', () => {
    expect(formatNumber(0.000000004032)).not.toBe('0');
    expect(formatNumber(40.32)).toBe('40.32');
    expect(formatNumber(-0)).toBe('0');
  });
});
