# Verification and acceptance plan

This document defines how the product will be accepted. Planning-file validation checks document
consistency and schema examples only; it cannot establish runtime safety, performance or usability.
Runtime evidence and remaining gaps are recorded in [the roadmap](../roadmap.md). The bounded
[PoC contract](../poc.md) selects the first runtime cases; a subset pass does not close the broader
case or establish a supported live deployment.

## Acceptance catalog

| ID | Behavior and independent expected result | Test boundary |
|---|---|---|
| AC-01 | Help/discovery perform no network work; successful text/JSON commands agree; all failures use documented exits | CLI black-box |
| AC-02 | Literal args containing spaces, Unicode, quotes and shell metacharacters reach the intended binary unchanged; no accidental shell effects | Real child fixture per OS |
| AC-03 | PowerShell/profile/batch/pipeline edge cases are accepted only by documented adapters; failed earlier pipeline stage remains visible | Windows PowerShell 5.1/7 and supported POSIX shell |
| AC-04 | Wrong executable/script path, writable substitution and workspace lookalike fail before effect; interpreter absence is explicit | Installed package with adversarial fixtures |
| AC-05 | Deadline/output caps hold for noisy, blocked and child-spawning processes; cancellation reports any child it cannot stop | Process-tree and resource tests |
| AC-06 | Read-profile escape flags, config hooks, environment overrides and caller-supplied role/grant fields cannot widen access | Policy and real process integration |
| AC-07 | Grafana org/datasource mismatch stops before selected query; response identifies the observed bound target | Controlled HTTP service and selected live org |
| AC-08 | HTTP 200 query errors fail; ranges, result bytes, query points/lines, redirects, TLS and retries honor policy | HTTP failure server and adapter contract |
| AC-09 | Fixture credentials and encodings do not appear in results/events/logs/artifacts, including errors and split output chunks | Secret-canary output tests |
| AC-10 | CLI/MCP give equivalent operation, result, failure and coverage semantics; healthy state is not inferred from transport success | Cross-interface fixture comparison |
| AC-11 | Task result schemas and bundle paths/digests are validated; malformed output, traversal and reparse/symlink escape are refused | Script and artifact integration |
| AC-12 | Ambiguous/missing/stale context never selects a live target silently; provenance and partial optional mappings survive | Realistic service-record fixtures |
| AC-13 | Partial collection retains each failure/omission and coverage scope; disk-full cannot report a complete bundle | Run store and filesystem failure injection |
| AC-14 | DNS/TCP/TLS/HTTP observations preserve vantage and per-check results; unauthorized hosts/ports are not contacted | Controlled network fixtures |
| AC-15 | Pack/runbook parent limits include children; stop/continue rules and prerequisites work; no hidden recursion or denied-step substitution | Workflow executor |
| AC-16 | Saved query/workflow parameters are typed; injection and changed definitions fail; resume respects version binding | Parser and adapter contracts |
| AC-17 | Comparison rejects incompatible units/targets/windows and never converts missing data to zero | Independent comparison fixtures |
| AC-18 | Offline replay causes no network/command effects; tampered evidence is identified and old timestamps retained | Network-disabled execution |
| AC-19 | Config checks detect known bad input, refuse unsupported models and leave originals unchanged; no live-health claim | Validator boundary |
| AC-20 | Reports retain conflicting evidence, source labels and redactions; knowledge updates remain proposals; no message is sent | Deterministic rendering plus adversarial content |
| AC-21 | Toil arithmetic uses consistent units/sample counts and subtracts maintenance; opt-out and retention work | Known numerical fixtures and privacy checks |
| AC-22 | Job disconnect/restart/duplicate submit/cancel do not lose ownership or replay effects; sequence gaps are disclosed | Crash/recovery integration |
| AC-23 | Remote caller/runner authentication and authorization both apply; partitions cannot cause unauthorized replay | Two-host failure environment |
| AC-24 | Exact-plan approval expires, binds scope and resists replay; target/implementation drift prevents apply | Protected change fixture |
| AC-25 | Crash/lost response after dispatch produces unknown; readback/reconciliation gates retry; recovery is actually exercised | Target-native effect simulator and approved live canary |
| AC-26 | Extension install/metadata cannot execute or grant itself access; incompatible/replaced package is refused | Package/protocol adversarial tests |
| AC-27 | Actual host loads only intended MCP tools; success/error/cancel/denial and no-input paths work; unavailable hosts are explicit | Installed Claude/Copilot acceptance |
| AC-28 | Embedded instructions in logs/dashboard/plugin text do not redirect execution or expose secrets; taint remains visible | Adversarial fixtures and bounded agent evaluation |
| AC-29 | Fourth distinct capability integrates through existing contracts; old consumers reject unsupported versions cleanly | Extension experiment and compatibility suite |
| AC-30 | Frozen target set, concurrency, failure threshold and per-target outcomes hold under partial discovery/failure | Multi-runner fixtures |
| AC-31 | Schedule ownership, overlap, DST, missed runs, restart and pause races are deterministic; no unauthorized notification | Injected clock and persistent scheduler |
| AC-32 | UI uses same permissions/results, escapes content, blocks cross-site actions and supports keyboard/small screens | Browser and API acceptance |
| AC-33 | Each additional integration has version/edition/auth/limits/failure/coverage tests and actual target acceptance | Connector-specific contract packet |
| AC-34 | Clean install works on each supported OS/arch; optional runtimes are diagnosed; offline help/doctor remain useful | Fresh machine/VM |
| AC-35 | Upgrade and downgrade preserve config/evidence under the published migration contract; bad artifact is refused | Release rehearsal |
| AC-36 | Five representative tasks have measured before/after evidence, user feedback, documented limitations and support ownership | Human/agent pilot |

