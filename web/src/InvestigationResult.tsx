import { Badge } from './components';
import { humanize, isRecord, plainText } from './model';

const labels: Record<string, string> = {
  created_at_unix_ms: 'Created at', finalized_at_unix_ms: 'Finalized at', id: 'ID', digest: 'Manifest SHA-256', sha256: 'SHA-256', run_id: 'Run ID',
  observed_at: 'Source observed at', evidence_state: 'Evidence state', untrusted: 'Untrusted source',
  started_at: 'Observation started at', finalized_at: 'Observation finalized at', from: 'Window from', to: 'Window to',
  max_age_days: 'Maximum age (days)', age_seconds: 'Age (seconds)', usable: 'Usable for context binding',
  missing_required: 'Missing required bindings', missing_optional: 'Missing optional context',
  omitted_details: 'Omitted detail items', expected_steps: 'Expected checks', retained_receipts: 'Retained receipts',
  review_status: 'Review status', provenance_changes: 'Provenance changes', source_limitations: 'Source limitations',
  samples_added: 'Samples added', samples_removed: 'Samples removed', series_added: 'Series added', series_removed: 'Series removed',
  entries_added: 'Log entries added', entries_removed: 'Log entries removed', value_delta: 'Value delta',
};
const title = (key: string) => Object.hasOwn(labels, key) ? labels[key] : plainText(humanize(key).replace(/^./u, character => character.toUpperCase()));
// Render schema fields as inert text, including nested labels and returned log lines.
function Value({ value }: { value: unknown }) {
  if (value === null || value === undefined) return <>Not returned</>;
  if (typeof value === 'string') return <>{plainText(value)}</>;
  if (typeof value === 'number') return <>{String(value)}</>;
  if (typeof value === 'boolean') return <>{value ? 'Yes' : 'No'}</>;
  if (Array.isArray(value)) return value.length ? <ul className="investigation-values">{value.map((item: unknown, index) => <li key={index}><Value value={item} /></li>)}</ul> : <>None returned</>;
  if (isRecord(value)) return <Fields value={value} />;
  return <>Not returned</>;
}
function storedTime(value: unknown): unknown {
  if (typeof value !== 'number' || !Number.isFinite(value)) return value;
  const time = new Date(value);
  return Number.isNaN(time.getTime()) ? 'Timestamp outside browser display range (see canonical receipt)' : time.toISOString();
}
function Fields({ value }: { value: Record<string, unknown> }) {
  return Object.keys(value).length ? <dl className="observation-fields">{Object.entries(value).map(([key, item]) => <div key={key}><dt>{title(key)}</dt><dd><Value value={item} /></dd></div>)}</dl> : <p className="field-hint">No details returned.</p>;
}
function Section({ name, value }: { name: string; value: unknown }) {
  return <section className="investigation-section"><h3>{name}</h3><Value value={value} /></section>;
}
function Checks({ value }: { value: unknown }) {
  if (!Array.isArray(value)) return <Section name="Checks" value={value} />;
  return <section className="investigation-section"><h3>Returned checks</h3>{value.length === 0 && <p>No comparable check details returned.</p>}{value.filter(isRecord).map((check, index) => <article className="diagnostic-step" key={index}>
    <header><h3>{check.id === 'metrics' ? 'Metrics' : check.id === 'logs' ? 'Logs' : 'Check'}</h3>{typeof check.state === 'string' && <Badge status={check.state} />}</header>
    <Fields value={Object.fromEntries(Object.entries(check).filter(([key]) => key !== 'id' && key !== 'state' && key !== 'details'))} />
    {Array.isArray(check.details) && check.details.length > 0 && <details className="step-observation"><summary>Returned observation details (untrusted)</summary><Value value={check.details} /></details>}
  </article>)}</section>;
}
function downloadReport(content: string, format: 'markdown' | 'json') {
  const url = URL.createObjectURL(new Blob([content], { type: format === 'markdown' ? 'text/markdown' : 'application/json' }));
  const link = document.createElement('a'); link.href = url;
  link.download = format === 'markdown' ? 'workbench-evidence-report.md' : 'workbench-evidence-report.json';
  try { link.click(); } finally { setTimeout(() => URL.revokeObjectURL(url), 1000); }
}
export function InvestigationResult({ operation, data }: { operation: string; data: Record<string, unknown> }) {
  if (!Object.keys(data).some(key => key !== 'cancellation_signal')) return <p className="empty-output">No historical observations returned. Read the errors and coverage for what was established.</p>;
  return <section className="investigation-result" aria-label="Offline investigation result">
    <p className="form-description">Historical / offline evidence · source values are untrusted. These observations do not assess current service health.</p>
    {typeof data.state === 'string' && <p>Evidence state: <Badge status={data.state} /></p>}
    {operation === 'context.resolve' ? <>
      {isRecord(data.resolution) && data.resolution.usable === false && <p className="inspection-note">Context is unusable for binding. The resolution state, freshness and missing fields below explain why.</p>}
      <Section name="Resolution and freshness" value={data.resolution} />
      <Section name="Service ownership, bindings and dependencies" value={data.record} />
      <Section name="Context source" value={data.source} />
    </> : operation === 'evidence.list' ? <section className="investigation-section"><h3>Saved evidence</h3><p className="field-hint">Copy an ID to Inspect evidence, Compare evidence or Evidence report. List entries describe storage state; inspection verifies the bundle.</p>
      {Array.isArray(data.runs) && data.runs.length === 0 && <p>No saved evidence returned.</p>}
      {Array.isArray(data.runs) && data.runs.filter(isRecord).map((run, index) => <article className="diagnostic-step" key={index}><h3>{typeof run.id === 'string' ? plainText(run.id) : 'Evidence ID unavailable'}</h3>
        <Fields value={Object.fromEntries(Object.entries(run).filter(([key]) => key !== 'id').map(([key, value]) => [key, key === 'created_at_unix_ms' || key === 'finalized_at_unix_ms' ? storedTime(value) : value]))} />
      </article>)}
    </section> : operation === 'evidence.open' ? <>
      <Section name="Verified bundle" value={data.bundle} /><Section name="Original observation window and capture" value={data.observation} />
      <Section name="Source limitations" value={data.source_limitations} /><Checks value={data.checks} />
      <details className="step-observation"><summary>Manifest and analysis provenance</summary><Fields value={{ manifest: data.manifest, analysis: data.analysis, algorithm: data.algorithm, omitted_details: data.omitted_details }} /></details>
    </> : operation === 'evidence.compare' ? <>
      <Section name="Compatibility and reasons" value={data.compatibility} /><Section name="Comparison normalization" value={data.normalization} />
      <Section name="Baseline original observation" value={data.before} /><Section name="Comparison original observation" value={data.after} />
      <Section name="Provenance changes" value={data.provenance_changes} /><Section name="Source limitations" value={data.source_limitations} /><Checks value={data.checks} />
    </> : operation === 'report.create' && isRecord(data.report) ? <>
      <Section name="Report summary" value={{ title: data.report.title, state: data.report.state, generated_at: data.report.generated_at, scope: data.report.scope }} />
      <Section name="Historical sources" value={data.report.sources} /><Section name="Limitations" value={data.report.limitations} />
      <p className="inspection-note">Pending proposals are unapproved, untrusted suggestions. Creating a report grants no approval.</p>
      <Section name="Pending knowledge proposals" value={data.report.knowledge_proposals} />
      <section className="investigation-section"><h3>Report preview (plain text)</h3>{isRecord(data.rendering) && data.rendering.state === 'complete' && typeof data.rendering.content === 'string' ? <>
        <pre className="report-preview" tabIndex={0}>{plainText(data.rendering.content)}</pre>
        {(data.format === 'markdown' || data.format === 'json') && <button className="button secondary" onClick={() => { if (isRecord(data.rendering) && typeof data.rendering.content === 'string') downloadReport(data.rendering.content, data.format === 'json' ? 'json' : 'markdown'); }}>Download report</button>}
      </> : <><p>The report rendering was omitted. The complete canonical receipt contains the retained report fields and disclosed limits.</p><Section name="Rendering limits" value={data.rendering} /></>}</section>
      <details className="step-observation"><summary>Report observations, comparisons and next questions</summary><Fields value={{ observations: data.report.observations, comparisons: data.report.comparisons, next_questions: data.report.next_questions }} /></details>
    </> : <p>No historical result details returned. Read the complete canonical receipt.</p>}
  </section>;
}
