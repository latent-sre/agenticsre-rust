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
export type Check = 'status' | 'diff' | 'log' | 'files' | 'search' | 'budget' | 'diagnostics' | 'context' | 'evidence-list' | 'evidence-open' | 'evidence-compare' | 'evidence-report' | 'queries-list' | 'queries-describe' | 'query-run' | 'evidence-packs-list' | 'evidence-packs-describe' | 'evidence-collect';
export type Theme = 'light' | 'dark' | 'system';
export const checkNames: Record<Check, string> = {
  status: 'Working tree status', diff: 'Working tree diff', log: 'Recent commits',
  files: 'List files', search: 'Find literal text', budget: 'Error budget', diagnostics: 'Diagnostics',
  context: 'Service context', 'evidence-list': 'Saved evidence', 'evidence-open': 'Inspect evidence',
  'evidence-compare': 'Compare evidence', 'evidence-report': 'Evidence report',
  'evidence-packs-list': 'Evidence pack catalog', 'evidence-packs-describe': 'Inspect evidence pack', 'evidence-collect': 'Collect evidence',
  'queries-list': 'Saved query catalog', 'queries-describe': 'Inspect saved query', 'query-run': 'Run saved query',
};
export const checkOptions = Object.keys(checkNames) as Check[];
export const isCheck = (value: unknown): value is Check =>
  typeof value === 'string' && checkOptions.includes(value as Check);

export const investigationOperations = { context: 'context.resolve', 'evidence-list': 'evidence.list',
  'evidence-open': 'evidence.open', 'evidence-compare': 'evidence.compare', 'evidence-report': 'report.create' } as const;
export const isInvestigation = (check: Check): check is keyof typeof investigationOperations => Object.hasOwn(investigationOperations, check);
export const collectionOperations = { 'evidence-packs-list': 'evidence.packs.list', 'evidence-packs-describe': 'evidence.packs.describe', 'evidence-collect': 'evidence.collect' } as const;
export const isCollection = (check: Check): check is keyof typeof collectionOperations => Object.hasOwn(collectionOperations, check);
export const queryOperations = { 'queries-list': 'queries.list', 'queries-describe': 'queries.describe', 'query-run': 'query.run' } as const;
export const isQuery = (check: Check): check is keyof typeof queryOperations => Object.hasOwn(queryOperations, check);
export const queryDefinitions = { 'prometheus.up-by-job': 'PromQL · scrape samples by job · version 1', 'loki.logs-by-job': 'LogQL · log entries by job · version 1' } as const;
export const isBundleId = (value: unknown): value is string => typeof value === 'string' && value.length <= 96 && /^run-[0-9]+-[a-f0-9]+-[a-f0-9]+$/u.test(value);
export function availableChecks(session: Session): Check[] {
  const commands = session.roots.length > 0 && session.operations.some(op =>
    op.id === 'process.exec' || op.id === 'command.inspect');
  return checkOptions.filter(check => isCollection(check) ? session.operations.some(op => op.id === collectionOperations[check]) && (check !== 'evidence-collect' || (session.collection_targets?.length ?? 0) > 0) : isQuery(check) ? session.operations.some(op => op.id === queryOperations[check]) && (check !== 'query-run' || (session.query_targets?.length ?? 0) > 0) : isInvestigation(check) ? session.operations.some(op => op.id === investigationOperations[check]) : check === 'budget'
    ? session.operations.some(op => op.id === 'task.run')
    : check === 'diagnostics' ? session.targets.length > 0 && diagnosticPack(session) !== undefined : commands);
}
export const diagnosticPack = (session: Session) => session.operations.find(op => op.id === 'diagnostics.run')?.packs[0];
export const runScope = (run: Run, session: Session) => run.operation === 'diagnostics.run'
  ? plainText(session.targets.find(target => target.id === run.target_id)?.label ?? run.target_id ?? 'Diagnostic target unavailable')
  : run.operation === 'evidence.collect' ? plainText(session.collection_targets?.find(target => target.id === run.target_id)?.label ?? run.target_id ?? 'Collection target unavailable')
  : run.operation === 'evidence.packs.list' || run.operation === 'evidence.packs.describe' ? 'Installed evidence pack catalog'
  : run.operation === 'query.run' ? plainText(session.query_targets?.find(target => target.id === run.target_id)?.label ?? run.target_id ?? 'Query target unavailable')
  : run.operation === 'queries.list' || run.operation === 'queries.describe' ? 'Installed query catalog'
  : Object.values(investigationOperations).some(operation => operation === run.operation) ? 'Historical / offline evidence' : run.root_id ? plainText(session.roots.find(root => root.id === run.root_id)?.label ?? run.root_id) : 'Numerical input';

