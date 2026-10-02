import { useEffect, useRef } from 'react';
import { Link } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import type { Api } from './api';
import { ApiError, errorMessage, requestSignal, responseData } from './api';
import { Badge, Icon, Notice } from './components';
import { budgetStatus, formatBytes, formatNumber, humanize, plainText } from './model';
import type { Receipt, Run, Session } from './model';

function download(originalText: string, id: string) {
  const url = URL.createObjectURL(new Blob([originalText], { type: 'application/json' }));
  const link = document.createElement('a');
  link.href = url; link.download = `workbench-${id.replace(/[^A-Za-z0-9_-]/gu, '_')}.json`;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

export function Result({ api, run, session, tab, onCancel, cancelling }: {
  api: Api; run: Run; session: Session; tab: 'output' | 'details'; onCancel: (id: string) => void; cancelling: boolean;
}) {
  const query = useQuery({
    queryKey: ['receipt', run.id],
    queryFn: async ({ signal }) => {
      const originalText = responseData(await api.GET('/api/v1/runs/{id}/receipt', {
        params: { path: { id: run.id } }, signal: requestSignal(signal), parseAs: 'text',
      }));
      // Parsing serves the readable summary only. Re-encoding would round u64 identities.
      return { originalText, receipt: JSON.parse(originalText) as Receipt };
    },
    enabled: run.state === 'terminal', staleTime: Infinity, gcTime: 0, retry: false,
    refetchOnWindowFocus: false,
  });
  const receipt = query.data?.receipt;
  const originalText = query.data?.originalText;
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => { heading.current?.focus({ preventScroll: true }); }, [run.id]);
  const status = receipt?.execution.status ?? (run.state === 'terminal' ? run.execution_status ?? 'unknown' : run.state);
  const budget = receipt ? budgetStatus(receipt) : null;
  return <div className="result-layout">
    <span className="sr-only" role="status" aria-live="polite">{plainText(run.check_label)}: {humanize(status)}</span>
    <div className="result-column">
      <div className="run-spine"><span />{run.state === 'terminal' ? 'RESULT' : 'CHECK IN PROGRESS'}</div>
      <section className="card result-card" aria-label="Check result">
        <header className="result-heading"><div><h2 ref={heading} tabIndex={-1}>{plainText(run.check_label)}</h2><p>{new Date(run.submitted_at).toLocaleString()}</p></div><Badge status={status} /></header>
        {run.state !== 'terminal' ? <div className="running-state" role="status"><span className="activity-mark" />
          <h3>{run.state === 'cancelling' ? 'Stopping the check…' : 'Check running'}</h3>
          <p>{run.state === 'cancelling' ? 'Waiting for the final result and cleanup confirmation.' : 'The result will appear when the check finishes. You can leave this page and return from history.'}</p>
          <button className="button secondary" disabled={cancelling || run.state === 'cancelling'} onClick={() => onCancel(run.id)}>{cancelling || run.state === 'cancelling' ? 'Cancellation requested' : 'Cancel check'}</button>
        </div> : query.isPending ? <div className="loading-state" role="status">Loading the result…</div>
          : query.error ? <div className="result-padding"><Notice title={query.error instanceof ApiError && query.error.status === 410 ? 'This result is no longer retained' : 'Result unavailable'} danger
            action={<button className="button secondary" onClick={() => { query.refetch().catch(() => undefined); }}>Try again</button>}>
            <p>{query.error instanceof ApiError && query.error.status === 410 ? 'It was cleared or evicted from this session. Start a new check to get a new result.' : errorMessage(query.error)}</p>
          </Notice></div>
            : receipt && originalText !== undefined && <>
              <nav className="result-tabs" aria-label="Result view">
                <Link to="/checks" search={previous => ({ ...previous, run: run.id, tab: 'output' })} className={tab === 'output' ? 'selected' : ''} aria-current={tab === 'output' ? 'page' : undefined}>Output</Link>
                <Link to="/checks" search={previous => ({ ...previous, run: run.id, tab: 'details' })} className={tab === 'details' ? 'selected' : ''} aria-current={tab === 'details' ? 'page' : undefined}>Details</Link>
              </nav>
              <div className="result-padding">
                {receipt.errors.length > 0 && <div className="result-errors">{receipt.errors.map((error, index) => <Notice key={index} title={humanize(error.code)} danger><p>{plainText(error.message)}</p>{error.next_step && <p>{plainText(error.next_step)}</p>}</Notice>)}</div>}
                {tab === 'details' ? <ReceiptDetails receipt={receipt} originalText={originalText} /> : <>
                  {budget && <div className="budget-result"><div className="budget-verdict"><span className="eyebrow">TIME BUDGET</span><h3>{budget.state === 'ok' ? 'Budget remaining' : budget.state === 'exhausted' ? 'Budget exhausted' : 'Over budget'}</h3><p>This is an arithmetic result from the values you supplied.</p></div>
                    <dl className="budget-metrics"><div><dt>Allowed downtime</dt><dd>{formatNumber(budget.budget)} <small>min</small></dd></div><div><dt>Consumed</dt><dd>{formatNumber(budget.consumed)} <small>min</small></dd></div><div><dt>Remaining</dt><dd>{formatNumber(budget.remaining)} <small>min</small></dd></div></dl><p className="budget-percent">{formatNumber(budget.percent)}% of this window’s budget consumed.</p>
                  </div>}
                  {receipt.operation === 'command.inspect' && <p className="inspection-note">Inspection only. The tool was not executed. Open Details for the admitted command and requested isolation controls.</p>}
                  {receipt.output.stdout && <div className="output-block"><h3>Standard output</h3><pre tabIndex={0}>{plainText(receipt.output.stdout)}</pre></div>}
                  {receipt.output.stderr && <div className="output-block stderr"><h3>Standard error</h3><pre tabIndex={0}>{plainText(receipt.output.stderr)}</pre></div>}
                  {!receipt.output.stdout && !receipt.output.stderr && !budget && <p className="empty-output">No captured output. The execution status and coverage below describe what was established.</p>}
                  {(receipt.output.stdout_truncated || receipt.output.stderr_truncated) && <Notice title="Output was truncated" danger><p>The displayed output is incomplete because a capture or result limit was reached.</p></Notice>}
                </>}
              </div>
              <footer className="result-footer"><span>Exit {receipt.execution.child_exit_code ?? '—'}</span><span>{formatNumber(receipt.duration_ms)} ms</span><span>Coverage: {humanize(receipt.coverage.state)}</span></footer>
            </>}
      </section>
    </div>
    <aside className="card run-details" aria-label="Run details"><span className="eyebrow">RUN DETAILS</span>
      <dl><div><dt>Result</dt><dd>{humanize(status)}</dd></div><div><dt>Workspace</dt><dd>{run.root_id ? plainText(session.roots.find(root => root.id === run.root_id)?.label ?? run.root_id) : 'Numerical input'}</dd></div>
        {receipt && <><div><dt>Duration</dt><dd>{formatNumber(receipt.duration_ms)} ms</dd></div><div><dt>Exit code</dt><dd>{receipt.execution.child_exit_code ?? '—'}</dd></div><div><dt>Captured output</dt><dd>{formatBytes(new TextEncoder().encode(receipt.output.stdout + receipt.output.stderr).length)}</dd></div><div><dt>Assessment</dt><dd>{humanize(receipt.assessment)}</dd></div></>}
      </dl>
      {receipt && originalText !== undefined && <><div className="coverage-note"><h3>Coverage</h3><p>{plainText(receipt.coverage.scope)}</p><p>State: {humanize(receipt.coverage.state)}. A completed check is not a service health assessment.</p></div>
        <button className="button secondary full-width" onClick={() => download(originalText, run.id)}><Icon name="download" size={16} />Download result</button></>}
      <p className="small-note">Stored in this server session only.</p>
    </aside>
  </div>;
}

function ReceiptDetails({ receipt, originalText }: { receipt: Receipt; originalText: string }) {
  return <div className="receipt-details"><h3>Scope and limitations</h3><p>{plainText(receipt.coverage.scope)}</p>
    <ul>{receipt.coverage.limitations.map((item, index) => <li key={index}>{plainText(item)}</li>)}</ul>
    <dl className="detail-pairs"><div><dt>Execution</dt><dd>{humanize(receipt.execution.status)}</dd></div><div><dt>Effect outcome</dt><dd>{humanize(receipt.effect_outcome)}</dd></div><div><dt>Assessment</dt><dd>{humanize(receipt.assessment)}</dd></div><div><dt>Output encoding</dt><dd>{receipt.output.encoding}</dd></div><div><dt>Timeout</dt><dd>{receipt.effective_limits.timeout_ms} ms</dd></div><div><dt>Output limit</dt><dd>{formatBytes(receipt.effective_limits.max_output_bytes)}</dd></div></dl>
    <details className="raw-details"><summary>Complete result JSON</summary><pre tabIndex={0}>{plainText(originalText)}</pre></details>
  </div>;
}
