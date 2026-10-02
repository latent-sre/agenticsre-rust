import type { ReactNode } from 'react';
import { humanize, plainText } from './model';

export function Icon({ name, size = 18 }: { name: 'terminal' | 'history' | 'arrow' | 'menu' | 'close' | 'download' | 'check' | 'sun' | 'moon'; size?: number }) {
  const paths = {
    terminal: <><path d="m5 6 5 6-5 6M13 18h6" /></>,
    history: <><path d="M3 11a9 9 0 1 1 2.7 7M3 4v7h7M12 7v5l3 2" /></>,
    arrow: <><path d="M5 12h14m-6-6 6 6-6 6" /></>,
    menu: <path d="M4 6h16M4 12h16M4 18h16" />,
    close: <path d="m6 6 12 12M6 18 18 6" />,
    download: <path d="M12 3v12m-5-5 5 5 5-5M4 16v5h16v-5" />,
    check: <path d="m5 12 4 4 10-10" />,
    sun: <><circle cx="12" cy="12" r="4" /><path d="M12 2v2m0 16v2M2 12h2m16 0h2M5 5l1.5 1.5m11 11L19 19M5 19l1.5-1.5m11-11L19 5" /></>,
    moon: <path d="M20 15A9 9 0 0 1 9 4 9 9 0 1 0 20 15Z" />,
  };
  return <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{paths[name]}</svg>;
}

export function Badge({ status }: { status: string }) {
  const tone = status === 'succeeded' || status === 'complete' ? 'good'
    : ['running', 'cancelling'].includes(status) ? 'accent'
      : ['failed', 'denied', 'unsupported', 'partial', 'timed_out', 'unknown'].includes(status) ? 'warn' : 'neutral';
  return <span className={`badge ${tone}`}><span className="status-dot" />{humanize(status)}</span>;
}

export function Notice({ title, children, action, danger = false }: { title: string; children?: ReactNode; action?: ReactNode; danger?: boolean }) {
  return <section className={`notice ${danger ? 'warning' : ''}`} role={danger ? 'alert' : undefined}>
    <div><h2>{title}</h2>{children && <div className="notice-copy">{children}</div>}</div>{action}
  </section>;
}

export function Field({ id, label, hint, error, children }: { id: string; label: string; hint?: string; error?: string; children: ReactNode }) {
  return <div className="field">
    <label htmlFor={id}>{label}</label>{children}
    {(error || hint) && <p id={`${id}-help`} className={error ? 'field-error' : 'field-hint'}>{error ? plainText(error) : hint}</p>}
  </div>;
}
