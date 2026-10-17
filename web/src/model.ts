import type { components } from './generated/api';
import type { MethodResponse } from 'openapi-fetch';
import type { Api } from './api';

// Use the client's generated read shape: its readOnly transform widens schema tuples to arrays.
export type Session = MethodResponse<Api, 'get', '/api/v1/session'>;
export type Run = components['schemas']['RunSummary'];
export type Receipt = components['schemas']['CoreResult'];
export type Submission = components['schemas']['RunSubmission'];
export type Problem = components['schemas']['Problem'];
export type DiagnosticData = components['schemas']['CoreDiagnosticsData'];
export type DiagnosticStep = components['schemas']['CoreDiagnosticsStep'];
export const diagnosticStepNames: Record<DiagnosticStep['id'], string> = {
  dns: 'DNS lookup', tcp: 'TCP connection', tls: 'TLS validation',
  http: 'HTTP response', process: 'Local process',
};
export type Check = 'status' | 'diff' | 'log' | 'files' | 'search' | 'budget' | 'diagnostics';
export type Theme = 'light' | 'dark' | 'system';
export const checkNames: Record<Check, string> = {
  status: 'Working tree status', diff: 'Working tree diff', log: 'Recent commits',
  files: 'List files', search: 'Find literal text', budget: 'Error budget', diagnostics: 'Diagnostics',
};
export const checkOptions = Object.keys(checkNames) as Check[];
export const isCheck = (value: unknown): value is Check =>
  typeof value === 'string' && checkOptions.includes(value as Check);

export function availableChecks(session: Session): Check[] {
  const commands = session.roots.length > 0 && session.operations.some(op =>
    op.id === 'process.exec' || op.id === 'command.inspect');
  return checkOptions.filter(check => check === 'budget'
    ? session.operations.some(op => op.id === 'task.run')
    : check === 'diagnostics' ? session.targets.length > 0 && diagnosticPack(session) !== undefined : commands);
}
export const diagnosticPack = (session: Session) => session.operations.find(op => op.id === 'diagnostics.run')?.packs[0];
export const runScope = (run: Run, session: Session) => run.operation === 'diagnostics.run'
  ? plainText(session.targets.find(target => target.id === run.target_id)?.label ?? run.target_id ?? 'Diagnostic target unavailable')
  : run.root_id ? plainText(session.roots.find(root => root.id === run.root_id)?.label ?? run.root_id) : 'Numerical input';

export type FormValues = {
  check: Check; root: string; path: string; pattern: string; count: string;
  cached: boolean; hidden: boolean; ignoreCase: boolean;
  diffFormat: 'patch' | 'stat' | 'name-status'; slo: string; days: string;
  minutes: string; timeout: string; target: string; captureBody: boolean;
  onFailure: 'continue' | 'stop'; concurrency: 1 | 2; outputLimit: string;
};
export const initialValues: FormValues = {
  check: 'status', root: '', path: '', pattern: '', count: '20', cached: false,
  hidden: false, ignoreCase: false, diffFormat: 'patch', slo: '99.9', days: '28',
  minutes: '20', timeout: '30', target: '', captureBody: false,
  onFailure: 'continue', concurrency: 2, outputLimit: '16384',
};
export type FieldErrors = Partial<Record<keyof FormValues, string>>;
const utf8Length = (value: string) => new TextEncoder().encode(value).length;

