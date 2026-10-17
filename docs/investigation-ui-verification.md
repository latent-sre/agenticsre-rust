# Browser investigation verification — 2026-10-15

WORKBENCH-027 is accepted for the local Linux browser scope in the
[investigation contract](investigation-ui.md). Service context, saved evidence, inspection,
comparison and report previews use independent startup grants and the existing session lifecycle.

The [retained packet](evidence/investigation-ui-2026-10-15/summary.json) contains 14 canonical
receipts, exact downloads, store snapshots, four screenshots, test logs and
[independent review](evidence/investigation-ui-2026-10-15/independent-review.json).
All 15 installed browser/API acceptance groups passed using Chromium 156.0.8078.4.

| Artifact | SHA-256 |
|---|---|
| Source manifest | `6544125fe54bc42f75cf0c524a8ca072813b305cc9c9be12458fc9683f6f1faf` |
| Ordinary installed CLI | `008ca048619806fa4e3ba07bd1c84cf4272385c10d89541df0a86aa9b0f7f295` |

The source manifest covers the crates, frontend source and embedded distribution, authoritative
schemas, context fixtures, dependency manifests, OpenAPI, investigation contract and browser harness.
Those bytes and the 51,395,024-byte installed executable remained unchanged during acceptance.
Acceptance prose, roadmap updates and retained evidence were added afterward.

## Established behavior

| Boundary | Observed evidence |
|---|---|
| Explicit authority | Exactly five granted operations appear in the session; no paths or credentials are exposed there. Empty grants remove runnable controls and reject direct API calls. Wrong bearer, foreign origin, ungranted deletion and path/record overrides are refused. |
| Context | Keyboard submission returns ownership, bindings, dependencies, freshness and source provenance. Stale sources are unusable. Changing pinned source bytes is denied. Core tests also cover inode and symlink replacement and the fixed startup age policy. |
| Historical evidence | Saved IDs open through normal integrity verification and optional manifest pins. Missing, corrupt and incorrectly pinned bundles fail honestly. A detail limit produces a partial receipt with disclosed omissions. |
| Comparison | Same-window bundles return metric/log changes; differing windows remain incompatible. Original observation times and analysis time remain distinct. |
| Reports | Markdown and JSON previews stay inert. Explicit browser downloads match the retained rendering exactly; JSON matches the report object. Proposals remain unapproved. |
| Session behavior | Canonical receipt downloads match the exact API bytes. History reopens the original invocation. Duplicate submission IDs return the original run; changed effective requests conflict. HTTP integration tests also compare all five operations with direct core results, excluding generated IDs/clocks. |
| Storage | All observation operations preserve source/store file bytes and modes. Replacing a store root with identical contents is refused. Corrupt evidence is not repaired. Core/startup tests refuse missing stores without creating them. |
| Presentation | Hostile HTML in context and report titles remains text, with no remote requests or script errors. Light/dark screenshots were inspected; the 390-pixel mobile report view has no horizontal overflow. |

## Checks and corrections

The workspace suite passed **550 tests, zero failures, 33 existing ignored cases**. Strict workspace
clippy, formatting and specification checks passed. The specification checker validated 108 schemas
and 135 positive/negative fixtures. All 54 OpenAPI components and the real session validate; all
14 retained receipts validate against the canonical result schema. The schema synchronization check
passed. The frontend passed **71 tests**, generated-type checking, TypeScript, lint and production
build checks.

Independent review found one P2 rendering defect: inherited object keys such as `__proto__` could
produce invalid React children. Both open/compare regressions failed before the fix, and pass with
an own-property lookup. The reviewer closed the finding against the corrected source hashes;
no material scoped finding remained. Review was static, with executable evidence supplied by the
separate checks above.

The browser harness first confused the app's root element with a workspace selector. A later
history assertion exposed inconsistent form/result labels, which were aligned. The final installed
workflow passed after these corrections. The final label-only backend change also passed all ten
UI unit tests; the full workspace run preceded that label change and the frontend prototype-key fix.
Frontend checks, rebuilt embedded assets and the final installed mission cover those final changes.
The retained earlier history attempt is diagnostic evidence and is not counted as a passing run.

## Reproduction and limits

Build the pinned frontend through `tools/test-ui-sandbox.sh`, then build the ordinary CLI through
`tools/test-sandbox.sh`. Copy `target/debug/save` to `target/investigation-ui/save` and run
`node web/tests/investigations.mjs` through the UI sandbox with the cached browser variables in
[development instructions](development.md#local-browser-interface). The harness uses copied
retained fixtures, synthetic context and a private temporary store. No dependency download is needed.

All executable checks ran within the existing private user/network/PID Bubblewrap boundary with
read-only source and no host credentials. The browser and server used private loopback only.
No live service or connector was configured, and no report was published. File access-time changes
are outside the retained-byte guarantee. This acceptance does not cover other browsers, native
Windows, live sources, remote/team hosting or MCP investigation grants. Changes remain local and
uncommitted.
