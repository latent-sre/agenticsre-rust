import { humanize, isRecord, plainText } from './model';
import { Badge } from './components';

const labels: Record<string, string> = { id: 'ID', sha256: 'Pack SHA-256', digest: 'Manifest SHA-256', expected_steps: 'Expected checks', retained_receipts: 'Retained receipts' };

function Fields({ value }: { value: Record<string, unknown> }) {
  return <dl className="observation-fields">{Object.entries(value).map(([key, item]) => <div key={key}>
    <dt>{Object.hasOwn(labels, key) ? labels[key] : plainText(humanize(key).replace(/^./u, letter => letter.toUpperCase()))}</dt>
    <dd>{typeof item === 'string' ? plainText(item) : typeof item === 'number' || typeof item === 'boolean' ? String(item) : item === null ? 'Not returned' : <pre className="query-json" tabIndex={0}>{plainText(JSON.stringify(item, null, 2))}</pre>}</dd>
  </div>)}</dl>;
}
function Section({ title, value }: { title: string; value: unknown }) {
  return <section className="investigation-section"><h3>{title}</h3>{isRecord(value) ? <Fields value={value} /> : <p>No details returned. See the errors and coverage.</p>}</section>;
}
export function CollectionResult({ operation, data }: { operation: string; data: Record<string, unknown> }) {
  if (operation !== 'evidence.collect') {
    const packs = operation === 'evidence.packs.list' && Array.isArray(data.packs) ? data.packs : isRecord(data.pack) ? [data.pack] : [];
    return <section aria-label="Evidence pack catalog result"><p>Installed pack metadata · offline inspection. Collection requires its own grant and an existing private store.</p>
      {packs.length === 0 && <p>No installed packs returned. See the errors and coverage.</p>}
      {packs.filter(isRecord).map((pack, index) => <article className="diagnostic-step" key={index}><h3>{typeof pack.id === 'string' ? plainText(pack.id) : 'Installed pack'}</h3><Fields value={pack} /></article>)}
    </section>;
  }
  return <section aria-label="Evidence collection result">
    <p>Historical observations, not a service health assessment. Partial or cancelled collection may retain evidence; read the capture state and each step for omissions.</p>
    {isRecord(data.bundle) ? <><Section title="Saved bundle" value={data.bundle} /><p>Use this ID and digest with Open evidence in a session granted evidence reading. Download result saves this receipt, not the entire bundle.</p></>
      : <p>No sealed bundle reference returned. Do not assume evidence was saved; inspect the errors and capture state.</p>}
    <Section title="Capture state" value={data.capture} />
    <Section title="Resolved observation window" value={data.window} />
    <section className="investigation-section"><h3>Collection steps</h3>{Array.isArray(data.steps) && data.steps.length > 0 ? data.steps.filter(isRecord).map((step, index) => <article className="diagnostic-step" key={index}>
      <header><h4>{step.id === 'metrics' ? 'Metrics' : step.id === 'logs' ? 'Logs' : `Step ${index + 1}`}</h4>{typeof step.status === 'string' && <Badge status={step.status} />}</header>
      {step.omission !== null && step.omission !== undefined && <Fields value={{ omission: step.omission }} />}
      {Array.isArray(step.errors) && step.errors.length > 0 && <Fields value={{ errors: step.errors }} />}
      <details className="step-observation"><summary>Receipt identity and coverage</summary><Fields value={step} /></details>
    </article>) : <p>No steps returned. See the errors and coverage.</p>}</section>
    <details className="step-observation"><summary>Selection, provenance and cancellation</summary>
      <Section title="Context selection" value={data.selection} /><Section title="Installed pack" value={data.pack} />
      <Section title="Context provenance" value={data.provenance} /><Section title="Connection binding" value={data.connection} />
      <Section title="Redaction" value={data.redaction} />
      <pre className="query-json" tabIndex={0}>{plainText(JSON.stringify({ cancellation_signal: data.cancellation_signal, delivery_cancellation_signal: data.delivery_cancellation_signal }, null, 2))}</pre>
    </details>
  </section>;
}