## Requirement traceability

| Requirement | Acceptance IDs |
|---|---|
| REQ-01 | AC-01, AC-02, AC-03, AC-04, AC-05, AC-06 |
| REQ-02 | AC-07, AC-08, AC-09, AC-10 |
| REQ-03 | AC-04, AC-11 |
| REQ-04 | AC-01, AC-10, AC-27, AC-28 |
| REQ-05 | AC-12 |
| REQ-06 | AC-14, AC-15 |
| REQ-07 | AC-11, AC-13, AC-17 |
| REQ-08 | AC-08, AC-15, AC-16 |
| REQ-09 | AC-19 |
| REQ-10 | AC-18, AC-20 |
| REQ-11 | AC-21 |
| REQ-12 | AC-26, AC-29 |
| REQ-13 | AC-22, AC-23, AC-30 |
| REQ-14 | AC-31 |
| REQ-15 | AC-24, AC-25 |
| REQ-16 | AC-32, AC-33 |
| REQ-17 | AC-34, AC-35, AC-36 |

NFR-01 through NFR-12 are verified across these cases: parity AC-10; bounds AC-05/08/15/30;
honesty AC-10/13/17; authority AC-06/23/24/26; secrets AC-09/28; portability AC-03/27/34;
compatibility AC-29/35; recovery AC-22/25; extensibility AC-29; performance/pilot AC-36;
diagnostics AC-01/34; privacy AC-09/13/20/21.

## Platform and host matrix

Proposed first supported build targets are Windows x86-64 and Linux x86-64, with macOS arm64 next.
The owner chooses the exact OS versions, Linux ABI/libc baseline and additional architectures in
DEC-03. RHEL 9+ is a relevant team runtime; Ubuntu is a relevant CI host, not proof of RHEL support.
PowerShell 5.1 and 7 differ and require separate evidence. Remote/WSL hosts are distinct from the
desktop where an agent UI is visible.

For each advertised combination record: product commit or frozen-source digest, artifact digest, toolchain, OS/arch, shell,
MCP host/client version, plugin version, operation set, credential boundary, tests run, outcomes
and limitations. A failed or unavailable combination remains unsupported/experimental in release
notes. Never infer support from successful compilation alone.

## Test strategy

Use unit/property tests for pure normalization, limits, schema transitions and calculations.
Use real child fixtures for process behavior and a controlled HTTP server for API behavior.
Use disposable target services for effect/recovery cases. Use actual installed hosts for MCP grants
and tool behavior. Model evaluations supplement deterministic tests; they do not replace them.

Import the relevant Python helper cases as behavioral reference data, then add independent expected
results from the intended contract. Differential agreement alone can preserve a shared bug.
For intentional differences, document the reason and new consumer expectation.

Fuzz request parsers, frame boundaries, JSON depth, argument vectors, paths and extension output.
Target Windows Unicode/quoting, symlinks/reparse points, file replacement races, slow/noisy processes,
partial writes, non-UTF-8 output and interrupted artifact commits. No performance gate relies on a
single noisy workstation run. Measure process startup separately from command/API duration.

Security tests use synthetic secrets and disposable targets. Never probe live credential paths
for leakage or run arbitrary candidate code with a real operator account. Cross-host protection
claims require the actual isolation topology, not mocks of permission decisions.

## Evidence format and release judgment

Each acceptance packet names requirement/case IDs, exact revision and artifact digest, environment,
command/procedure, observed result, expected invariant, limitations and retained sanitized evidence.
Runtime case status is pass, fail, partial or unrun; evidence claims retain verified/sourced/unverified
labels. A partial live check cannot be converted to pass by a green structural suite.

A release requires the phase's cases, supported-host matrix, independent review, reproducible
artifact build, documentation examples, upgrade/rollback evidence and human acceptance of the
exact candidate. Publishing and production adoption remain separate decisions.

## Planning package checks

Before delivering this package:

1. Resolve every relative Markdown link in the package and its roadmap pointer.
2. Parse every JSON schema and example; check schemas against Draft 2020-12.
3. Validate each positive example and demonstrate rejection of the documented negative fixtures.
4. Verify every CAP-01 through CAP-25, REQ-01 through REQ-17, NFR-01 through NFR-12 and AC-01
   through AC-36 appears in its owning document and traceability tables.
5. Review phase dependencies, CLI examples, IDs, exit codes, limit values and lifecycle terms for
   contradictions. Check that no proposed feature is described as already installed.
6. Run this repository's specification verifier, including repository-document links, and inspect
   the final diff/status for unrelated changes.

These checks are documentation/specification evidence. They do not run the product, any live
Grafana call, agent campaign, plugin installation, remote job or live change.
