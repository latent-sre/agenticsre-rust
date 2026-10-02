import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import { describe, expect, it } from 'vitest';

const script = readFileSync('public/bootstrap.js', 'utf8');
describe('launch bootstrap', () => {
  it.each(['#token=' + 'a'.repeat(43), '#token=invalid', '#token=' + 'a'.repeat(64)])('strips every fragment before handing off a valid token', hash => {
    const events: string[] = [];
    const window = { location: { hash, pathname: '/checks', search: '?tab=details' },
      history: { replaceState: (_state: unknown, _title: string, url: string) => events.push(url) },
      matchMedia: () => ({ matches: true }), __WORKBENCH_BOOTSTRAP: undefined as unknown };
    const document = { documentElement: { dataset: {} } };
    const localStorage = { getItem: (key: string) => { events.push(key); return 'dark'; } };
    vm.runInNewContext(script, { window, document, localStorage });
    expect(events).toEqual(['/checks?tab=details', 'agenticsre.theme']);
    expect(window.__WORKBENCH_BOOTSTRAP).toEqual({ token: hash.length === 50 ? 'a'.repeat(43) : null, theme: 'dark' });
  });
});
