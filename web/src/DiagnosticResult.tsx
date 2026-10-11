import { Badge } from './components';
import { diagnosticStepNames as stepNames, formatNumber, humanize, isRecord, plainText } from './model';
import type { DiagnosticData, DiagnosticStep, Receipt } from './model';

function observationSummary(step: DiagnosticStep): string {
  const observation = step.observation;
  const text = (key: string) => typeof observation[key] === 'string' ? plainText(observation[key]) : '';
  if (step.id === 'dns' && text('selected_peer')) return `Selected service peer: ${text('selected_peer')}`;
  if (step.id === 'tcp' && text('peer')) return `Attempted service peer: ${text('peer')}`;
  if (step.id === 'tls' && text('protocol')) return `${text('protocol')} · hostname verified: ${observation.hostname_verified === true ? 'yes' : 'no'} · trust verified: ${observation.trust_verified === true ? 'yes' : 'no'}`;
  if (step.id === 'http' && typeof observation.status === 'number') return `HTTP status ${observation.status}`;
  if (step.id === 'process' && text('state')) return `Observed process state: ${text('state')}`;
  return Object.keys(observation).length ? 'Observation details below.' : 'No observation returned.';
}

export function DiagnosticOutcomes({ data }: { data: DiagnosticData }) {
  return <section className="diagnostic-result" aria-label="Diagnostic outcomes">
    <p className="form-description">Five check outcomes from this run. Observations do not assess service health.</p>
    <dl className="detail-pairs"><div><dt>Configured service / TLS name</dt><dd>{plainText(data.binding.hostname)}</dd></div><div><dt>Configured port and path</dt><dd>{data.binding.port} · {plainText(data.binding.path)}</dd></div></dl>
    <ol className="diagnostic-steps">{data.steps.map(step => <li key={step.id}>
      <section className="diagnostic-step" aria-label={stepNames[step.id]}>
        <header><h3>{stepNames[step.id]}</h3><Badge status={step.state} /></header>
        <p className="step-timing">{step.attempted ? 'Attempted' : 'Not run'} · Elapsed: {step.duration_ms === null ? 'Not recorded' : `${formatNumber(step.duration_ms)} ms`}</p>
        {step.reason && <p className="step-reason">Reason: <code>{plainText(step.reason)}</code></p>}
        <p className="step-summary">{observationSummary(step)}</p>
        <details className="step-observation"><summary>Actual observation and check details</summary>
          <dl className="observation-fields"><div><dt>Operation</dt><dd>{plainText(step.operation)} · v{step.operation_version}</dd></div>
            <div><dt>Prerequisites</dt><dd>{step.prerequisites.length ? step.prerequisites.map(id => stepNames[id]).join(', ') : 'Independent check'}</dd></div>
            <div><dt>Observed at</dt><dd>{step.observed_at === null ? 'Not recorded' : plainText(step.observed_at)}</dd></div>
            <div><dt>Started after</dt><dd>{step.started_offset_ms === null ? 'Not recorded' : `${formatNumber(step.started_offset_ms)} ms`}</dd></div>
            <div><dt>Check timeout</dt><dd>{formatNumber(step.timeout_ms)} ms</dd></div>
          </dl>
          <ObservationFields value={step.observation} />
        </details>
      </section>
    </li>)}</ol>
  </section>;
}

const labels: Record<string, string> = {
  hostname: 'Configured service / TLS name', selected_peer: 'Selected service peer',
  addresses: 'Observed service addresses', peer: 'Service peer', local_address: 'Local socket',
  cname_chains: 'DNS names (original name and aliases)', resolver: 'Resolver provenance',
  questions: 'DNS transport requests', body_capture: 'Body capture',
  content: 'Captured body (untrusted base64)', content_encoding: 'HTTP content encoding',
  redaction_applied: 'Redaction applied', decompressed: 'Decompressed',
  requested: 'Capture requested', truncated: 'Truncated', encoding: 'Byte encoding',
  configuration_sha256: 'Configuration SHA-256', additional_ca_sha256: 'Additional CA SHA-256',
  certificate_sha256: 'Leaf certificate SHA-256', sha256: 'SHA-256',
  namespaces: 'Observed namespaces', net: 'Network namespace', mnt: 'Mount namespace',
  pid: 'PID', uid: 'Real UID', observer_pid: 'Observer PID', start_time_ticks: 'Start-time ticks',
  identity_verified: 'Process identity verified', headers: 'Returned headers (untrusted)',
  sensitive_content_detection: 'Sensitive-content detection',
};
function ObservationFields({ value, depth = 0 }: { value: Record<string, unknown>; depth?: number }) {
  return <dl className="observation-fields">{Object.entries(value).map(([key, item]) => <div key={key}>
    <dt>{labels[key] ?? plainText(humanize(key))}</dt><dd>
      {key === 'questions' && <p className="field-hint">Requests handed to the transport. A closed stream can reject one before transmission.</p>}
      {key === 'cname_chains' && isRecord(item)
        ? <dl className="observation-fields">{Object.entries(item).map(([family, names]) => <div key={family}><dt>{plainText(family.toUpperCase())} chain (original name → aliases)</dt><dd>{Array.isArray(names) && names.every(name => typeof name === 'string') ? plainText(names.join(' → ')) : 'Not recorded'}</dd></div>)}</dl>
        : key === 'content' && typeof item === 'string' ? <pre className="captured-body" tabIndex={0}>{plainText(item)}</pre>
          : <ObservationValue value={item} depth={depth} />}
    </dd>
  </div>)}</dl>;
}
function ObservationValue({ value, depth }: { value: unknown; depth: number }) {
  if (value === null) return <>Not returned</>;
  if (typeof value === 'string') return <>{plainText(value)}</>;
  if (typeof value === 'boolean') return <>{value ? 'Yes' : 'No'}</>;
  if (typeof value === 'number') return <>{String(value)}</>;
  if (isRecord(value) && depth < 5) return <ObservationFields value={value} depth={depth + 1} />;
  if (Array.isArray(value) && value.every(item => typeof item === 'string')) return <>{plainText(value.join(', ')) || 'None'}</>;
  // Preserve all nested values as inert text if a schema's open observation is more complex.
  return <pre tabIndex={0}>{plainText(JSON.stringify(value, null, 2) ?? 'Not returned')}</pre>;
}

export function DiagnosticProvenance({ data, receipt }: { data: DiagnosticData; receipt: Receipt }) {
  return <section className="diagnostic-provenance"><h3>Diagnostic provenance</h3>
    <dl className="observation-fields"><div><dt>Pack</dt><dd>{data.pack.definition.id} · v{data.pack.definition.version}</dd></div><div><dt>Pack SHA-256</dt><dd>{plainText(data.pack.sha256)}</dd></div></dl>
    <ObservationFields value={data.binding.identity} />
    <h3>Trusted target binding</h3><ObservationFields value={{ target: data.binding.target, hostname: data.binding.hostname,
      port: data.binding.port, path: data.binding.path, allowed_ips: data.binding.allowed_ips }} />
    <h3>Observer vantage</h3><ObservationFields value={data.vantage} />
    <h3>Capture and execution choices</h3><ObservationFields value={{ ...data.input,
      concurrency: receipt.effective_limits.concurrency, cancellation_signal: data.cancellation_signal, ...data.redaction }} />
    <h3>Sources</h3>{receipt.sources.length ? receipt.sources.map((source, index) => <ObservationFields key={index} value={source} />) : <p className="field-hint">No sources returned.</p>}
  </section>;
}
