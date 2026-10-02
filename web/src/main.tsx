import { Component } from 'react';
import type { ReactNode } from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { RouterProvider } from '@tanstack/react-router';
import { createApi } from './api';
import type { Theme } from './model';
import { createWorkbenchRouter } from './router';
import './styles.css';

declare global { interface Window { __WORKBENCH_BOOTSTRAP?: { token: string | null; theme: Theme } } }
const bootstrap = window.__WORKBENCH_BOOTSTRAP;
delete window.__WORKBENCH_BOOTSTRAP;
const api = bootstrap?.token ? createApi(bootstrap.token) : null;
const router = createWorkbenchRouter(api, bootstrap?.theme ?? 'system');
const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });

class ErrorBoundary extends Component<{ children: ReactNode }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() { return { failed: true }; }
  componentDidCatch() { /* Do not log state, tokens or captured output. */ }
  render() { return this.state.failed ? <main className="fatal-error"><h1>The workbench could not render this view</h1><p>Reopen the launch link to recover access. Any active check continues in the server session.</p></main> : this.props.children; }
}
const element = document.getElementById('root');
if (!element) throw new Error('Missing application root');
createRoot(element).render(<ErrorBoundary><QueryClientProvider client={queryClient}><RouterProvider router={router} /></QueryClientProvider></ErrorBoundary>);
