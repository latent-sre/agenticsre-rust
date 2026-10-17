import { humanize, isRecord, plainText } from './model';

const labels: Record<string, string> = { id: 'Definition ID', definition_sha256: 'Definition SHA-256', sha256: 'Definition SHA-256', parameter_schema: 'Parameter schema', expression_structure: 'Fixed expression structure', expected_shape: 'Expected response shape', limitations: 'Blind spots', limits: 'Requested work limits' };
function Fields({ value }: { value: Record<string, unknown> }) {
  return <dl className="observation-fields">{Object.entries(value).map(([key, item]) => <div key={key}>
    <dt>{Object.hasOwn(labels, key) ? labels[key] : plainText(humanize(key).replace(/^./u, char => char.toUpperCase()))}</dt>
    <dd>{item === null ? 'Not applicable' : typeof item === 'string' ? plainText(item) : Array.isArray(item) && item.every(entry => typeof entry === 'string') ? <ul>{item.map((entry: string, index) => <li key={index}>{plainText(entry)}</li>)}</ul> : typeof item === 'number' || typeof item === 'boolean' ? String(item) : <pre className="query-json" tabIndex={0}>{plainText(JSON.stringify(item, null, 2))}</pre>}</dd>
  </div>)}</dl>;
}
function Section({ title, value }: { title: string; value: unknown }) {
  return <section className="investigation-section"><h3>{title}</h3>{isRecord(value) ? <Fields value={value} /> : <p className="field-hint">No details returned. See the errors and coverage.</p>}</section>;
}
export function QueryResult({ operation, data }: { operation: string; data: Record<string, unknown> }) {
  if (operation === 'queries.list' || operation === 'queries.describe') {
    const definitions: unknown[] = operation === 'queries.list' && Array.isArray(data.queries) ? data.queries : isRecord(data.query) ? [data.query] : [];
    return <section aria-label="Saved query catalog result"><p className="form-description">Installed definition metadata · offline inspection. Declared units and expected shapes do not verify backend semantics or service health.</p>
      {definitions.length === 0 && <p>No installed definitions returned. See the errors and coverage.</p>}
      {definitions.filter(isRecord).map((definition, index) => <article className="diagnostic-step" key={index}><h3>{typeof definition.id === 'string' ? plainText(definition.id) : 'Installed query'}</h3><Fields value={definition} /></article>)}
    </section>;
  }
  return <section aria-label="Saved query result"><p className="form-description">Returned observations are untrusted and do not assess service health. Requested points and lines do not prove coverage. Empty results are not zero or an all-clear.</p>
    <Section title="Installed definition identity" value={data.definition} />
    <Section title="Resolved observation window" value={data.window} />
    <Section title="Literal parameters" value={data.parameters} />
    <Section title="Compiled request" value={data.request} />
    <section className="investigation-section"><h3>Returned query observations (untrusted)</h3>{data.response === undefined || data.response === null ? <p>No response retained. Read the errors, transport state and coverage for omissions.</p> : <pre className="query-json" tabIndex={0}>{plainText(JSON.stringify(data.response, null, 2))}</pre>}</section>
    <details className="step-observation"><summary>Binding, transport and redaction</summary><Section title="Connection binding" value={data.binding} /><Section title="Transport" value={data.transport} /><Section title="Redaction" value={data.redaction} /><Section title="Adapter provenance" value={data.adapter} /></details>
  </section>;
}