export function validateForm(values: FormValues): FieldErrors {
  const errors: FieldErrors = {};
  const number = (field: 'slo' | 'days' | 'minutes' | 'timeout' | 'count' | 'outputLimit', valid: (v: number) => boolean, message: string) => {
    const raw = values[field].trim();
    const value = Number(raw);
    if (!/^[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?$/u.test(raw) || !Number.isFinite(value) || !valid(value)) errors[field] = message;
  };
  const timeoutCeiling = values.check === 'diagnostics' ? 60 : 300;
  number('timeout', v => v >= 0.001 && v <= timeoutCeiling && /^\d+(?:\.\d{1,3})?$/u.test(values.timeout.trim()), `Use 0.001–${timeoutCeiling} seconds, to the nearest millisecond.`);
  if (values.check === 'budget') {
    number('slo', v => v > 0 && v < 100, 'Enter an SLO greater than 0 and less than 100%.');
    number('days', v => v > 0, 'Enter a positive number of days.');
    number('minutes', v => v >= 0, 'Enter zero or a positive number of minutes.');
  } else if (values.check === 'diagnostics') {
    if (!values.target) errors.target = 'Select a granted diagnostic target.';
    number('outputLimit', v => Number.isInteger(v) && v >= 1024 && v <= 16384 && /^\d+$/u.test(values.outputLimit.trim()), 'Use a whole number from 1,024 to 16,384 bytes.');
  } else {
    if (!values.root) errors.root = 'Select a granted workspace.';
    const path = values.path;
    if (path && (utf8Length(path) > 1024 || path === '-' || path.startsWith('/') || /[\\\0]/u.test(path)
      || path.split('/').some(part => !part || part === '..')
      || ((values.check === 'files' || values.check === 'search') && path.split('/').includes('.git')))) {
      errors.path = 'Use a relative path inside this workspace, without parent segments or Git metadata.';
    }
    if (values.check === 'search' && (!values.pattern || utf8Length(values.pattern) > 4096 || values.pattern.includes('\0'))) {
      errors.pattern = 'Enter literal text of 1–4,096 UTF-8 bytes.';
    }
    if (values.check === 'log') number('count', v => Number.isInteger(v) && v >= 1 && v <= 100, 'Enter a whole number from 1 to 100.');
  }
  return errors;
}

export function commandInput(values: FormValues): components['schemas']['CommandSubmission']['inputs'] {
  let args: string[];
  switch (values.check) {
    case 'status': args = ['status', '--short']; break;
    case 'diff': args = ['diff', ...(values.cached ? ['--cached'] : []), ...(values.diffFormat === 'patch' ? [] : [`--${values.diffFormat}`])]; break;
    case 'log': args = ['log', '-n', values.count]; break;
    case 'files': args = ['--files', ...(values.hidden ? ['--hidden'] : [])]; break;
    case 'search': args = ['-F', ...(values.ignoreCase ? ['-i'] : []), ...(values.hidden ? ['--hidden'] : []), '-e', values.pattern]; break;
    case 'budget': throw new Error('A calculation has no command arguments.');
    case 'diagnostics': throw new Error('Diagnostics have no command arguments.');
  }
  if (values.path) args.push('--', values.path);
  return { program: values.check === 'files' || values.check === 'search' ? 'rg' : 'git', args };
}

export function makeSubmission(values: FormValues, operation: Submission['operation'], id: string): Submission {
  const shared = { submission_id: id, operation_version: 1 as const,
    limits: { timeout_ms: Math.round(Number(values.timeout) * 1000), max_output_bytes: 1_048_576 } };
  if (values.check === 'diagnostics') return { ...shared, operation: 'diagnostics.run', target_id: values.target,
    inputs: { id: 'connectivity', version: 1, input: { capture_body: values.captureBody, on_failure: values.onFailure } },
    limits: { ...shared.limits, max_output_bytes: Number(values.outputLimit), concurrency: values.concurrency },
  };
  if (values.check === 'budget') return { ...shared, operation: 'task.run', inputs: {
    id: 'error-budget', version: 1, input: { slo: Number(values.slo), window_days: Number(values.days), bad_minutes: Number(values.minutes) },
  } };
  if (operation === 'task.run' || operation === 'diagnostics.run') throw new Error('Select a command operation.');
  return { ...shared, operation, root_id: values.root, inputs: commandInput(values) };
}

// Display controls visibly; captured bytes remain unchanged in the downloaded receipt.
export function plainText(value: string): string {
  // eslint-disable-next-line no-control-regex -- Deliberately render control bytes visibly.
  return value.replace(/[\u0000-\u0008\u000b-\u001f\u007f-\u009f\u202a-\u202e\u2066-\u2069]/gu,
    char => `\\u${char.charCodeAt(0).toString(16).padStart(4, '0')}`);
}
export const humanize = (value: string) => value.replaceAll('_', ' ');
export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}
export function diagnosticData(receipt: Receipt): DiagnosticData | null {
  const data: unknown = receipt.data;
  if (receipt.operation !== 'diagnostics.run' || !isRecord(data)
    || !isRecord(data.binding) || !isRecord(data.binding.identity) || !isRecord(data.pack)
    || !isRecord(data.vantage) || !Array.isArray(data.steps) || data.steps.length !== 5
    || !data.steps.every(step => isRecord(step) && typeof step.id === 'string' && Object.hasOwn(diagnosticStepNames, step.id)
      && typeof step.state === 'string' && typeof step.attempted === 'boolean' && isRecord(step.observation))) return null;
  // Schema-derived presentation applies only to pack data; admission refusals have none.
  return data as DiagnosticData;
}
export function budgetStatus(receipt: Receipt) {
  const data: unknown = receipt.data;
  if (!isRecord(data) || !isRecord(data.calculation) || !isRecord(data.calculation.status)) return null;
  const status = data.calculation.status;
  if (status.kind !== 'time' || typeof status.state !== 'string'
    || !['budget', 'consumed', 'remaining', 'consumed_percent'].every(key => typeof status[key] === 'number' && Number.isFinite(status[key]))) return null;
  // These are presentation values, read from the core's open operation-data object.
  return { state: status.state, budget: Number(status.budget), consumed: Number(status.consumed),
    remaining: Number(status.remaining), percent: Number(status.consumed_percent) };
}
export const formatNumber = (value: number) => new Intl.NumberFormat(undefined, {
  maximumSignificantDigits: 6,
  notation: value !== 0 && (Math.abs(value) < 0.0001 || Math.abs(value) >= 1e12) ? 'scientific' : 'standard',
}).format(value === 0 ? 0 : value);
export const formatBytes = (value: number) => value < 1024 ? `${value} B` : `${formatNumber(value / 1024)} KiB`;
