# Local browser workbench — interface contract

- **Version:** 0.1.1 (wire `api_version` remains `0.1`), 2026-10-02.
- **Owner:** GUI backend builder owns this contract and `docs/ui/openapi.json` during implementation;
  changes go through the coordinator and must reach the frontend builder before use.
- **Status:** The owner selected a local browser app with direct checks and results/history, ahead
  of MCP and AI chat. The subsequent “continue” accepts the presented light/dark mockup for
  implementation. Live status remains in [the roadmap](roadmap.md).

## Mission and first slice

Start one local `save ui serve` process, select a launcher-granted checkout, run a Git/ripgrep
check, inspect its actual core receipt, and reopen it from this server session's history. The
numerical error-budget form supplies a second useful flow using the accepted task implementation.

The [static visual specification](ui/mockup.html) covers checks, result details, history, empty
and unavailable states in light/dark themes and a narrow layout. Images in `ui/previews/` are
explicitly illustrative. The UI uses the same dispatcher, literal command grammar, task and
result contracts as the CLI. There is no unrestricted command fallback or alternate privileged
execution path. Preserve the current-image internal launcher before ordinary CLI parsing.

Initial forms cover Git status, diff and bounded log; ripgrep listing and literal text search;
and a numerical time-budget calculation. The API permits only the existing numerical error-budget
input schema for `task.run`. Dashboard file tasks, browser filesystem browsing/uploads, Grafana
forms, arbitrary shell input, AI chat, MCP, persistent history and remote/team access are later
increments. Existing CLI capabilities retain their current behavior.

## Packaging and execution

Use embedded React/TypeScript static assets and one Rust HTTP adapter in the existing `save`
executable. No database, background service registration, external CDN or separate Node runtime
is needed to run the built app. Node is a build/test dependency only. Preserve the restricted
launcher's 64 MiB image bound and support a build without development debug symbols.

The launcher binds only `127.0.0.1`, default ephemeral port; a caller-selected loopback port is
allowed for an explicit SSH tunnel. No wildcard/hostname/non-loopback bind in this slice. The
launch command prints the local URL; it does not open browsers or modify system services.
An operator using another machine can use an existing SSH tunnel. This contract does not
implicitly authorize LAN exposure or deployment.

The launcher supplies repeated `--allow` operation IDs (`process.exec`, `command.inspect`,
`task.run`) and `--root` IDs. Empty grants expose no checks. Command grants require an explicit
trusted `--read-policy` and at least one declared, permitted root. `task.run` grants only
`error-budget` version 1. Unknown/duplicate grants and missing prerequisites fail startup clearly.

Browser requests contain IDs, never cwd/config/policy/executable paths, URLs, environments,
runtimes or permission overrides. Root IDs map to the trusted policy's exact roots. Keep the
mapping/policy identity fixed for the session; refuse dispatch on policy replacement rather than
silently changing a grant. Revalidate through the core at every execution. Discovery intersects
these grants and actual dependencies; configured isolation is not a successful kernel probe.

## Browser boundary

Generate a random 256-bit process-scoped bearer token. Put it only in the operator's launch
URL fragment as `http://127.0.0.1:PORT/#token=TOKEN`, where TOKEN is the 43-character
unpadded base64url encoding of 32 random bytes. The frontend reads and immediately removes the fragment using `history.replaceState`,
holds the token in memory, and sends it in `Authorization: Bearer …`. Do not store it in query
parameters, cookies, local/session storage, request logs or browser history after bootstrap.
The launch link is intentionally sensitive operator output; later diagnostics redact it.
Reload without a token shows “Reopen the workbench launch link”; server history remains in memory.
Stopping the process revokes the token. This is not a protected identity against the same OS account.

Every API request needs the bearer token. Require an exact bound `Host` authority and ignore
forwarded-host headers. Mutations additionally require exact same-origin `Origin` and
`application/json`; refuse absent/null/foreign origins. Refuse foreign origins on reads when
present. Do not enable CORS or cross-origin preflight. GETs never start or cancel work.

Serve assets from a compiled allowlist, not request-derived filesystem paths. Restrict CSP to
bundled assets and same-origin API connections; use `frame-ancestors 'none'`, `base-uri 'none'`,
`form-action 'none'`, `nosniff`, no-referrer and no-store. The synchronous external `/bootstrap.js` removes the token fragment and applies the theme
before bundled application code. No inline script permission is granted. Escape output as text; no HTML, terminal escape interpretation,
automatic links or remote images from results. Only theme preference persists in the browser.

## Endpoints

All routes below require bearer authentication; mutations also require Origin/content type.
The machine-readable shapes live in `ui/openapi.json`; generate the TypeScript client types from
that artifact. The backend owns it and tests served responses against it. Embed the existing
core schemas by local references for offline generation/validation; never hand-recreate receipts.

