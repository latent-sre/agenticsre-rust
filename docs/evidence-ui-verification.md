# Browser evidence collection acceptance — 2026-10-19

WORKBENCH-033 passed the installed Linux browser mission against ordinary binary SHA-256
`6830d57b2387f47283ddec42fb9f018f3297b59f39418f11feb6f1f3c4969635`.
The executable was copied into a disposable prefix and launched from an unrelated directory.
Chromium, the UI and real HTTPS fixture ran inside the existing private user/network/PID sandbox
with synthetic credentials, generated certificates and a disposable initialized evidence store.

The [retained evidence](evidence/evidence-ui-2026-10-19/browser-collection-results.json) contains
six canonical receipts, the binary/copy digests, and before/after-checked source hashes for Rust,
frontend source/generated types/assets, schemas and the mission. All receipts passed the core
result schema. Collection data passed its strict schema. Every returned sealed manifest and each
retained artifact was checked against its digest; retained receipts were schema-validated and
checked for the synthetic credential canary. Nineteen actual session, submission and run-summary
objects also passed the OpenAPI contract. The UI, browser driver and fixture threads were joined.

The browser exercised:

- Independent pack listing and description with no fixture traffic, including digest display.
- Invalid-field focus without submission, separate collection target selection and explicit store effects.
- Pinned collection with two real HTTPS observations, one common relative window and literal
  markup-looking jobs verified against the exact PromQL/LogQL request expressions.
- Sealed bundle identity display, exact receipt downloads before/after reopening session history,
  and separately granted offline inspection without additional HTTP requests.
- A failed first observation with `stop`, retained partial evidence and an explicitly omitted second step.
- Cancellation only after the fixture witnessed the first query, a cancelled receipt and no second query.
- Inert returned content, no external browser requests, no cookies or credential persistence, and
  theme-only local storage. Mobile layout had no horizontal overflow.

Screenshots were inspected in [light desktop](evidence/evidence-ui-2026-10-19/collection-light.png),
[dark desktop](evidence/evidence-ui-2026-10-19/partial-dark.png),
[narrow mobile](evidence/evidence-ui-2026-10-19/partial-mobile-dark.png) and the
[offline catalog](evidence/evidence-ui-2026-10-19/catalog-light.png). Step outcomes and omissions
remain visible; technical receipt details use expandable sections.

Four installed API test groups additionally cover independent catalog grants, strict field/null/
limit/pin refusals, retry identity and conflicts, separate read authority, startup prerequisites,
no store initialization, target-scope separation, and config/CA/context/store drift before network
or retention effects. Five frontend tests cover grant visibility, typed inputs, validation,
store-effect disclosure and inert results. The complete frontend suite passed 84 tests; after the
final result-layout adjustment the five affected tests passed again. TypeScript, ESLint, generated
API parity and the production asset build passed. Independent static review found no actionable
correctness or security finding in the local source changes.

The final installed mission command was:

```sh
WORKBENCH_BROWSER_EXECUTABLE=/home/hawkfire/.cache/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-linux64/chrome-headless-shell \
WORKBENCH_BROWSER_LIBRARIES=/tmp/agenticsre-browser-runtime/empty-libs \
WORKBENCH_BROWSER_NODE_MODULES=/tmp/agenticsre-browser-runtime/node_modules \
  tools/test-ui-sandbox.sh .venv/bin/python -B tools/verify-evidence-ui.py target/evidence-ui-20261019/save
```

The driver used offline-cached Playwright 1.64.1 and Chromium revision 1248. Browser shared libraries
were already available through the sandbox's system mounts; the extra library directory was empty.
No host packages, services or live integrations were changed. Initial harness attempts assumed the
wrong offline submit-button label and a unique status badge across nested step headers; selectors
were corrected. The final mission passed with the rebuilt display and HTTP contract validation.
Live compatibility, native Windows, additional packs and other recording policies remain open.

The [final integrated record](evidence/evidence-ui-2026-10-19/summary.json) reports 599 passing Rust
tests, zero failures and 33 existing scoped ignores. Strict all-target/all-feature workspace Clippy,
formatting, schema synchronization and specification checks passed. After the full Rust suite and
installed mission, only a nested conditional in the new API test was collapsed to satisfy Clippy;
all four API tests passed again. Production code, assets, schemas and browser harness hashes still
match the installed acceptance record. The local source review is not immutable merge approval.

## PR #7 review follow-up — 2026-10-19

Both P2 comments were reproduced with failing tests and corrected. The form remembers target
choices separately for collection, queries and diagnostics; installed catalogs preserve their
family's selection without submitting a target. Collection selectors now count Unicode code points,
matching the schema/core 128-character limit. Tests cover 65/128 non-BMP characters, refusal at
129, catalog/query/target-free navigation, and the actual collection submission target.

The [follow-up record](evidence/evidence-ui-2026-10-19/review-followup/summary.json) reports 86 passing
frontend tests, TypeScript, ESLint, rebuilt assets and ordinary CLI build. The refreshed installed
browser mission passed all six receipts and 19 HTTP contract checks using two aliases and a
128-emoji service name, including the selected second alias after catalog navigation. Its
[source-bound receipts](evidence/evidence-ui-2026-10-19/review-followup/browser-collection-results.json)
retain the new executable/asset identities. Earlier full Rust/API results remain their recorded
baseline; these corrections change frontend behavior and acceptance fixtures only.
