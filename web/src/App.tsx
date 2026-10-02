import { useEffect, useRef, useState } from 'react';
import { Link, useNavigate, useRouterState, useSearch } from '@tanstack/react-router';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { admissionUnknown, ApiError, errorMessage, requestSignal, responseData } from './api';
import type { Api } from './api';
import { CheckForm } from './CheckForm';
import { Badge, Icon, Notice } from './components';
import { availableChecks, plainText } from './model';
import type { Run, Session, Submission, Theme } from './model';
import { Result } from './Result';

export function App({ api, initialTheme }: { api: Api | null; initialTheme: Theme }) {
  const [theme, setTheme] = useState(initialTheme);
  const [pendingSubmission, setPendingSubmission] = useState<Submission | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [clearConfirm, setClearConfirm] = useState(false);
  const [announcement, setAnnouncement] = useState('');
  const drawer = useRef<HTMLDialogElement>(null);
  const clearDialog = useRef<HTMLDialogElement>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  const navigate = useNavigate();
  const path = useRouterState({ select: state => state.location.pathname });
  const search = useSearch({ strict: false });
  const historyPage = path === '/history';
  const client = useQueryClient();
  const sessionQuery = useQuery({ queryKey: ['session'], enabled: api !== null,
    queryFn: async ({ signal }) => { if (!api) throw new Error('Missing launch token'); return responseData(await api.GET('/api/v1/session', { signal: requestSignal(signal) })); },
    staleTime: Infinity, retry: false,
  });
  const session = sessionQuery.data;
  const runsQuery = useQuery({ queryKey: ['runs'], enabled: api !== null && session !== undefined,
    queryFn: async ({ signal }) => { if (!api) throw new Error('Missing launch token'); return responseData(await api.GET('/api/v1/runs', { params: { query: { limit: 50, offset: 0 } }, signal: requestSignal(signal) })); },
    refetchInterval: query => query.state.data?.runs.some(run => run.state !== 'terminal') ? 500 : false,
    refetchIntervalInBackground: true, retry: false,
  });
  const runs = runsQuery.data?.runs ?? [];
  const selectedInList = runs.find(run => run.id === search.run);
  const runQuery = useQuery({ queryKey: ['run', search.run], enabled: Boolean(api && session && search.run && !selectedInList),
    queryFn: async ({ signal }) => {
      if (!api || !search.run) throw new Error('Missing selected run');
      return responseData(await api.GET('/api/v1/runs/{id}', { params: { path: { id: search.run } }, signal: requestSignal(signal) }));
    }, retry: false, refetchInterval: query => query.state.data && query.state.data.state !== 'terminal' ? 500 : false,
  });
  const selected = selectedInList ?? (search.run ? runQuery.data : undefined);
  const active = runs.find(run => run.state !== 'terminal');
  const refreshRuns = () => client.invalidateQueries({ queryKey: ['runs'] });
  const rememberRun = async (run: Run) => {
    client.setQueryData(['run', run.id], run);
    await Promise.all([refreshRuns(), navigate({ to: '/checks', search: previous => ({ ...previous, run: run.id, tab: 'output' }) })]);
  };
  const submission = useMutation({ mutationFn: async (request: Submission) => {
    if (!api) throw new Error('Missing launch token');
    return responseData(await api.POST('/api/v1/runs', { body: request, signal: requestSignal() }));
  }, retry: false,
  onSuccess: async run => { setPendingSubmission(null); setActionError(null); setAnnouncement('Check admitted.'); await rememberRun(run); },
  onError: async error => { if (!admissionUnknown(error)) setPendingSubmission(null); setActionError(errorMessage(error)); await refreshRuns(); },
  });
  const cancellation = useMutation({ mutationFn: async (id: string) => {
    if (!api) throw new Error('Missing launch token');
    return responseData(await api.POST('/api/v1/runs/{id}/cancel', { params: { path: { id } }, body: {}, signal: requestSignal() }));
  }, retry: false,
  onSuccess: async run => { setActionError(null); setAnnouncement(run.state === 'terminal' ? 'The check has already finished.' : 'Cancellation requested. Waiting for the final result.'); client.setQueryData(['run', run.id], run); await refreshRuns(); },
  onError: async error => { setActionError(errorMessage(error)); await refreshRuns(); },
  });
  const clear = useMutation({ mutationFn: async () => {
    if (!api) throw new Error('Missing launch token');
    return responseData(await api.DELETE('/api/v1/runs', { body: {}, signal: requestSignal() }));
  }, retry: false,
  onSuccess: async result => {
    setClearConfirm(false); setActionError(null); setAnnouncement(`${result.removed} results cleared. Active work was preserved.`);
    client.removeQueries({ queryKey: ['receipt'] }); client.removeQueries({ queryKey: ['run'] });
    await Promise.all([refreshRuns(), navigate({ to: '/history', search: previous => ({ ...previous, run: undefined }) })]);
  }, onError: error => { setActionError(errorMessage(error)); setClearConfirm(false); },
  });
  useEffect(() => {
    const media = window.matchMedia('(prefers-color-scheme: dark)');
    const apply = () => { document.documentElement.dataset.theme = theme === 'system' ? media.matches ? 'dark' : 'light' : theme; };
    apply(); media.addEventListener('change', apply);
    try { localStorage.setItem('agenticsre.theme', theme); } catch { /* Preference persistence is optional. */ }
    return () => media.removeEventListener('change', apply);
  }, [theme]);
  useEffect(() => { heading.current?.focus({ preventScroll: true }); drawer.current?.close(); document.title = `${path === '/history' ? 'Run history' : 'Workbench'} · AgenticSRE`; }, [path]);
  useEffect(() => { if (clearConfirm) clearDialog.current?.showModal(); else clearDialog.current?.close(); }, [clearConfirm]);
  const select = (run: Run) => { navigate({ to: '/checks', search: previous => ({ ...previous, run: run.id, tab: 'output' }) }).catch(() => setActionError('Could not open this result.')); drawer.current?.close(); };
  const reload = () => { Promise.all([sessionQuery.refetch(), runsQuery.refetch()]).catch(() => setActionError('Cannot reach the workbench.')); };
  const unauthorized = [sessionQuery.error, runsQuery.error, runQuery.error, submission.error, cancellation.error, clear.error].some(error => error instanceof ApiError && error.status === 401);
  const sidebar = <>
    <div className="brand"><span className="brand-mark"><Icon name="terminal" size={20} /></span><div>AgenticSRE<span>LOCAL WORKBENCH</span></div></div>
    <nav className="primary-nav" aria-label="Main navigation">
      <Link to="/checks" search={previous => previous} className={!historyPage ? 'active' : ''} aria-current={!historyPage ? 'page' : undefined}><Icon name="terminal" />Workbench</Link>
      <Link to="/history" search={previous => ({ ...previous, run: undefined })} className={historyPage ? 'active' : ''} aria-current={historyPage ? 'page' : undefined}><Icon name="history" />Run history{runs.length > 0 && <span className="nav-count">{runs.length}</span>}</Link>
    </nav>
    <div className="rail-section"><span className="eyebrow">THIS SESSION</span><p className="rail-note">{session ? `${session.roots.length} granted workspace${session.roots.length === 1 ? '' : 's'}` : 'Local browser session'}</p><p className="small-note">Checks and results stay in this server session.</p></div>
    {runs[0] && <div className="rail-section"><span className="eyebrow">LATEST CHECK</span><button className="recent-run" onClick={() => { if (runs[0]) select(runs[0]); }}><span>{plainText(runs[0].check_label)}</span><Badge status={runs[0].execution_status ?? runs[0].state} /></button></div>}
    <div className="rail-bottom"><div className="theme-picker" role="group" aria-label="Appearance">{(['light', 'dark', 'system'] as const).map(choice => <button key={choice} type="button" className={theme === choice ? 'selected' : ''} aria-pressed={theme === choice} onClick={() => setTheme(choice)}>{choice.slice(0, 1).toUpperCase() + choice.slice(1)}</button>)}</div>
      <div className="session-indicator"><span className={`status-dot ${session && !runsQuery.isError ? 'connected' : ''}`} />{api && !unauthorized ? 'Local session' : 'Launch link required'}</div></div>
  </>;
  return <div className="app-shell">
    <a href="#main" className="skip-link">Skip to content</a>
    <aside className="sidebar">{sidebar}</aside>
    <dialog className="nav-drawer" ref={drawer} aria-label="Navigation"><button className="icon-button drawer-close" aria-label="Close navigation" onClick={() => drawer.current?.close()}><Icon name="close" /></button>{sidebar}</dialog>
    <div className="main-shell"><header className="topbar"><button className="icon-button mobile-menu" aria-label="Open navigation" onClick={() => drawer.current?.showModal()}><Icon name="menu" /></button><div className="breadcrumb">Workbench <span>/</span> {historyPage ? 'Run history' : 'Checks'}</div><span className="topbar-note">Session-only results</span></header>
      <main id="main" tabIndex={-1}><header className="page-heading"><div><span className="eyebrow">YOUR LOCAL WORKBENCH</span><h1 ref={heading} tabIndex={-1}>{historyPage ? 'Run history' : 'Workspace checks'}</h1><p>{historyPage ? 'Reopen a result from this server session.' : 'Run a granted check. Read what actually happened.'}</p></div><span className="session-note">No persistent history</span></header>
        <div role="status" aria-live="polite" className="sr-only">{announcement}</div>
        {!api || unauthorized ? <Notice title="Reopen the workbench launch link"><p>The link printed by the local server grants access to this session. Its token is removed from the address bar after opening.</p><p>Reloading this page does not retain access. Your results stay with the running server.</p></Notice>
          : sessionQuery.isPending ? <div className="loading-state" role="status">Loading granted checks…</div>
            : !session ? <Notice title="Workbench unavailable" danger action={<button className="button secondary" onClick={reload}>Try again</button>}><p>{errorMessage(sessionQuery.error)}</p></Notice>
              : <>
                {runsQuery.isError && <Notice title="Connection interrupted" danger action={<button className="button secondary" onClick={reload}>Reconnect</button>}><p>{errorMessage(runsQuery.error)} Previously loaded summaries may be out of date.</p></Notice>}
                {pendingSubmission && !submission.isPending ? <Notice title="The submission outcome is unknown" danger action={<button className="button secondary" onClick={() => submission.mutate(pendingSubmission)}>Reconcile submission</button>}><p>The server may already have started this check. Reconcile to recover its original result without starting a second check.</p></Notice>
                  : actionError && <Notice title="Action could not be completed" danger><p>{plainText(actionError)}</p>{submission.error instanceof ApiError && submission.error.problem?.errors?.map(error => <p key={error.field}>{plainText(error.field)}: {plainText(error.detail)}</p>)}</Notice>}
                {historyPage ? <History runs={runs} loading={runsQuery.isPending} evicted={runsQuery.data?.evicted_count ?? 0} session={session} onSelect={select} onClear={() => setClearConfirm(true)} />
                  : <>
                    {active && active.id !== selected?.id && <Notice title="A check is running" action={<button className="button secondary" onClick={() => select(active)}>View active check</button>}><p>{plainText(active.check_label)}. Wait for it to finish or cancel it before starting another.</p></Notice>}
                    {availableChecks(session).length === 0 ? <Notice title="No checks are granted"><p>Start a new workbench session with the checks and workspaces you want to allow. This session cannot start work.</p></Notice>
                      : <CheckForm session={session} check={search.check} root={search.root} busy={Boolean(active || pendingSubmission)} submitting={submission.isPending}
                        problem={submission.error instanceof ApiError ? submission.error.problem : undefined}
                        onSubmit={request => { setPendingSubmission(request); setActionError(null); submission.mutate(request); }}
                        onSelection={(check, root) => { navigate({ to: '/checks', search: previous => ({ ...previous, check, root }) }).catch(() => setActionError('Could not update the selected check.')); }} />}
                    {selected ? <Result key={selected.id} api={api} run={selected} session={session} tab={search.tab ?? 'output'} onCancel={id => cancellation.mutate(id)} cancelling={cancellation.isPending} />
                      : search.run && runQuery.isPending ? <div className="loading-state" role="status">Loading check…</div>
                        : search.run && runQuery.isError ? <Notice title="Check unavailable" danger><p>{errorMessage(runQuery.error)}</p></Notice>
                          : <div className="first-result"><div className="empty-glyph"><Icon name="terminal" size={24} /></div><h2>Your next result starts here</h2><p>Choose a check above. Its output, status, and limitations will appear together.</p></div>}
                  </>}
              </>}
      </main>
    </div>
    <dialog ref={clearDialog} className="confirmation" aria-labelledby="clear-title" onCancel={() => setClearConfirm(false)}><h2 id="clear-title">Clear completed results?</h2><p>This forgets the retained results for this session. A running check will continue.</p><div className="actions"><button className="button secondary" onClick={() => setClearConfirm(false)}>Keep results</button><button className="button primary" onClick={() => clear.mutate()} disabled={clear.isPending}>{clear.isPending ? 'Clearing…' : 'Clear completed results'}</button></div></dialog>
  </div>;
}

