import { Field } from './components';
import type { FormValues } from './model';

type Props = {
  values: FormValues;
  setValue: <K extends keyof FormValues>(key: K, value: FormValues[K]) => void;
  error: (key: keyof FormValues) => string | undefined;
  inputProps: (key: keyof FormValues) => { id: keyof FormValues; 'aria-invalid': boolean; 'aria-describedby': string | undefined; onBlur: () => void };
};

export function CollectionFields({ values, setValue, error, inputProps }: Props) {
  return <>
    <p className="form-description">Collects the installed metrics and logs pack and saves its receipts to the private evidence store. This performs network observations and local writes.</p>
    <div className="form-grid">{(['service', 'environment', 'team', 'deployment'] as const).map(key => <Field key={key} id={key} label={`${key.slice(0, 1).toUpperCase()}${key.slice(1)}${key === 'team' || key === 'deployment' ? ' (optional)' : ''}`} error={error(key)} hint="Exact selectors from the configured service context.">
      <input {...inputProps(key)} value={values[key]} onChange={event => setValue(key, event.target.value)} autoComplete="off" />
    </Field>)}</div>
    <p className="field-hint">Service queries · version 1. Both observations use one time window.</p>
    <div className="form-grid">{([
      ['metricsDatasource', 'Metrics datasource UID'], ['metricsJob', 'Metrics job'],
      ['logsDatasource', 'Logs datasource UID'], ['logsJob', 'Logs job'],
    ] as const).map(([key, label]) => <Field key={key} id={key} label={label} error={error(key)} hint={key.endsWith('Datasource') ? 'Configured datasource UID, not a URL.' : 'Literal job label; no query macros.'}>
      <input {...inputProps(key)} value={values[key]} onChange={event => setValue(key, event.target.value)} autoComplete="off" spellCheck={false} />
    </Field>)}</div>
    <Field id="windowKind" label="Observation window"><select id="windowKind" value={values.windowKind} onChange={event => { if (event.target.value === 'relative' || event.target.value === 'absolute') setValue('windowKind', event.target.value); }}>
      <option value="relative">Relative lookback</option><option value="absolute">Absolute timestamps</option>
    </select></Field>
    {values.windowKind === 'relative' ? <Field id="lookback" label="Lookback (seconds)" error={error('lookback')} hint="1–86,400 whole seconds.">
      <input {...inputProps('lookback')} inputMode="numeric" value={values.lookback} onChange={event => setValue('lookback', event.target.value)} />
    </Field> : <div className="form-grid">{(['windowFrom', 'windowTo'] as const).map(key => <Field key={key} id={key} label={key === 'windowFrom' ? 'Window start' : 'Window end'} error={error(key)} hint="RFC3339 timestamp with timezone; at most 24 hours apart.">
      <input {...inputProps(key)} value={values[key]} onChange={event => setValue(key, event.target.value)} autoComplete="off" spellCheck={false} />
    </Field>)}</div>}
    <Field id="onFailure" label="On a failed check" hint="A failed observation remains explicit in the saved evidence.">
      <select id="onFailure" value={values.onFailure} onChange={event => { if (event.target.value === 'stop' || event.target.value === 'continue') setValue('onFailure', event.target.value); }}>
        <option value="continue">Continue remaining checks</option><option value="stop">Stop admitting checks</option>
      </select>
    </Field>
    <Field id="packPin" label="Pack SHA-256 (optional)" error={error('packPin')} hint="Copy the 64-character digest from Inspect evidence pack; no sha256: prefix.">
      <input {...inputProps('packPin')} value={values.packPin} onChange={event => setValue('packPin', event.target.value)} autoComplete="off" spellCheck={false} />
    </Field>
    <p className="field-hint">Saved receipts do not prove complete telemetry coverage or service health. Cancelling may leave partial evidence.</p>
  </>;
}
