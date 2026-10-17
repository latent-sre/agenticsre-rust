import { useRef, useState } from 'react';
import type { FormEvent } from 'react';
import { Field, Icon } from './components';
import { availableChecks, checkNames, commandInput, diagnosticPack, initialValues, investigationOperations, isInvestigation, isCheck, isQuery, queryDefinitions, queryOperations, makeSubmission, plainText, validateForm } from './model';
import type { Check, FieldErrors, FormValues, Problem, Session, Submission } from './model';

type Props = {
  session: Session; check?: Check; root?: string; target?: string; busy: boolean; submitting: boolean;
  problem?: Problem; onSubmit: (request: Submission) => void;
  onSelection: (check: Check, root: string, target: string) => void;
};
export function CheckForm({ session, check, root, target, busy, submitting, problem, onSubmit, onSelection }: Props) {
  const options = availableChecks(session);
  const selectedCheck = check && options.includes(check) ? check : options[0] ?? 'budget';
  const selectedRoot = session.roots.some(item => item.id === root) ? root ?? '' : session.roots[0]?.id ?? '';
  const targets = selectedCheck === 'query-run' ? session.query_targets ?? [] : session.targets;
  const selectedTarget = targets.some(item => item.id === target) ? target ?? '' : targets[0]?.id ?? '';
  const pack = diagnosticPack(session);
  const [draft, setDraft] = useState(initialValues);
  const [errors, setErrors] = useState<FieldErrors>({});
  const form = useRef<HTMLFormElement>(null);
  const values: FormValues = { ...draft, check: selectedCheck, root: selectedRoot, target: selectedTarget };
  const diagnostics = selectedCheck === 'diagnostics';
  const investigation = isInvestigation(selectedCheck);
  const query = isQuery(selectedCheck);
  const hasExec = session.operations.some(operation => operation.id === 'process.exec');
  const hasInspect = session.operations.some(operation => operation.id === 'command.inspect');
  const canSubmit = options.length > 0 && !busy && !submitting;
  const setValue = <K extends keyof FormValues>(key: K, value: FormValues[K]) => {
    setDraft(current => ({ ...current, [key]: value }));
    if (errors[key]) setErrors(current => ({ ...current, [key]: undefined }));
  };
  const fieldError = (key: keyof FormValues) => {
    const wire: Partial<Record<keyof FormValues, string>> = { slo: 'inputs.input.slo', days: 'inputs.input.window_days', minutes: 'inputs.input.bad_minutes', root: 'root_id', target: 'target_id', timeout: 'limits.timeout_ms', outputLimit: 'limits.max_output_bytes', captureBody: 'inputs.input.capture_body', onFailure: 'inputs.input.on_failure', concurrency: 'limits.concurrency', path: 'inputs.args', pattern: 'inputs.args', service: 'inputs.service', environment: 'inputs.environment', team: 'inputs.team', deployment: 'inputs.deployment', bundleId: 'inputs.id', bundlePin: 'inputs.expected_manifest_sha256', beforeId: 'inputs.before.id', beforePin: 'inputs.before.expected_manifest_sha256', afterId: 'inputs.after.id', afterPin: 'inputs.after.expected_manifest_sha256', maxItems: 'limits.max_items', reportTitle: 'inputs.title' };
    const queryWire: Partial<Record<keyof FormValues, string>> = { queryId: 'inputs.id', datasource: 'inputs.datasource', job: 'inputs.parameters.job', definitionPin: 'inputs.expected_definition_sha256', lookback: 'inputs.window.lookback_seconds', windowFrom: 'inputs.window.from', windowTo: 'inputs.window.to' };
    return errors[key] ?? problem?.errors?.find(item => item.field === (queryWire[key] ?? wire[key]))?.detail;
  };
  const inputProps = (key: keyof FormValues) => ({
    id: key, 'aria-invalid': Boolean(fieldError(key)), 'aria-describedby': key === 'root' && !fieldError(key) ? undefined : `${key}-help`,
    onBlur: () => setErrors(current => ({ ...current, [key]: validateForm(values)[key] })),
  });
  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!canSubmit) return;
    const nextErrors = validateForm(values);
    setErrors(nextErrors);
    const first = Object.keys(nextErrors)[0];
    if (first) { const field = form.current?.querySelector<HTMLElement>(`#${first}`); const details = field?.closest('details'); if (details) details.open = true; field?.focus(); return; }
    const submitter = (event.nativeEvent as SubmitEvent).submitter;
    const operation = isQuery(values.check) ? queryOperations[values.check] : isInvestigation(values.check) ? investigationOperations[values.check] : diagnostics ? 'diagnostics.run' : values.check === 'budget' ? 'task.run'
      : submitter instanceof HTMLButtonElement && submitter.value === 'inspect' || !hasExec ? 'command.inspect' : 'process.exec';
    onSubmit(makeSubmission(values, operation, crypto.randomUUID()));
  };
  const preview = query ? selectedCheck === 'query-run' ? `${values.queryId} v1 · ${selectedTarget} · ${values.windowKind === 'relative' ? `${values.lookback} second lookback` : 'absolute window'}` : `${checkNames[selectedCheck]} · installed definitions · offline` : investigation ? `${checkNames[selectedCheck]} · offline · historical sources` : diagnostics ? `connectivity v1 · ${selectedTarget} · body capture ${values.captureBody ? 'on' : 'off'}` : values.check === 'budget'
    ? `${values.slo || '—'}% SLO · ${values.days || '—'} days · ${values.minutes || '—'} bad minutes`
    : (() => { const command = commandInput(values); return `${command.program} ${command.args.map(arg => /^[A-Za-z0-9_./=-]+$/u.test(arg) ? arg : JSON.stringify(arg)).join(' ')}`; })();
  return <form className="card check-form" ref={form} onSubmit={submit} noValidate aria-label="Run a check">
    <div className="card-heading"><div><span className="eyebrow">NEW CHECK</span><h2>Choose what to check</h2></div><Icon name="terminal" /></div>
    <div className="form-fields">
      <div className="form-grid">
        <Field id="check" label="Check">
          <select id="check" value={selectedCheck} onChange={event => { if (isCheck(event.target.value)) { setErrors({}); onSelection(event.target.value, selectedRoot, selectedTarget); } }}>
            {options.map(option => <option key={option} value={option}>{checkNames[option]}</option>)}
          </select>
        </Field>
        {diagnostics || selectedCheck === 'query-run' ? <Field id="target" label={diagnostics ? 'Diagnostic target' : 'Query target'} error={fieldError('target')} hint="Only aliases explicitly granted to this session.">
          <select {...inputProps('target')} value={selectedTarget} onChange={event => onSelection(selectedCheck, selectedRoot, event.target.value)}>
            {targets.map(item => <option key={item.id} value={item.id}>{plainText(item.label)}</option>)}
          </select>
        </Field> : selectedCheck !== 'budget' && !investigation && !query && <Field id="root" label="Workspace" error={fieldError('root')}>
          <select {...inputProps('root')} value={selectedRoot} onChange={event => onSelection(selectedCheck, event.target.value, selectedTarget)}>
            {session.roots.map(item => <option key={item.id} value={item.id}>{plainText(item.label)}</option>)}
          </select>
        </Field>}
      </div>
      {!options.includes('diagnostics') && <p className="field-hint">Diagnostics require a new session with explicitly granted targets.</p>}
      {query ? <>
        <p className="form-description">Installed, versioned queries use one literal job label. Observations do not assess service health. Catalog inspection is offline.</p>
        {selectedCheck === 'queries-list' ? <p className="field-hint">Read the installed definitions, their digests, parameters, limits and blind spots. Select Inspect saved query for one definition or Run saved query to collect observations.</p> : <Field id="queryId" label="Saved query definition" error={fieldError('queryId')} hint="Only these installed definitions are supported. Inspect saved query shows the authoritative definition and digest.">
          <select {...inputProps('queryId')} value={values.queryId} onChange={event => { const id = event.target.value; if (id === 'prometheus.up-by-job' || id === 'loki.logs-by-job') setValue('queryId', id); }}>
            {Object.entries(queryDefinitions).map(([id, label]) => <option key={id} value={id}>{label}</option>)}
          </select>
        </Field>}
        {selectedCheck === 'query-run' && <>
          <div className="form-grid">
            <Field id="datasource" label="Datasource UID" error={fieldError('datasource')} hint="Explicit configured datasource UID; 1–40 letters, digits, underscores or hyphens."><input {...inputProps('datasource')} value={values.datasource} onChange={event => setValue('datasource', event.target.value)} autoComplete="off" spellCheck={false} /></Field>
            <Field id="job" label="Literal job" error={fieldError('job')} hint="Exact label value, 1–128 UTF-8 bytes. Quotes and backslashes remain literal; query macros are unsupported."><input {...inputProps('job')} value={values.job} onChange={event => setValue('job', event.target.value)} autoComplete="off" spellCheck={false} /></Field>
          </div>
          <Field id="windowKind" label="Observation window" hint="At most 24 hours. A relative window resolves once when the server starts the query."><select id="windowKind" value={values.windowKind} onChange={event => { if (event.target.value === 'relative' || event.target.value === 'absolute') setValue('windowKind', event.target.value); }}><option value="relative">Relative lookback</option><option value="absolute">Absolute timestamps</option></select></Field>
          {values.windowKind === 'relative' ? <Field id="lookback" label="Lookback (seconds)" error={fieldError('lookback')} hint="1–86,400 whole seconds."><input {...inputProps('lookback')} inputMode="numeric" value={values.lookback} onChange={event => setValue('lookback', event.target.value)} /></Field>
            : <div className="form-grid">{(['windowFrom', 'windowTo'] as const).map(key => <Field key={key} id={key} label={key === 'windowFrom' ? 'Window start' : 'Window end'} error={fieldError(key)} hint="RFC3339 with timezone, e.g. 2026-10-02T00:00:00.123Z."><input {...inputProps(key)} value={values[key]} onChange={event => setValue(key, event.target.value)} autoComplete="off" spellCheck={false} /></Field>)}</div>}
          <Field id="definitionPin" label="Definition SHA-256 (optional)" error={fieldError('definitionPin')} hint="Copy the 64-character digest from Inspect saved query. No sha256: prefix."><input {...inputProps('definitionPin')} value={values.definitionPin} onChange={event => setValue('definitionPin', event.target.value)} autoComplete="off" spellCheck={false} /></Field>
          <p className="field-hint">Requested points and lines do not prove complete coverage. Empty results are not zero or an all-clear.</p>
        </>}
      </> : investigation ? <>
        <p className="form-description">Resolve offline context or inspect historical, untrusted evidence. These operations make no live service health assessment.</p>
        {selectedCheck === 'context' && <div className="form-grid">{(['service', 'environment', 'team', 'deployment'] as const).map(key => <Field key={key} id={key} label={`${key.slice(0, 1).toUpperCase()}${key.slice(1)}${key === 'team' || key === 'deployment' ? ' (optional)' : ''}`} error={fieldError(key)} hint="Exact context selector; no automatic lookup or substitution.">
          <input {...inputProps(key)} value={values[key]} onChange={event => setValue(key, event.target.value)} autoComplete="off" />
        </Field>)}</div>}
        {selectedCheck === 'evidence-list' && <p className="field-hint">List saved evidence from the launcher-granted store. Copy an evidence ID into Inspect evidence, Compare evidence or Evidence report.</p>}
        {(selectedCheck === 'evidence-open' || selectedCheck === 'evidence-compare') && <div className="form-grid">{(selectedCheck === 'evidence-open' ? [{ id: 'bundleId', pin: 'bundlePin', label: 'Evidence' }] as const : [{ id: 'beforeId', pin: 'beforePin', label: 'Baseline evidence' }, { id: 'afterId', pin: 'afterPin', label: 'Comparison evidence' }] as const).map(reference => <div key={reference.id}>
          <Field id={reference.id} label={`${reference.label} ID`} error={fieldError(reference.id)} hint="Copy the exact saved evidence ID, such as run-1-ab-c.">
            <input {...inputProps(reference.id)} value={values[reference.id]} onChange={event => setValue(reference.id, event.target.value)} autoComplete="off" spellCheck={false} />
          </Field>
          <Field id={reference.pin} label={`${reference.label} SHA-256 (optional)`} error={fieldError(reference.pin)} hint="Exact manifest digest pin; leave blank to verify the stored manifest without a supplied pin.">
            <input {...inputProps(reference.pin)} value={values[reference.pin]} onChange={event => setValue(reference.pin, event.target.value)} autoComplete="off" spellCheck={false} />
          </Field>
        </div>)}</div>}
        {selectedCheck === 'evidence-compare' && <Field id="normalization" label="Comparison normalization" hint="Exact window requires identical observation windows. Window offset compares positions within each original window.">
          <select id="normalization" value={values.normalization} onChange={event => { if (event.target.value === 'exact-window-v1' || event.target.value === 'window-offset-v1') setValue('normalization', event.target.value); }}><option value="exact-window-v1">Exact window</option><option value="window-offset-v1">Window offset</option></select>
        </Field>}
        {selectedCheck === 'evidence-report' && <>
          <div className="form-grid">{([1, 2, 3, 4] as const).map(index => { const id = `reportId${index}` as const; const pin = `reportPin${index}` as const; return <div key={id}>
            <Field id={id} label={`Report evidence ${index} ID${index === 1 ? '' : ' (optional)'}`} error={fieldError(id)} hint="One to four distinct saved evidence IDs."><input {...inputProps(id)} value={values[id]} onChange={event => setValue(id, event.target.value)} autoComplete="off" spellCheck={false} /></Field>
            <Field id={pin} label={`Report evidence ${index} SHA-256 (optional)`} error={fieldError(pin)}><input {...inputProps(pin)} value={values[pin]} onChange={event => setValue(pin, event.target.value)} autoComplete="off" spellCheck={false} /></Field>
          </div>; })}</div>
          <div className="form-grid"><Field id="reportFormat" label="Report format"><select id="reportFormat" value={values.reportFormat} onChange={event => { if (event.target.value === 'markdown' || event.target.value === 'json') setValue('reportFormat', event.target.value); }}><option value="markdown">Markdown</option><option value="json">JSON</option></select></Field>
            <Field id="reportTitle" label="Report title (optional)" error={fieldError('reportTitle')}><input {...inputProps('reportTitle')} value={values.reportTitle} onChange={event => setValue('reportTitle', event.target.value)} /></Field></div>
          <p className="field-hint">Reports are historical handoff documents. Any knowledge proposals remain pending and unapproved.</p>
        </>}
      </> : diagnostics ? <>
        <div className="diagnostic-pack"><h3>Connectivity · version {pack?.definition.version}</h3>
          <p>DNS → TCP → TLS → HTTP, plus an independent local process observation.</p>
          <p>Checks observe the configured target from the server’s vantage. They do not assess service health. Results appear together when the run finishes.</p>
        </div>
        <div className="form-grid">
          <Field id="onFailure" label="On a failed check" error={fieldError('onFailure')} hint="Dependent checks are blocked. A check already running may finish.">
            <select {...inputProps('onFailure')} value={values.onFailure} onChange={event => {
              if (event.target.value === 'stop' || event.target.value === 'continue') setValue('onFailure', event.target.value);
            }}><option value="continue">Continue independent checks</option><option value="stop">Stop admitting checks</option></select>
          </Field>
          <Field id="concurrency" label="Concurrent checks" error={fieldError('concurrency')} hint="At most two checks at once.">
            <select {...inputProps('concurrency')} value={values.concurrency} onChange={event => {
              if (event.target.value === '1' || event.target.value === '2') setValue('concurrency', event.target.value === '1' ? 1 : 2);
            }}><option value="2">2 checks</option><option value="1">1 check</option></select>
          </Field>
        </div>
        <div className="checkbox-row"><label><input id="captureBody" type="checkbox" aria-describedby="captureBody-help" checked={values.captureBody} onChange={event => setValue('captureBody', event.target.checked)} />Capture response body</label></div>
        <p id="captureBody-help" className="field-hint">Off by default. Opt-in body bytes are bounded, untrusted base64, without redaction or decompression.</p>
        {fieldError('captureBody') && <p className="field-error" role="alert">{plainText(fieldError('captureBody') ?? '')}</p>}
      </> : selectedCheck === 'budget' ? <>
        <p className="form-description">Calculate the downtime allowed by your SLO and the budget remaining. This calculation does not assess a live service.</p>
        <div className="form-grid three">
          <Field id="slo" label="Availability SLO (%)" error={fieldError('slo')} hint="Greater than 0, less than 100.">
            <input {...inputProps('slo')} inputMode="decimal" value={values.slo} onChange={event => setValue('slo', event.target.value)} />
          </Field>
          <Field id="days" label="Window (days)" error={fieldError('days')} hint="The period covered by your budget.">
            <input {...inputProps('days')} inputMode="decimal" value={values.days} onChange={event => setValue('days', event.target.value)} />
          </Field>
          <Field id="minutes" label="Bad minutes" error={fieldError('minutes')} hint="Downtime consumed in this window.">
            <input {...inputProps('minutes')} inputMode="decimal" value={values.minutes} onChange={event => setValue('minutes', event.target.value)} />
          </Field>
        </div>
      </> : <>
        {selectedCheck === 'search' && <Field id="pattern" label="Literal text" error={fieldError('pattern')} hint="Matches this text exactly. Shell expressions and regular expressions are not evaluated.">
          <input {...inputProps('pattern')} value={values.pattern} onChange={event => setValue('pattern', event.target.value)} placeholder="Text to find" autoComplete="off" />
        </Field>}
        <div className="form-grid">
          <Field id="path" label="Path (optional)" error={fieldError('path')} hint="One literal path relative to this workspace. Leave blank for the whole workspace.">
            <input {...inputProps('path')} value={values.path} onChange={event => setValue('path', event.target.value)} placeholder="e.g. src" autoComplete="off" spellCheck={false} />
          </Field>
          {selectedCheck === 'log' && <Field id="count" label="Number of commits" error={fieldError('count')} hint="1–100 commits.">
            <input {...inputProps('count')} inputMode="numeric" value={values.count} onChange={event => setValue('count', event.target.value)} />
          </Field>}
          {selectedCheck === 'diff' && <Field id="diff-format" label="Diff format">
            <select id="diff-format" value={values.diffFormat} onChange={event => {
              const value = event.target.value;
              if (value === 'patch' || value === 'stat' || value === 'name-status') setValue('diffFormat', value);
            }}><option value="patch">Full patch</option><option value="stat">Change summary</option><option value="name-status">File names and status</option></select>
          </Field>}
        </div>
        {selectedCheck === 'status' && <p className="field-hint">Shows tracked changes. Untracked files and submodule changes are outside this check.</p>}
        <div className="checkbox-row">
          {selectedCheck === 'diff' && <label><input type="checkbox" checked={values.cached} onChange={event => setValue('cached', event.target.checked)} />Staged changes</label>}
          {(selectedCheck === 'files' || selectedCheck === 'search') && <label><input type="checkbox" checked={values.hidden} onChange={event => setValue('hidden', event.target.checked)} />Include hidden files (excluding .git)</label>}
          {selectedCheck === 'search' && <label><input type="checkbox" checked={values.ignoreCase} onChange={event => setValue('ignoreCase', event.target.checked)} />Ignore case</label>}
        </div>
      </>}
      <details className="advanced"><summary>Run limits</summary><div className="advanced-fields">
        <Field id="timeout" label="Timeout (seconds)" error={fieldError('timeout')} hint={diagnostics ? 'Up to 60 seconds for the whole pack; each check is bounded to 10 seconds.' : investigation ? 'Up to 300 seconds for the offline operation. Set its output allowance below.' : 'Up to 300 seconds. Captured output is limited to 1 MiB.'}>
          <input {...inputProps('timeout')} inputMode="decimal" value={values.timeout} onChange={event => setValue('timeout', event.target.value)} />
        </Field>
        {(diagnostics || investigation || query) && <Field id="outputLimit" label="Output allowance (bytes)" error={fieldError('outputLimit')} hint={diagnostics ? "1,024–16,384 bytes. Body capture uses this allowance and the 16 KiB diagnostic ceiling." : "1,024–1,048,576 bytes. Large results may be partial; canonical receipt retains the disclosed bounds."}>
          <input {...inputProps('outputLimit')} inputMode="numeric" value={values.outputLimit} onChange={event => setValue('outputLimit', event.target.value)} />
        </Field>}
      {(selectedCheck === 'evidence-open' || selectedCheck === 'evidence-compare') && <Field id="maxItems" label="Maximum detail items" error={fieldError('maxItems')} hint="1–1,000. Omitted details are disclosed in the result."><input {...inputProps('maxItems')} inputMode="numeric" value={values.maxItems} onChange={event => setValue('maxItems', event.target.value)} /></Field>}
      </div></details>
    </div>
    <div className="form-footer"><code className="command-preview" aria-label="Check preview">{plainText(preview)}</code><div className="actions">
      {selectedCheck !== 'budget' && !diagnostics && !investigation && !query && hasExec && hasInspect && <button className="button secondary" type="submit" value="inspect" disabled={!canSubmit}>Inspect</button>}
      <button className="button primary" type="submit" disabled={!canSubmit}>
        {submitting ? 'Starting…' : query ? selectedCheck === 'query-run' ? 'Run query' : 'Inspect catalog' : diagnostics ? 'Run diagnostics' : selectedCheck === 'budget' ? 'Calculate budget' : investigation ? 'Run check' : !hasExec ? 'Inspect check' : 'Run check'}<Icon name="arrow" size={16} />
      </button>
    </div></div>
  </form>;
}
