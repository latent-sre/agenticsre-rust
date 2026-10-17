# Configured service diagnostics — first implementation contract

This implements the first CAP-08/AC-14/AC-15 pack under the existing core/CLI and Cally fixture
boundaries. Status belongs in [the roadmap](roadmap.md). The fixed pack covers DNS, TCP, TLS,
HTTP and an explicitly configured local process. It does not require a live team catalog, make
a health assessment or persist evidence. Browser and MCP grants are later explicit increments.

## Mission and public surface

From an installed ordinary CLI away from source, select one trusted target alias, run the fixed
`connectivity` pack against controlled DNS/HTTPS/process fixtures, and return one schema-valid
receipt with all five observations, their timings, prerequisites and source/vantage identities.
CLI and structured calls must preserve the same semantics. Deliberate failures retain completed
independent checks, disclose blocked checks and stop before any unauthorized destination.

The CLI provides offline `diagnostics list` and `diagnostics describe connectivity`, plus
`save --diagnostics-config ABS_FILE [--json] diagnose --target ALIAS --pack connectivity
--input INPUT.json`. The input file contains only the typed pack input below. Existing timeout
and output controls apply; requested concurrency may be 1 or 2 and cannot exceed the pack ceiling.

```json
{"capture_body":false,"on_failure":"continue"}
```

Structured calls use operation `diagnostics.run`, version 1, target
`{"kind":"connection","id":"fixture-web"}` and
`{"id":"connectivity","version":1,"input":{"capture_body":false,"on_failure":"continue"}}`.
Input defaults are capture disabled and continue after step failure. Unknown fields, explicit
null, positional arrays at declared object positions, wrong types and unsupported pack versions
are refused before dispatch. No wire field can provide a hostname, address, URL, resolver, PID,
file path, TLS override, credential, executable or grant. Recording remains `never`.

The dedicated startup option is distinct from the existing Grafana/context configuration.
Discovery reports configuration requirements without opening files or making network/proc probes.
The [browser](diagnostics-ui.md) and [MCP](mcp.md) adapters accept this configuration only with
their explicit diagnostic execution grants and selected target IDs.

## Trusted binding

Read one strict versioned regular configuration file, capped at 64 KiB, depth 16 and 16 targets.
Pin its inode before reading, refuse symbolic links in every component, FIFOs/devices and weaker
fallbacks, and report the digest of the exact bytes used. Linux is the first verified lane.
Select aliases exactly; duplicates and absent aliases are refused without another target.
One parsed snapshot supplies every step; mid-run configuration replacement cannot change binding.

A target declares a DNS hostname or literal IP, HTTPS port and path, an explicit allowed IP list,
DNS resolver selection, optional additional startup-selected CA file and optional local process
identity. The authored configuration schema and examples must document the exact finished shape.
Keep fields typed and bounded; allow at most 16 service addresses per target. IPv4 and IPv6 forms
must be canonicalized consistently, with an explicit rule for mapped addresses. Query strings,
fragments, userinfo, path controls and arbitrary schemes are unsupported in this first pack.

Resolvers use the already locked asynchronous Hickory implementation. System selection fails
closed on unavailable/invalid resolver configuration; it never falls back to a public resolver.
An explicit resolver endpoint is a separate trusted startup binding, useful for private fixtures.
Declare its transport and lookup strategy. Bound attempts and timeouts; disclose any supported
same-endpoint transport fallback. No blocking libc resolver or unbounded background DNS work.

Validate all observed service addresses against the allowed list before TCP/TLS/HTTP. Do not
silently drop denied answers and connect to a different one. Select a deterministic authorized
address and retain it for subsequent steps; each step reports its actual peer and configured
hostname/port. Connections never re-resolve the service host or substitute a destination.
Literal IPs report the distinction from DNS lookup. Resolver traffic and service traffic have
separate explicit bindings.

TLS validates the original hostname/IP and certificate trust, using bundled roots or the explicit
CA bytes. Additional CA files obey bounded regular-file admission; no environment trust override,
disabled verification or unknown-CA retry. Report leaf certificate digest and negotiated TLS facts
without claiming service health. TCP success cannot substitute for TLS validation.

HTTP uses a fixed GET to the configured HTTPS path, with no redirects, proxy inheritance, retries,
cookies, authentication or arbitrary request headers. Pin the same authorized IP while retaining
the original TLS name/HTTP authority. Report the HTTP status as an observation; non-success status
is a failed check, with no healthy/unhealthy assessment inferred. Never copy raw transport errors
or error headers/bodies into failure messages.