| Method + path | Purpose |
|---|---|
| `GET /api/v1/session` | Session identity, granted catalog/roots and bounds |
| `POST /api/v1/runs` | Admit one invocation; `202` and an invocation summary |
| `GET /api/v1/runs?limit=50&offset=0` | Bounded newest-first summaries, no captured output |
| `GET /api/v1/runs/{id}` | Current summary |
| `GET /api/v1/runs/{id}/receipt` | Complete unchanged core result; `409` until terminal |
| `POST /api/v1/runs/{id}/cancel` | Idempotent cancellation request with `{}` body |
| `DELETE /api/v1/runs` | Forget terminal receipts; preserve active work and deduplication |

### Session

Request: authenticated `GET /api/v1/session`, no body.
Response `200`:

```json
{"api_version":"0.1","session_id":"s-example","roots":[{"id":"checkout","label":"agenticsre-rust"}],"operations":[{"id":"process.exec","version":1,"profile":"linux-read-v1"},{"id":"command.inspect","version":1,"profile":"linux-read-v1"},{"id":"task.run","version":1,"tasks":["error-budget"]}],"limits":{"active_runs":1,"history_entries":50,"history_bytes":16777216,"max_request_bytes":65536,"max_submissions":1024}}
```

### Start a check

Request `POST /api/v1/runs`:

```json
{"submission_id":"a19c8f9e-4f43-43df-b61b-98dbd77f5949","operation":"process.exec","operation_version":1,"root_id":"checkout","inputs":{"program":"git","args":["status","--short"]},"limits":{"timeout_ms":30000,"max_output_bytes":1048576}}
```

The adapter supplies cwd, the local target, `record=never` and the trusted core context. It
validates the normalized request with the existing core limits; it does not implement a second
command grammar. The numerical request omits `root_id`:

```json
{"submission_id":"e8044313-79de-41ed-a2cd-a4c89106f109","operation":"task.run","operation_version":1,"inputs":{"id":"error-budget","version":1,"input":{"slo":99.9,"window_days":28,"bad_minutes":20}},"limits":{"timeout_ms":30000,"max_output_bytes":1048576}}
```

Response `202` and `Location: /api/v1/runs/inv-example`:

```json
{"id":"inv-example","operation":"process.exec","check_label":"Working tree status","root_id":"checkout","state":"running","submitted_at":"2026-10-02T20:00:00Z","execution_status":null,"receipt_url":null}
```

### List and inspect

Request: `GET /api/v1/runs?limit=50&offset=0`, no body. `limit` is 1–50, `offset` is 0–49;
unknown/duplicate query fields are invalid. This is a small bounded session list, not a durable
paginated database. Response `200`:

```json
{"runs":[{"id":"inv-example","operation":"process.exec","check_label":"Working tree status","root_id":"checkout","state":"terminal","submitted_at":"2026-10-02T20:00:00Z","execution_status":"succeeded","receipt_url":"/api/v1/runs/inv-example/receipt"}],"next_offset":null,"evicted_count":0}
```

Request: `GET /api/v1/runs/inv-example`, no body. Response `200` is the same individual summary.
The states are `running`, `cancelling` and `terminal`; terminal execution status comes from the
actual receipt. Labels come from the fixed check kind, not request/output prose.

Request: `GET /api/v1/runs/inv-example/receipt`, no body. Response `200` is the complete existing
`OperationResult` described by `sre-workbench/schemas/result.schema.json`, with no UI wrapper,
renamed fields or synthesized success. This same body is the downloadable JSON receipt. A failed
or refused core operation still has a retrievable receipt; HTTP success does not mean task success.

### Cancel and clear

Request: `POST /api/v1/runs/inv-example/cancel`, body `{}`. Response `202`:

```json
{"id":"inv-example","operation":"process.exec","check_label":"Working tree status","root_id":"checkout","state":"cancelling","submitted_at":"2026-10-02T20:00:00Z","execution_status":null,"receipt_url":null}
```

A terminal invocation returns its unchanged terminal summary with `200`. A cancellation request
never manufactures a terminal cancelled result; the worker supplies the actual final receipt.

Request: `DELETE /api/v1/runs`, body `{}`. Response `200`:

```json
{"removed":3,"active_preserved":true}
```

## Errors and bounds

Use top-level RFC 9457 `application/problem+json` with a bounded request ID:

```json
{"type":"urn:agenticsre:problem:run-in-progress","title":"A check is already running","status":409,"detail":"Wait for the current check or cancel it.","request_id":"http-example"}
```

Categories: `400` malformed/duplicate JSON or invalid query syntax; `401` missing/invalid token;
`403` origin/grant refusal; `404` unknown handle/route; `409` busy, conflicting submission or
receipt not ready; `410` evicted receipt; `413` oversized body; `415` wrong content type;
`422` invalid typed input (with field errors); `429` bounded overload with `Retry-After`;
`503` shutdown/session submission capacity; `500` internal failure without internal details.
Core grammar/runtime/tool outcomes remain in core receipts, distinct from these HTTP errors.

