import { afterEach, expect, it, vi } from 'vitest';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, RouterProvider } from '@tanstack/react-router';
import { createApi } from './api';
import { createWorkbenchRouter } from './router';
import apiContract from '../../docs/ui/openapi.json';

const originalClose = Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, 'close');
afterEach(() => {
  cleanup(); vi.unstubAllGlobals();
  if (originalClose) Object.defineProperty(HTMLDialogElement.prototype, 'close', originalClose);
  else Reflect.deleteProperty(HTMLDialogElement.prototype, 'close');
});

it('preserves independent workspace and diagnostic target selections across check kinds', async () => {
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() }));
  vi.stubGlobal('scrollTo', vi.fn());
  // jsdom omits native dialog lifecycle; browser fixtures cover the real drawer separately.
  Object.defineProperty(HTMLDialogElement.prototype, 'close', { configurable: true, value: vi.fn() });
  const session = { ...apiContract.components.schemas.Session.examples[0],
    roots: [{ id: 'first', label: 'First checkout' }, { id: 'second', label: 'Second checkout' }],
    targets: [{ id: 'fixture-web', label: 'Fixture web' }, { id: 'fixture-other', label: 'Fixture other' }],
    operations: [...apiContract.components.schemas.Session.examples[0]!.operations,
      { id: 'process.exec', version: 1, profile: 'linux-read-v1' }, { id: 'task.run', version: 1, tasks: ['error-budget'] }],
  };
  const fetcher = vi.fn<typeof fetch>().mockImplementation(request => {
    const url = request instanceof Request ? request.url : String(request);
    return Promise.resolve(new Response(JSON.stringify(url.endsWith('/session') ? session : {
      runs: [], next_offset: null, evicted_count: 0,
    }), { headers: { 'content-type': 'application/json' } }));
  });
  const router = createWorkbenchRouter(createApi('synthetic-token', fetcher), 'light');
  router.update({ history: createMemoryHistory({ initialEntries: ['/checks'] }) });
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(<QueryClientProvider client={client}><RouterProvider router={router} /></QueryClientProvider>);
  const user = userEvent.setup();
  await user.selectOptions(await screen.findByLabelText('Workspace'), 'second');
  await user.selectOptions(screen.getByLabelText('Check'), 'diagnostics');
  await user.selectOptions(await screen.findByLabelText('Diagnostic target'), 'fixture-other');
  await user.selectOptions(screen.getByLabelText('Check'), 'budget');
  await screen.findByLabelText('Availability SLO (%)');
  await user.selectOptions(screen.getByLabelText('Check'), 'status');
  expect(await screen.findByLabelText('Workspace')).toHaveProperty('value', 'second');
  await user.selectOptions(screen.getByLabelText('Check'), 'diagnostics');
  expect(await screen.findByLabelText('Diagnostic target')).toHaveProperty('value', 'fixture-other');
  expect(fetcher.mock.calls.every(call => !(call[0] instanceof Request) || call[0].method === 'GET')).toBe(true);
});