Body capture is opt-in and bounded to at most 16 KiB and the tighter caller allowance. Disclose
truncation, encoding and untrusted-content classification; default observations contain no body.
Return only a small declared header allowlist. Header/body limits must include slow/oversized
responses and encoded expansion; no automatic content decompression. Unrelated sensitive content
cannot be declared redacted merely because no credentials were configured.

## Local process observation

The first supported local backend is `linux-process-v1`. Trusted configuration supplies a PID,
expected start-time ticks and expected UID, identifying one process instance. The tick identity
uses a lossless representation. Read bounded kernel proc metadata and verify identity around the
observation. Report running/sleeping/stopped/zombie/gone or unavailable state explicitly; these
are process observations and never systemd-unit, Windows-service or health assertions.

An omitted process binding is a disclosed unconfigured check. An inaccessible process, identity
mismatch, reused PID or missing proc prerequisite must not fall back to PID 1, scan processes,
execute a command or choose another instance. Fixtures supply the real private-namespace server
process identity and prove positive, exited, stopped/identity-mismatch behavior as applicable.

## Pack execution and results

The installed ID/version binds one fixed acyclic definition: DNS -> TCP -> TLS -> HTTP, with the
local process check independent. No user-authored graph, eval, recursion, shell or extension
registration is accepted. Metadata documents each typed primitive/version and prerequisite.
Execution has a ceiling of two concurrent checks; requests may lower it. Continue mode runs
independent branches after failure and blocks dependent steps explicitly. Stop mode admits no
new step after an observed failure; an already active check still receives bounded cleanup.

One monotonic parent deadline includes configuration/CA reads and every step. Cap the parent at
60 seconds and each step at 10 seconds, limited by remaining parent time. Cancellation/deadline
must close pending sockets/futures, leave no resolver worker or owned child, and prohibit further
dispatch. Preserve existing terminal delivery/exit semantics and bounded cleanup allowance.

Each step has an ID, operation/version, terminal state, attempted flag, observation/error,
prerequisites and bounded timing. Not-started steps carry a specific reason rather than zero data.
Sources preserve configuration/CA/pack identity, observation time and actual local/network/PID
vantage. Public target identity is preserved; resolved metadata never grants authority.

All requested checks completed successfully gives execution `succeeded` with complete coverage.
Mixed completed/failed/blocked/unconfigured checks are partial with explicit coverage; an entirely
failed pack is failed. Parent cancellation/timeout takes precedence while retaining completed
observations and omitted-check reasons. Assessment is always `not_assessed`; completed observations
use `not_applicable`, and admission refusals use `not_attempted`. Child exit/signal stay null.
Cap selected structured data at 64 KiB and the final encoded receipt at the existing 2 MiB ceiling.
Loss of required observations cannot remain successful/complete. Safe static errors never echo
configuration contents, response exceptions or private environment values.

## Acceptance

- Real DNS/HTTPS/kernel-process mission; CLI/call and human/JSON agreement, installed execution
  away from source, offline discovery, actual source/peer/CA/definition identity and valid schemas.
- Positive DNS/name validation, literal-IP behavior, denied/mixed answers, changed address answers,
  absent/duplicate aliases, hostile types and paths, unknown pack/input/target and policy bypass.
- TCP refusal, unknown CA, wrong TLS name, certificate failure, redirects, proxy/retry canaries,
  HTTP error, oversized/slow headers/body, body opt-in/truncation and untrusted metadata.
- Independent process continuation after a network failure; dependency blocking; stop versus
  continue, lowered concurrency, whole-parent limits, SIGINT/SIGTERM and no further effects.
- Actual configured process identity, exit/PID mismatch/unconfigured/unsupported states; no
  command effects, substitution, external file changes, response execution or leaked canaries.
- Existing command/task/Grafana/context/UI/MCP regressions for affected seams, locked offline
  builds, formatting/Clippy/specifications; independent correctness/security review and exact
  frozen-source Cally verification with full residue and staging/container/image cleanup.

Kernel/platform limitations and live acceptance stay explicit. Linux fixture success does not
establish Windows, real team mappings, protected identities, persistence, live endpoint compatibility
or human/agent usefulness. New evidence is captured with its own identity; earlier artifacts remain
retained and are not relabeled as containing diagnostics.