Enforce 64 KiB input, depth 24 before typed JSON decoding, duplicate/unknown-field refusal and
existing tighter normalized core bounds. Keep body/header read and response-write deadlines,
connection/request concurrency and buffers bounded. No request bodies, output or credentials in
HTTP diagnostics. The transport is HTTP/1 only, with 16 concurrent connections, one request per connection (no
keepalive), at most 32 headers and a 16KiB HTTP parser buffer. Headers and a complete request
body each have a 2s deadline; the entire connection, including response write, has a 5s deadline.
An overload rejection consumes at most 100ms and creates no queued task. Protocol framing errors
may be closed by the HTTP parser before an application problem can be delivered. Body timeout
returns 408 when the transport can still deliver the problem. Request targets are capped at 2048
bytes. The body remains limited to 64KiB even for chunked transfer; compressed bodies are refused.
Static assets total at most 2MiB across 128 allowlisted files; asset requests never read the filesystem.
The session retains at most 1024 small submission summaries and SHA-256 normalized-request
identities. Terminal receipt bytes use shared immutable buffers while being delivered.

## Work, history and lifecycle

One owned blocking core worker, one active execution, no queue. Busy admission cannot consume
unbounded task/thread capacity. Discovery, polling and cancellation remain responsive. Give every
invocation its own `RunControl`; do not use an unrelated HTTP socket as the core result descriptor.

Poll active summaries every 500 ms and fetch the receipt once terminal. No output stream is
promised by the current core. Do not retry mutations automatically. A repeated `submission_id`
with the same normalized request returns its original handle; a changed payload gets `409`.
Keep bounded submission tombstones through receipt eviction/clear. A session accepts at most
1,024 submissions; beyond that return an actionable new-session error without launching work.

Retain at most 50 terminal receipts and 16 MiB of serialized receipt bytes, evict oldest terminal
results, and bound summaries/tombstones separately. Active work is preserved. Report evictions.
No disk history/database/browser output storage. The client keeps only the selected full receipt
and drops it when deselected. The UI explains that closing the server loses session history.

A lost admission response is reconciled with the same submission ID, never silently replayed as
a new operation. Browser disconnect does not cancel or retry work; reconnection can inspect it.
Server SIGINT/SIGTERM closes admission, signals active work and awaits bounded core cleanup. A
failed cleanup is explicit and prevents further execution. Shutdown allows five seconds for the
owned core worker after signalling cancellation; timeout is reported as unconfirmed cleanup.
If the worker panics, admission closes, its control is signalled, and the server exits with a
bounded outcome/cleanup-unconfirmed diagnostic. No synthetic core receipt is created. The
browser reports a lost connection and unknown outcome; it must not automatically replay work.
Closing the tab is not server shutdown.

## Build and verification gates

The approved visual specification precedes framework work. First integrate a thin real browser →
authenticated HTTP → numerical core task → receipt → history flow; then extend checks/states with
separate frontend/backend ownership. Use actual generated contract types and actual core receipts.

Verify allowed command/task parity, hidden/unknown grants and path/config injection refusal, busy
and duplicate submission handling, cancellation/finalization, lost response/reconnect, eviction,
shutdown and HTTP bounds. Verify Host/Origin/token/CORS/CSRF controls and text-only result rendering.
Render both themes, keyboard-only primary flow, mobile navigation, loading/empty/error states and
server disconnect in Playwright. Reuse accepted runner evidence; carry its actual namespace-failure
coverage gap until genuinely exercised rather than relabeling it passed for GUI acceptance.

Tests run only in the declared private local/Cally boundaries, without real credentials or live
targets. Freeze source and independently review/verify the new HTTP/grant/lifecycle surface before
claiming acceptance. Rollback is stopping the local process and using the existing CLI; there is
no database migration, service registration or host-policy change.

## Local build and launch

Ordinary Cargo builds embed the prebuilt `web/dist` allowlist and do not run Node or contact a
package registry. Frontend changes must regenerate that directory using the frontend's locked
build command before Cargo build. The built runtime needs no Node installation. Build without
development debug symbols to preserve the restricted launcher's 64MiB current-image limit:

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 cargo build --locked -p workbench-cli
./target/debug/save ui serve --allow task.run
./target/debug/save --read-policy /absolute/policy.json ui serve --allow process.exec --allow command.inspect --allow task.run --root checkout
```

The printed launch link is sensitive. Empty `--allow` grants start a session with no checks.
No browser is opened automatically. SIGINT or SIGTERM stops the server and cancels its active
worker, with exit 130/143 respectively. History and token disappear with the process. A missing numerical task runtime hides
that operation from discovery. Invalid command policy/dependencies fail startup; core inspection
runs no command and makes no successful kernel-probe claim. Command execution revalidates the
same policy digest at admission and records an unchanged denial receipt if it changed.

## Change log

| Version | Change | Propagated to |
|---|---|---|
| 0.1 | Owner-selected browser/checks scope, approved mockup, shared-core HTTP/session contract | GUI backend and frontend builders |
| 0.1.1 | Pin fragment format, bootstrap, HTTP/worker bounds and embedded-build behavior; wire API stays 0.1 | GUI frontend and coordinator |
