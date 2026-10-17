# Saved-query browser acceptance — 2026-10-17

The installed Linux browser mission passed against ordinary binary SHA-256
`7c6d4d22d386cc00c5ea714c630688cad19a5d5ac681608967afdcffcabab5c6`.
The test copied that binary into a disposable prefix and ran it from an unrelated working
directory. All listeners, generated test certificates, synthetic credentials and browser work
remained inside the existing private user/network/PID sandbox.

The [retained result](evidence/query-ui-2026-10-17/browser-query-results.json) contains six
canonical receipts, the installed binary identity, and before/after-checked SHA-256 identities
for UI source, generated API types, embedded assets, schema and browser harness. Every receipt
passed the authoritative core result schema; returned saved-query definition data also passed
the strict query-run-data schema. The UI process, browser driver and fixture workers were joined.

The browser exercised independent catalog, description and execution selections without
workspace or diagnostic grants. Catalog listing and description generated no fixture traffic.
Missing required inputs focused the invalid field without admitting a submission. Real PromQL
execution preserved a quote/backslash/Unicode/operator-looking job as one literal matcher,
the explicit absolute window and inspected definition digest. The exact compiled expression
was checked against the HTTPS fixture request ledger. Original receipt download matched API
bytes before and after reopening the invocation from history. A mismatched definition pin
returned a denial with no additional HTTP requests.

Real LogQL execution resolved its 300-second relative window once from the parent receipt's
start. Hostile returned markup stayed inert text and caused no external browser request.
Cancellation was requested only after the fixture witnessed the query request; the receipt
reported `cancelled` without a retry. Browser storage retained only the theme preference.
Light/dark desktop and narrow mobile layouts were inspected; the mobile document had no
horizontal overflow. See the [catalog](evidence/query-ui-2026-10-17/catalog-light.png),
[PromQL](evidence/query-ui-2026-10-17/prometheus-light.png),
[LogQL](evidence/query-ui-2026-10-17/loki-dark.png) and
[mobile](evidence/query-ui-2026-10-17/loki-mobile-dark.png) screenshots.

Frontend checks passed: 79 Vitest tests across eight files, TypeScript, ESLint, generated
OpenAPI types and the production asset build. Eight new tests cover independent grants,
literal normalized submissions, absolute/relative bounds, digest validation, invalid-field
focus, target scope, metadata and inert observations.

The installed mission command was:

```sh
tools/test-ui-sandbox.sh .venv/bin/python -B tools/verify-query-ui.py target/finish-20261017/save
```

The sandbox used the existing pinned UI dependency cache, staged Playwright 1.61.1 driver,
Chromium headless shell revision 1248 and extracted browser runtime libraries selected through
the documented `WORKBENCH_UI_NODE_MODULES` and `WORKBENCH_BROWSER_*` environment variables.
No host package installation, live connection or service deployment was needed.

Initial acceptance attempts exposed three harness assumptions: history navigation clears form
drafts, cancellation has both a status badge and an error heading, and the fixture RPC input
must close after its completion acknowledgement. The harness was corrected to refill inputs,
select the header badge and close its input pipe. The final mission passed with unchanged
product source and embedded assets. Live Grafana compatibility and native Windows remain
unverified by this Linux fixture acceptance.