export type FormValues = {
  check: Check; root: string; path: string; pattern: string; count: string;
  cached: boolean; hidden: boolean; ignoreCase: boolean;
  diffFormat: 'patch' | 'stat' | 'name-status'; slo: string; days: string;
  minutes: string; timeout: string; target: string; captureBody: boolean;
  onFailure: 'continue' | 'stop'; concurrency: 1 | 2; outputLimit: string;
  service: string; environment: string; team: string; deployment: string;
  bundleId: string; bundlePin: string; beforeId: string; beforePin: string; afterId: string; afterPin: string;
  normalization: 'exact-window-v1' | 'window-offset-v1'; maxItems: string;
  reportId1: string; reportPin1: string; reportId2: string; reportPin2: string; reportId3: string; reportPin3: string; reportId4: string; reportPin4: string;
  reportFormat: 'markdown' | 'json'; reportTitle: string;
  metricsDatasource: string; metricsJob: string; logsDatasource: string; logsJob: string; packPin: string;
  queryId: keyof typeof queryDefinitions; datasource: string; job: string; definitionPin: string;
  windowKind: 'relative' | 'absolute'; lookback: string; windowFrom: string; windowTo: string;
};
export const initialValues: FormValues = {
  check: 'status', root: '', path: '', pattern: '', count: '20', cached: false,
  hidden: false, ignoreCase: false, diffFormat: 'patch', slo: '99.9', days: '28',
  minutes: '20', timeout: '30', target: '', captureBody: false,
  onFailure: 'continue', concurrency: 2, outputLimit: '16384',
  service: '', environment: '', team: '', deployment: '', bundleId: '', bundlePin: '',
  beforeId: '', beforePin: '', afterId: '', afterPin: '', normalization: 'exact-window-v1', maxItems: '100',
  reportId1: '', reportPin1: '', reportId2: '', reportPin2: '', reportId3: '', reportPin3: '', reportId4: '', reportPin4: '', reportFormat: 'markdown', reportTitle: '',
  metricsDatasource: '', metricsJob: '', logsDatasource: '', logsJob: '', packPin: '',
  queryId: 'prometheus.up-by-job', datasource: '', job: '', definitionPin: '', windowKind: 'relative', lookback: '300', windowFrom: '', windowTo: '',
};
export type FieldErrors = Partial<Record<keyof FormValues, string>>;
const hasControl = (value: string) => Array.from(value).some(character => character.charCodeAt(0) <= 31 || character.charCodeAt(0) === 127);
const utf8Length = (value: string) => new TextEncoder().encode(value).length;
// Calendar validation avoids Date.parse silently normalizing an impossible day.
function exactTimestamp(value: string): number | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.(\d{1,3}))?(Z|[+-]\d{2}:\d{2})$/u.exec(value);
  if (!match) return null;
  const [, year, month, day, hour, minute, second, , offset] = match;
  const calendar = new Date(`${year}-${month}-${day}T00:00:00Z`);
  const stamp = Date.parse(value);
  if (!Number.isFinite(stamp) || !Number.isFinite(calendar.getTime()) || calendar.toISOString().slice(0, 10) !== `${year}-${month}-${day}`
    || Number(hour) > 23 || Number(minute) > 59 || Number(second) > 59
    || (offset !== 'Z' && (Number(offset?.slice(1, 3)) > 23 || Number(offset?.slice(4)) > 59))) return null;
  return stamp;
}

