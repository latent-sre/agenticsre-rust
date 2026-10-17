# Browser diagnostics — dependent increment contract

This is the next browser increment after the core/CLI diagnostic pack has independent
correctness, security and executable acceptance. It extends the accepted [browser contract](ui.md)
using the real [diagnostic pack](diagnostics.md) and its [configuration profile](diagnostics-config.md).
Implementation status belongs only in [the roadmap](roadmap.md). This contract alone does not
establish executable acceptance of the candidate diagnostic grant.

## Mission

Launch the ordinary installed workbench with one explicitly granted target, open its browser
session, run `connectivity` and inspect all five actual DNS/TCP/TLS/HTTP/process observations.
Reopen the unchanged receipt from session history and download it as JSON. Exercise a deliberate
network failure and an in-flight cancellation through the same browser flow. Compare each result
with the equivalent core/CLI call using only independently validated volatile-field exclusions.

## Trusted session binding

The launch form is:

```text
save --diagnostics-config /absolute/targets.json ui serve --allow diagnostics.run --target fixture-web
```

The launcher must require the operation and at least one explicit target ID together. Unknown or duplicate
grants/target IDs, absent configuration, unsupported pack/version and missing prerequisites fail
startup clearly. Diagnostic-only sessions do not require a command root or read policy. Existing
command/task sessions retain their accepted grants and behavior.

Only the fixed `connectivity` pack, version 1, is granted. Session discovery lists only the granted
aliases and installed pack metadata. It neither probes network/process availability nor exposes
ungranted target records, configuration/CA paths, credentials or arbitrary process selectors.
Kernel/process availability remains a run observation, never an invented successful startup probe.

Pin the admitted configuration-byte identity and any additional CA-byte identity for the session.
Each execution revalidates those identities through the core before dispatch. Different pinned
bytes or failure of regular-file/path admission refuses that invocation and requires a new
session; destination or certificate trust cannot silently expand. The binding is to exact bytes,
so replacing a valid regular file with identical bytes does not change that identity. Each run
still reports its actual resolver, peer, CA, process
and namespace identities. DNS answers remain subject to the accepted whole-answer allowlist rule.

Browser fields select a granted `target_id` and supply only the fixed typed pack input. They
cannot contain endpoint, IP, port, URL, resolver, PID, UID, path, CA, environment, executable or
grant overrides. A diagnostic request rejects command `root_id` and unrelated operation fields.
The adapter supplies the core connection target and trusted context; it introduces no second
network executor. The core's current hosted-adapter refusal may be replaced only with an explicit
pinned diagnostic-grant path. Generic callers must not gain diagnostic authority by omission.

## API and browser behavior

The backend first updates `docs/ui/openapi.json` and its embedded local schema references.
Generated TypeScript types then drive the frontend. Retain the current API version and routes,
authentication/Host/Origin/CSP controls, request limits, one active invocation, deduplication,
bounded session history, receipt endpoint and cancellation/shutdown semantics. Add diagnostic
session metadata, `target_id` admission and diagnostic summary labels to that established surface.
Return the complete unchanged core receipt; HTTP admission never creates a successful observation.

The diagnostic submission uses the existing run endpoint and version:

```json
{
  "submission_id": "d23a5a58-5e88-4729-a142-0f3cb6339200",
  "operation": "diagnostics.run",
  "operation_version": 1,
  "target_id": "fixture-web",
  "inputs": {
    "id": "connectivity",
    "version": 1,
    "input": {"capture_body": false, "on_failure": "continue"}
  },
  "limits": {"timeout_ms": 30000, "max_output_bytes": 16384, "concurrency": 2}
}
```

A diagnostic submission requires exactly one granted target alias and rejects `root_id`, including
explicit null. Command
and task submissions retain their existing shape and reject a diagnostic target selector or
diagnostic-only controls. Normalize the fixed pack input/defaults and its effective bounds before
forming the submission fingerprint. Include the operation/version, selected target, normalized
input and all effective limits: reusing an ID with a different alias or concurrency is a conflict,
even if the two targets happen to point at the same service. An identical retry returns its existing
invocation without starting another resolver, socket or process observation.

The backend contract must expose granted alias IDs/labels and fixed pack metadata through session
discovery, plus the selected `target_id` in run summaries. It must not mix a target alias into a
workspace `root_id`. Diagnostic controls admit only stop/continue, body capture off/on, concurrency
1/2, timeout at most 60 seconds and the core's existing output bounds. The frontend displays the
diagnostic timeout ceiling when that check is selected. No new route, API-version fallback or
larger generic request/history allowance is required.

Add a Diagnostics choice to the current check form and a selector for granted target aliases.
Show the fixed pack and its five checks. Body capture defaults off; the user can explicitly enable
it. Stop/continue and concurrency 1/2 remain bounded choices. Preserve the effective timeout and
output limits and provide a clear empty state when diagnostics are not granted.

Keep wire `api_version` at `0.1`; the additive OpenAPI artifact version is `0.1.2`.
Default omitted diagnostic controls to body capture off, `continue` and concurrency 2. Explicit
null concurrency is invalid. Admit timeout 1–60000 ms and the existing general output input range
1024–2097152 bytes, then normalize diagnostic output to its effective maximum of 16384 bytes.
The form exposes that effective bound; the fingerprint uses the normalized value. Command/task
requests reject diagnostic selectors/controls even when explicitly null and retain their accepted
generic limits. This normalization introduces no larger core allowance.

After the run, display each check's terminal state, actual observation, elapsed time and specific
failure/blocked reason. An unattempted check says it was not run; null timing is not displayed as
zero. Partial coverage, timeout and cancellation stay visible, and observations never become a
service-health verdict. No progress stream is promised by a core that emits only its final receipt.

Show source/configuration/CA/pack identities and actual network/process vantage in the result
details, including reported CNAME chains and bounded DNS questions. Describe `questions` as
transport requests: a closed stream can reject one before transmission, so the browser cannot
label that list as a count of packets sent. Keep the configured service/TLS name distinct from
its DNS aliases; aliases grant no new service destination or trust. Captured body bytes remain
explicitly untrusted, with encoding and truncation visible;
render text/base64 as inert text only. No HTML interpretation, automatic result links, remote
images, terminal escape interpretation or response-driven actions. Preserve download of the
original receipt rather than a reconstructed browser model.

Use the accepted light/dark layout, mobile navigation and keyboard patterns. This increment adds
controls and result rows within that visual design; it does not replace the interface framework.
Server-session history stays bounded and ephemeral. Persistence, AI chat, MCP diagnostic grants
and live target onboarding remain separate dependent capabilities.

## Required evidence

- A real browser-to-HTTP-to-core five-check mission, history reopen, JSON download and CLI parity
  against private DNS/HTTPS and actual process fixtures, from the installed ordinary binary.
- Empty/ungranted session, alias and operation injection, changed config/CA bytes or invalid
  replacement file/path, hostile types,
  output limits and startup combinations; unauthorized requests cause no resolver/service effects.
- Real partial and blocked results, stop/continue, concurrency, busy/deduplicated admission,
  browser cancellation during witnessed network IO, shutdown and no surviving fixture/worker.
- Text-only hostile output, both themes, mobile and keyboard primary flow in Playwright, with
  retained screenshots and exact schema-valid receipts. Do not replace the mission with mocks.
- Independent affected correctness/security review and executable acceptance bound to immutable
  bytes. Reuse unchanged core, transport and browser proofs with their original revision identities;
  rerun affected checks for new grants, request normalization and result rendering.

No Linux fixture result establishes Windows, durable history, live endpoints or protected identity.
