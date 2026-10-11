import { useRef, useState } from 'react';
import type { FormEvent } from 'react';
import { Field, Icon } from './components';
import { availableChecks, checkNames, commandInput, diagnosticPack, initialValues, isCheck, makeSubmission, plainText, validateForm } from './model';
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
  const selectedTarget = session.targets.some(item => item.id === target) ? target ?? '' : session.targets[0]?.id ?? '';
  const pack = diagnosticPack(session);
  const [draft, setDraft] = useState(initialValues);
  const [errors, setErrors] = useState<FieldErrors>({});
  const form = useRef<HTMLFormElement>(null);
  const values: FormValues = { ...draft, check: selectedCheck, root: selectedRoot, target: selectedTarget };
  const diagnostics = selectedCheck === 'diagnostics';
  const hasExec = session.operations.some(operation => operation.id === 'process.exec');
  const hasInspect = session.operations.some(operation => operation.id === 'command.inspect');
  const canSubmit = options.length > 0 && !busy && !submitting;
  const setValue = <K extends keyof FormValues>(key: K, value: FormValues[K]) => {
    setDraft(current => ({ ...current, [key]: value }));
    if (errors[key]) setErrors(current => ({ ...current, [key]: undefined }));
  };
  const fieldError = (key: keyof FormValues) => {
    const wire: Partial<Record<keyof FormValues, string>> = { slo: 'inputs.input.slo', days: 'inputs.input.window_days', minutes: 'inputs.input.bad_minutes', root: 'root_id', target: 'target_id', timeout: 'limits.timeout_ms', outputLimit: 'limits.max_output_bytes', captureBody: 'inputs.input.capture_body', onFailure: 'inputs.input.on_failure', concurrency: 'limits.concurrency', path: 'inputs.args', pattern: 'inputs.args' };
    return errors[key] ?? problem?.errors?.find(item => item.field === wire[key])?.detail;
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
    if (first) { form.current?.querySelector<HTMLElement>(`#${first}`)?.focus(); return; }
    const submitter = (event.nativeEvent as SubmitEvent).submitter;
    const operation = diagnostics ? 'diagnostics.run' : values.check === 'budget' ? 'task.run'
      : submitter instanceof HTMLButtonElement && submitter.value === 'inspect' || !hasExec ? 'command.inspect' : 'process.exec';
    onSubmit(makeSubmission(values, operation, crypto.randomUUID()));
  };
  const preview = diagnostics ? `connectivity v1 · ${selectedTarget} · body capture ${values.captureBody ? 'on' : 'off'}` : values.check === 'budget'
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
        {diagnostics ? <Field id="target" label="Diagnostic target" error={fieldError('target')} hint="Only aliases explicitly granted to this session.">
          <select {...inputProps('target')} value={selectedTarget} onChange={event => onSelection(selectedCheck, selectedRoot, event.target.value)}>
            {session.targets.map(item => <option key={item.id} value={item.id}>{plainText(item.label)}</option>)}
          </select>
        </Field> : selectedCheck !== 'budget' && <Field id="root" label="Workspace" error={fieldError('root')}>
          <select {...inputProps('root')} value={selectedRoot} onChange={event => onSelection(selectedCheck, event.target.value, selectedTarget)}>
            {session.roots.map(item => <option key={item.id} value={item.id}>{plainText(item.label)}</option>)}
          </select>
        </Field>}
      </div>
      {!options.includes('diagnostics') && <p className="field-hint">Diagnostics require a new session with explicitly granted targets.</p>}
      {diagnostics ? <>
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
        <Field id="timeout" label="Timeout (seconds)" error={fieldError('timeout')} hint={diagnostics ? 'Up to 60 seconds for the whole pack; each check is bounded to 10 seconds.' : 'Up to 300 seconds. Captured output is limited to 1 MiB.'}>
          <input {...inputProps('timeout')} inputMode="decimal" value={values.timeout} onChange={event => setValue('timeout', event.target.value)} />
        </Field>
        {diagnostics && <Field id="outputLimit" label="Output allowance (bytes)" error={fieldError('outputLimit')} hint="1,024–16,384 bytes. Body capture uses this allowance and the 16 KiB diagnostic ceiling.">
          <input {...inputProps('outputLimit')} inputMode="numeric" value={values.outputLimit} onChange={event => setValue('outputLimit', event.target.value)} />
        </Field>}
      </div></details>
    </div>
    <div className="form-footer"><code className="command-preview" aria-label="Check preview">{plainText(preview)}</code><div className="actions">
      {selectedCheck !== 'budget' && !diagnostics && hasExec && hasInspect && <button className="button secondary" type="submit" value="inspect" disabled={!canSubmit}>Inspect</button>}
      <button className="button primary" type="submit" disabled={!canSubmit}>
        {submitting ? 'Starting…' : diagnostics ? 'Run diagnostics' : selectedCheck === 'budget' ? 'Calculate budget' : !hasExec ? 'Inspect check' : 'Run check'}<Icon name="arrow" size={16} />
      </button>
    </div></div>
  </form>;
}