export function validateForm(values: FormValues): FieldErrors {
  const errors: FieldErrors = {};
  const number = (field: 'slo' | 'days' | 'minutes' | 'timeout' | 'count' | 'outputLimit' | 'maxItems' | 'lookback', valid: (v: number) => boolean, message: string) => {
    const raw = values[field].trim();
    const value = Number(raw);
    if (!/^[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?$/u.test(raw) || !Number.isFinite(value) || !valid(value)) errors[field] = message;
  };
  const timeoutCeiling = values.check === 'diagnostics' || values.check === 'evidence-collect' ? 60 : 300;
  number('timeout', v => v >= 0.001 && v <= timeoutCeiling && /^\d+(?:\.\d{1,3})?$/u.test(values.timeout.trim()), `Use 0.001–${timeoutCeiling} seconds, to the nearest millisecond.`);
  if (isCollection(values.check)) {
    number('outputLimit', v => Number.isInteger(v) && v >= 1024 && v <= 2_097_152 && /^\d+$/u.test(values.outputLimit), 'Use a whole number from 1,024 to 2,097,152 bytes.');
    if (values.check === 'evidence-collect') {
      if (!values.target) errors.target = 'Select a granted collection target.';
      for (const field of ['service', 'environment', 'team', 'deployment'] as const) {
        if ((!values[field] && (field === 'service' || field === 'environment')) || values[field].length > 128 || hasControl(values[field])) errors[field] = 'Use 1–128 characters without control characters.';
      }
      // Reuse the installed-query validation for each literal query and their common window.
      for (const [datasource, job] of [['metricsDatasource', 'metricsJob'], ['logsDatasource', 'logsJob']] as const) {
        const checked = validateForm({ ...values, check: 'query-run', datasource: values[datasource], job: values[job], definitionPin: values.packPin, outputLimit: '1048576' });
        if (checked.datasource) errors[datasource] = checked.datasource;
        if (checked.job) errors[job] = checked.job;
        if (checked.definitionPin) errors.packPin = 'Use 64 lowercase hexadecimal characters without a sha256: prefix, or leave blank.';
        for (const field of ['lookback', 'windowFrom', 'windowTo'] as const) if (checked[field]) errors[field] = checked[field];
      }
    }
  } else if (isQuery(values.check)) {
    number('outputLimit', v => Number.isInteger(v) && v >= 1024 && v <= 1_048_576 && /^\d+$/u.test(values.outputLimit), 'Use a whole number from 1,024 to 1,048,576 bytes.');
    if (values.check !== 'queries-list' && !Object.hasOwn(queryDefinitions, values.queryId)) errors.queryId = 'Select an installed query definition.';
    if (values.check === 'query-run') {
      if (!values.target) errors.target = 'Select a granted query target.';
      if (!/^[A-Za-z0-9_-]{1,40}$/u.test(values.datasource)) errors.datasource = 'Use a datasource UID of 1–40 letters, digits, underscores or hyphens.';
      if (!values.job || utf8Length(values.job) > 128 || Array.from(values.job).some(character => character.charCodeAt(0) < 32 || (character.charCodeAt(0) >= 127 && character.charCodeAt(0) <= 159)) || /\$(?:[A-Za-z_]|\{)|\[\[[A-Za-z_][^\]]*\]\]/u.test(values.job)) errors.job = 'Use 1–128 UTF-8 bytes without control characters or query macros.';
      if (values.definitionPin && !/^[a-f0-9]{64}$/u.test(values.definitionPin)) errors.definitionPin = 'Use 64 lowercase hexadecimal characters, without a sha256: prefix, or leave blank.';
      if (values.windowKind === 'relative') number('lookback', v => Number.isInteger(v) && v >= 1 && v <= 86400 && /^\d+$/u.test(values.lookback), 'Use a whole number from 1 to 86,400 seconds.');
      else {
        const from = exactTimestamp(values.windowFrom); const to = exactTimestamp(values.windowTo);
        if (from === null) errors.windowFrom = 'Use an RFC3339 timestamp with timezone and at most millisecond precision.';
        if (to === null) errors.windowTo = 'Use an RFC3339 timestamp with timezone and at most millisecond precision.';
        else if (from !== null && (to <= from || to - from > 86_400_000)) errors.windowTo = 'The end must follow the start by at most 24 hours.';
      }
    }
  } else if (isInvestigation(values.check)) {
    number('outputLimit', v => Number.isInteger(v) && v >= 1024 && v <= 1_048_576 && /^\d+$/u.test(values.outputLimit), 'Use a whole number from 1,024 to 1,048,576 bytes.');
    const reference = (id: 'bundleId' | 'beforeId' | 'afterId' | 'reportId1' | 'reportId2' | 'reportId3' | 'reportId4', pin: 'bundlePin' | 'beforePin' | 'afterPin' | 'reportPin1' | 'reportPin2' | 'reportPin3' | 'reportPin4', required = true) => {
      if ((required || values[id] || values[pin]) && !isBundleId(values[id])) errors[id] = 'Enter a saved evidence ID, such as run-1-ab-c. Paths are not accepted.';
      if (values[pin] && !/^sha256:[a-f0-9]{64}$/u.test(values[pin])) errors[pin] = 'Use sha256: followed by 64 lowercase hexadecimal characters, or leave blank.';
    };
    if (values.check === 'context') for (const field of ['service', 'environment', 'team', 'deployment'] as const) {
      if ((!values[field] && (field === 'service' || field === 'environment')) || values[field].length > 128 || hasControl(values[field])) errors[field] = 'Use 1–128 characters without control characters.';
    }
    if (values.check === 'evidence-open') reference('bundleId', 'bundlePin');
    if (values.check === 'evidence-compare') { reference('beforeId', 'beforePin'); reference('afterId', 'afterPin'); }
    if (values.check === 'evidence-open' || values.check === 'evidence-compare') number('maxItems', v => Number.isInteger(v) && v >= 1 && v <= 1000 && /^\d+$/u.test(values.maxItems), 'Use a whole number from 1 to 1,000.');
    if (values.check === 'evidence-report') {
      const ids: string[] = [];
      for (const index of [1, 2, 3, 4] as const) {
        const key = `reportId${index}` as const;
        reference(key, `reportPin${index}`, index === 1);
        if (values[key] && ids.includes(values[key])) errors[key] = 'Choose each evidence ID once.';
        ids.push(values[key]);
      }
      if (values.reportTitle.length > 160 || hasControl(values.reportTitle)) errors.reportTitle = 'Use up to 160 characters without control characters.';
    }
  } else if (values.check === 'budget') {
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
    case 'context': case 'evidence-list': case 'evidence-open': case 'evidence-compare': case 'evidence-report': throw new Error('Offline investigation has no command arguments.');
    case 'diagnostics': throw new Error('Diagnostics have no command arguments.');
    case 'evidence-packs-list': case 'evidence-packs-describe': case 'evidence-collect': throw new Error('Evidence collection has no command arguments.');
    case 'queries-list': case 'queries-describe': case 'query-run': throw new Error('Saved queries have no command arguments.');
  }
  if (values.path) args.push('--', values.path);
  return { program: values.check === 'files' || values.check === 'search' ? 'rg' : 'git', args };
}

export function makeSubmission(values: FormValues, operation: Submission['operation'], id: string): Submission {
  const shared = { submission_id: id, operation_version: 1 as const,
    limits: { timeout_ms: Math.round(Number(values.timeout) * 1000), max_output_bytes: 1_048_576 } };
  if (isCollection(values.check)) {
    const collection = { ...shared, limits: { ...shared.limits, max_output_bytes: Number(values.outputLimit) } };
    if (values.check === 'evidence-packs-list') return { ...collection, operation: 'evidence.packs.list', inputs: {} };
    if (values.check === 'evidence-packs-describe') return { ...collection, operation: 'evidence.packs.describe', inputs: { id: 'service-queries', version: 1 } };
    return { ...collection, operation: 'evidence.collect', target_id: values.target, inputs: {
      service: values.service, environment: values.environment,
      ...(values.team ? { team: values.team } : {}), ...(values.deployment ? { deployment: values.deployment } : {}),
      pack: { id: 'service-queries', version: 1 },
      metrics: { datasource: values.metricsDatasource, parameters: { job: values.metricsJob } },
      logs: { datasource: values.logsDatasource, parameters: { job: values.logsJob } },
      window: values.windowKind === 'relative' ? { kind: 'relative', lookback_seconds: Number(values.lookback) } : { kind: 'absolute', from: values.windowFrom, to: values.windowTo },
      on_failure: values.onFailure, ...(values.packPin ? { expected_pack_sha256: values.packPin } : {}),
    } };
  }
  if (isQuery(values.check)) {
    const query = { ...shared, limits: { ...shared.limits, max_output_bytes: Number(values.outputLimit) } };
    if (values.check === 'queries-list') return { ...query, operation: 'queries.list', inputs: {} };
    if (values.check === 'queries-describe') return { ...query, operation: 'queries.describe', inputs: { id: values.queryId, version: 1 } };
    return { ...query, operation: 'query.run', target_id: values.target, inputs: {
      id: values.queryId, version: 1, datasource: values.datasource, parameters: { job: values.job },
      window: values.windowKind === 'relative' ? { kind: 'relative', lookback_seconds: Number(values.lookback) } : { kind: 'absolute', from: values.windowFrom, to: values.windowTo },
      ...(values.definitionPin ? { expected_definition_sha256: values.definitionPin } : {}),
    } };
  }
  if (isInvestigation(values.check)) {
    const reference = (id: string, pin: string) => ({ id, ...(pin ? { expected_manifest_sha256: pin } : {}) });
    const limits = { ...shared.limits, max_output_bytes: Number(values.outputLimit) };
    const offline = { ...shared, limits };
    if (values.check === 'context') return { ...offline, operation: 'context.resolve', inputs: { service: values.service, environment: values.environment, ...(values.team ? { team: values.team } : {}), ...(values.deployment ? { deployment: values.deployment } : {}) } };
    if (values.check === 'evidence-list') return { ...offline, operation: 'evidence.list', inputs: {} };
    if (values.check === 'evidence-open') return { ...offline, operation: 'evidence.open', inputs: reference(values.bundleId, values.bundlePin), limits: { ...limits, max_items: Number(values.maxItems) } };
    if (values.check === 'evidence-compare') return { ...offline, operation: 'evidence.compare', inputs: { before: reference(values.beforeId, values.beforePin), after: reference(values.afterId, values.afterPin), normalization: values.normalization }, limits: { ...limits, max_items: Number(values.maxItems) } };
    return { ...offline, operation: 'report.create', inputs: { bundles: ([1, 2, 3, 4] as const).filter(index => values[`reportId${index}`]).map(index => reference(values[`reportId${index}`], values[`reportPin${index}`])), format: values.reportFormat, ...(values.reportTitle ? { title: values.reportTitle } : {}) } };
  }
  if (values.check === 'diagnostics') return { ...shared, operation: 'diagnostics.run', target_id: values.target,
    inputs: { id: 'connectivity', version: 1, input: { capture_body: values.captureBody, on_failure: values.onFailure } },
    limits: { ...shared.limits, max_output_bytes: Number(values.outputLimit), concurrency: values.concurrency },
  };
  if (values.check === 'budget') return { ...shared, operation: 'task.run', inputs: {
    id: 'error-budget', version: 1, input: { slo: Number(values.slo), window_days: Number(values.days), bad_minutes: Number(values.minutes) },
  } };
  if (operation !== 'process.exec' && operation !== 'command.inspect') throw new Error('Select a command operation.');
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
