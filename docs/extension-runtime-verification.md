# External protocol and fourth-capability acceptance — 2026-10-11

**Verdict:** PASS for WORKBENCH-021's Linux x86-64 operator-local external execution and
capacity-projection experiment, covering the implemented CAP-17/AC-26/AC-29 boundaries. The
[runtime contract](extension-runtime.md) builds on [inert package acceptance](extensions-verification.md).
Broader platforms, hosted grants, untrusted-code isolation, artifact exchange, automated store
repair and distribution trust remain on [the roadmap](roadmap.md).

Final frozen source: 1435-file manifest SHA-256
`6525fe4d821fa43966450d6b9e12d60487cb25e952eff532ede6822c3dc38f9e`.
Ordinary CLI binary SHA-256:
`59156778f2602f7c4c29b1f9a81d579b948cd1ca148693aa9562f1d41fc197af`.
Independent capacity projector SHA-256:
`2c10ab896d97f7c27303862ba24dd93bb609e8c3e010d9fe791c514b1b616107`.
Adversarial test peer SHA-256:
`9199dbb8894ce506fab6bb45444498ba60d1222e024b0b7f1ec65ea79002bd57`.
Work remains local and uncommitted. Acceptance documentation follows the source freeze; tested
product bytes remain unchanged. Test policies/installations existed only in disposable namespaces.

The packet retains the [installed verdict](evidence/extension-runtime-2026-10-11/verification.json),
[source manifest](evidence/extension-runtime-2026-10-11/source-manifest.json),
[check record](evidence/extension-runtime-2026-10-11/checks.json),
[workspace output](evidence/extension-runtime-2026-10-11/workspace-r1.log),
[package identities](evidence/extension-runtime-2026-10-11/package-sha256.json),
[closed-consumer observation](evidence/extension-runtime-2026-10-11/closed-consumer.json) and
[artifact hashes](evidence/extension-runtime-2026-10-11/artifact-sha256.json).
The 45 installed CLI commands and three direct peer compatibility checks retain exact streams.

## Demonstrated behavior

| Requirement or risk | Evidence |
|---|---|
| Real fourth capability | A separate `capacity-projector` executable, with no workbench-core dependency, performs capacity projections through the installed-package protocol. CLI and structured calls agree. Tests cover normal, already-full, zero-growth, subday and maximum-safe-integer inputs, with explicit supplied-data assumptions. |
| Independent authorization | Installation without policy cannot dispatch. Private policy pins exact package bytes and capability names. Wrong pins, public policy permissions, wire grants, invalid inputs and unsupported operation versions refuse before child launch. A newer installed version cannot replace the one authorized pin; two authorized versions refuse ambiguity. |
| Identity and discovery | Retained executable bytes are copied to an unlinked read-only descriptor. Core tests check content independence and absence of writable handles/names. Store-byte replacement is detected. Effective inventory/discovery distinguishes installed from granted and classifies external effects as unclassified. The advertised output schema validates the actual public receipt. |
| Strict protocol | Native adversaries exercise unsupported versions, wrong correlation, invalid JSON/UTF-8, oversized frames, wrong sequences, early/duplicate/trailing terminals, missing terminal, excess progress, invalid output data, partial trailing frames, duplicate keys and positional arrays. None produces success. |
| Process truthfulness | A valid terminal followed by exit 7 remains failed with exit 7 preserved. A peer-reported failure remains failed. Truncated stderr produces partial status; the 1024-byte detail budget is independently recomputed from retained values. Effects stay unknown after launch. |
| Timeout and cancellation | Timeout remains timed out despite termination-induced protocol EOF. A peer ignoring TERM is killed after the shared grace period and reaped; the host returns 143 for the caller's TERM, retains SIGKILL as the child signal and confirms cleanup. Closing the result consumer also terminates that peer and leaves no live PID. |
| Compatibility | The real version-1 peer refuses future protocol version 2 and an array hello before responding. It returns an explicit failed terminal for future operation version 2. The host refuses unsupported request versions before dispatch. |
| Installed boundaries | An ordinary copied CLI runs outside the checkout in the existing private namespace. All populated receipts/data and discovery descriptors validate. Source, CLI, both peer binaries and package input hashes match their guards afterward. |

## Checks and resolved findings

The full workspace suite passed **419 tests, zero failures**, with 25 native/fixture cases ignored.
That run preceded two equivalent lint normalizations and the final discovery metadata correction.
After those respective changes, all six consumer tests, all four native CLI tests and the targeted
discovery regression passed. Final installed verification exercised the frozen corrected binary
across 45 commands, three peer compatibility checks and closed-consumer cleanup. Existing generic
process, task and hosted-adapter checks exercise the shared supervisor's unchanged default path.

Strict workspace lint, formatting, ordinary build, whitespace and specification checks passed.
The specification suite covered 68 schemas and 46 positive/35 negative examples. The new local
workspace member reuses locked dependencies; no existing external dependency version changed.
The standalone development packaging helper was also exercised without executing its payload.

Independent review found and closed the peer's positional-array admission gap: explicit object
checks now precede strict deserialization of original bytes, preserving duplicate-key rejection.
Host-runtime review found no material issues. A final coordinator contract audit then demonstrated
that discovery advertised the flat peer payload instead of the canonical public receipt schema.
The initial advertised schema and actual receipt, failing replay and original source manifest are
retained under `initial-discovery-contract/`. The metadata correction preserves the separate peer
output validator; focused and final installed regressions prove closure. Review confirmed it
statically. No material finding remains; reviewers did not run executable checks.

## Extension design evidence and limits

The independent capability added one executable crate and a package manifest. Host changes were
limited to extension admission/execution/discovery, public dispatch/rendering, CLI configuration
and a private optional dialogue interface in the existing process supervisor. Generic commands
retain closed stdin; protocol streams use nonblocking stdin without a second lifecycle manager
or detached reader thread. Existing Prometheus binding bytes were not refactored.

Installation still creates no execution grant. `operator-reviewed-v1` uses ordinary account
authority, not filesystem/network containment. It requires separately reviewed code and an
explicit policy. Same-account/root tampering and deliberately escaping processes remain outside
the demonstrated boundary. Local namespace tests do not establish live deployment, signing,
revocation, distribution clearance or native support elsewhere. The broader product backlog
remains active; durable jobs are next.
