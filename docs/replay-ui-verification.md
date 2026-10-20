# Browser offline replay verification — 2026-10-19

WORKBENCH-035 adds independently granted `evidence.replay` to the existing browser checks,
results and history flow under the [investigation contract](investigation-ui.md). It shares
the fixed offline analyzer already used by CLI/MCP; no core executor, storage format, network
permission or recording policy changed.

The [retained installed packet](evidence/browser-offline-replay-2026-10-19/summary.json) passed
**17 real-browser/API groups**, with **19 schema-valid canonical receipts** and **39 OpenAPI
transaction validations**. A copied ordinary executable ran from a disposable unrelated
directory inside the private UI sandbox, using synthetic context and retained fixture bundles.

| Artifact | SHA-256 |
|---|---|
| Ordinary CLI, 50,826,592 bytes | `7ed9a73b0d4a98fc6977232fef30f1530a2b3b352a3ed9aabfedc5f81db5a2f5` |
| Source manifest, 326 files | `c5b44bbad172e38698ce842524665c1ac6dab79739db12f5b55f288c9cc9db44` |

The manifest binds crates, frontend source and assets, schemas, context fixtures, dependency
manifests, OpenAPI, the investigation contract and browser harness. All manifest entries and
binary bytes remained unchanged during final acceptance. This record and roadmap were added later.

## Established behavior

- Replay appears only under its own grant. A replay-only browser session needs no context source,
  target or credentials, and direct inspection requests remain forbidden.
- The evidence ID, optional manifest pin and bounded item controls produce canonical replay
  receipts. Existing HTTP admission, deduplication, conflict, authentication and Origin rules apply.
- Results preserve original observation windows, metric/log summaries, separate analysis provenance
  and explicit partial omissions. Nested labels and saved text render inertly. The browser made no
  remote requests and reported no script errors.
- Downloads match the original API receipt bytes. History reopens those same invocation bytes
  without recomputation. Source/store replacement, missing/corrupt evidence and wrong pins fail.
- Saved evidence and source bytes/modes remain unchanged. Light desktop, dark mobile form and
  dark mobile partial-result screenshots were inspected; the replay view has no horizontal overflow.

The final workspace passed **601 Rust tests, zero failures and 33 existing scoped ignores**.
The frontend passed **88 tests**, types, lint, generated-contract checks and production build;
the 22 affected frontend tests passed again after a test-only lint correction. Strict workspace
Clippy, formatting, schema synchronization and specification validation passed. Independent static
review found no actionable P0/P1/P2 issue, with exact diff and asset hashes retained in the packet.

New HTTP/frontend tests failed for missing replay support before implementation. A browser-driver
attempt then failed because opening a history item navigates to Checks; the driver now returns to
History before selecting the second item. Product code was unchanged. Final browser acceptance
includes that correction and a separate mobile partial-result screenshot.

## Reproduction and limits

```sh
tools/test-ui-sandbox.sh npm --prefix web run build
tools/test-sandbox.sh cargo build -p workbench-cli --locked --offline
WORKBENCH_BROWSER_EXECUTABLE=/home/hawkfire/.cache/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-linux64/chrome-headless-shell \
WORKBENCH_BROWSER_LIBRARIES=/tmp/agenticsre-browser-runtime/empty-libs \
WORKBENCH_BROWSER_NODE_MODULES=/tmp/agenticsre-browser-runtime/node_modules \
  tools/test-ui-sandbox.sh node web/tests/investigations.mjs \
  target/debug/save target/browser-offline-replay/fresh-acceptance
```

Use an empty output directory. The cached browser paths name this tested environment; supply valid
cached paths on another machine. The executable and browser remain inside private namespaces.
Acceptance is local Linux x86-64 and Chromium fixture evidence. User acceptance, live sources,
other browser/platform combinations, agent-host registration and release packaging remain open.
Publication uses `codex/browser-offline-replay`, based on PR #8's `96476b2`, together with the
[current development candidate](acceptance-bundle-verification.md). The source capture above
predates publication; its accepted product bytes are unchanged.