function History({ runs, loading, evicted, session, onSelect, onClear }: { runs: Run[]; loading: boolean; evicted: number; session: Session; onSelect: (run: Run) => void; onClear: () => void }) {
  return <section className="card history-card" aria-label="Session history"><div className="card-heading"><div><h2>Recent checks</h2><p>Up to 50 completed results, within the session’s memory limit.</p></div><button className="button secondary" disabled={!runs.some(run => run.state === 'terminal')} onClick={onClear}>Clear history</button></div>
    {loading ? <div className="loading-state" role="status">Loading history…</div> : runs.length === 0 ? <div className="first-result"><Icon name="history" size={28} /><h3>No checks in this session</h3><p>Run a check from the workbench to see its result here.</p><Link to="/checks" search={{}} className="button secondary">Go to workbench</Link></div>
      : <ul className="history-list">{runs.map(run => <li key={run.id}><button className="history-row" onClick={() => onSelect(run)}><Icon name={run.operation === 'task.run' ? 'check' : 'terminal'} /><div className="history-title"><strong>{plainText(run.check_label)}</strong><span>{run.root_id ? plainText(session.roots.find(root => root.id === run.root_id)?.label ?? run.root_id) : 'Numerical input'} · {new Date(run.submitted_at).toLocaleString()}</span></div><Badge status={run.execution_status ?? run.state} /><Icon name="arrow" size={16} /></button></li>)}</ul>}
    <footer className="history-footer">Closing the server clears this history.{evicted > 0 && <span> {evicted} older result{evicted === 1 ? ' was' : 's were'} evicted.</span>}</footer>
  </section>;
}
