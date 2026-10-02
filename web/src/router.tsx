import { createRootRoute, createRoute, createRouter } from '@tanstack/react-router';
import { App } from './App';
import type { Api } from './api';
import { isCheck } from './model';
import type { Check, Theme } from './model';

type Search = { run?: string; tab?: 'output' | 'details'; check?: Check; root?: string };
export function validateSearch(search: Record<string, unknown>): Search {
  return {
    run: typeof search.run === 'string' && /^[A-Za-z0-9_-]{1,128}$/u.test(search.run) ? search.run : undefined,
    tab: search.tab === 'details' ? 'details' : 'output',
    check: isCheck(search.check) ? search.check : undefined,
    root: typeof search.root === 'string' && /^[A-Za-z0-9._-]{1,64}$/u.test(search.root) ? search.root : undefined,
  };
}
export function createWorkbenchRouter(api: Api | null, theme: Theme) {
  const rootRoute = createRootRoute({ validateSearch, component: () => <App api={api} initialTheme={theme} /> });
  const children = (['/', '/checks', '/history'] as const).map(path => createRoute({ getParentRoute: () => rootRoute, path, component: () => null }));
  return createRouter({ routeTree: rootRoute.addChildren(children), defaultPreload: false, scrollRestoration: true });
}
declare module '@tanstack/react-router' {
  interface Register { router: ReturnType<typeof createWorkbenchRouter> }
}
