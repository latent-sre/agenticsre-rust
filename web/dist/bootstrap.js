// Deliberately synchronous: remove the launch secret before any application work.
(() => {
  const fragment = window.location.hash;
  window.history.replaceState(null, '', window.location.pathname + window.location.search);
  const token = /^#token=([A-Za-z0-9_-]{43})$/.exec(fragment)?.[1] ?? null;
  let theme = 'system';
  try {
    const stored = localStorage.getItem('agenticsre.theme');
    if (stored === 'light' || stored === 'dark') theme = stored;
  } catch { /* A blocked preference store must not prevent access. */ }
  document.documentElement.dataset.theme = theme === 'system'
    ? (window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light') : theme;
  window.__WORKBENCH_BOOTSTRAP = { token, theme };
})();
