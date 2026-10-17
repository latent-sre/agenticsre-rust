# Authenticated remote runner verification

WORKBENCH-023 is accepted for the Linux x86-64 isolated-process experiment described in
[the runner contract](runner.md). This supplies CAP-19/AC-23 fixture evidence. Native Windows,
live cross-host operation and a production deployment remain unverified. Changes remain local
on base commit `9016dcba4d42adbf3dd1972cb0f2dba6ed8fc664`.

## Frozen candidate

The [source manifest](evidence/runner-2026-10-11/source-manifest.json) covers 1,853 files.
The installed mission checked those bytes and all three executable hashes before and after
execution. Acceptance prose and this evidence packet were added afterward; product code was unchanged.

| Artifact | SHA-256 |
|---|---|
| Source manifest | `5342752de2cf2a321ac4bd11ac45e9ed97b86ab4fb178977a1eefdaf6f7804f1` |
| Ordinary `save` | `78f040b13a4998ac787e3876217437fab5165328cfe5c4e8a4e77b8ff4447c3a` |
| Capacity extension | `28061a389b8f28bdd6906a551ae3698291f1195101781c7b28dedf5712f035e8` |
| Adversarial extension fixture | `40b2e8f091744d8002d162ed437b8ae922ea97f2c9362127188ff4152cf0ac5f` |

See [installed results](evidence/runner-2026-10-11/verification.json),
[lifecycle observations](evidence/runner-2026-10-11/lifecycles.json),
[checks](evidence/runner-2026-10-11/checks.json),
[review closure](evidence/runner-2026-10-11/review.json) and
[artifact hashes](evidence/runner-2026-10-11/artifact-sha256.json).
Private keys, certificate payloads, packages and executable payloads are not retained in this packet.

## Evidence

| Requirement or risk | Direct evidence |
|---|---|
| Authenticated principals and runner | Real TLS 1.3 exchanges with mandatory client certificates, explicit CA/name validation, leaf pins and ALPN; missing, untrusted, expired, wrong-name, wrong-pin and unmapped identities are refused |
| Refusal before application delivery | Transport tests observe zero RPC bytes for a wrong server pin and no connection for expired or cancelled preflight; malformed lengths and slow partial frames remain bounded |
| Independent principal ownership | Two certificates can own the same request ID without sharing a job; one principal's missing lookup does not expose the other's result |
| Durable admission and policy | Identical bodies return the original job; conflicts are refused; task/source and extension package/policy/executable identities remain in the frozen binding; ordinary local workers cannot dispatch remote admissions |
| Lost reply and reconnect | A real TLS client sends submit and discards its acknowledgment; status alone retrieves the admission. Event gaps and subsequent cursors preserve monotonic reconnect behavior |
| Revocation and expiry | Restart with removed grants preserves historical owner lookup while denying new work; focused tests reject expired or changed dispatch scope without launching a child |
| Real supported consumers | Copied installed CLI runs the embedded error-budget task and the pinned native capacity extension through the runner; separate job runtime accepts one-hour metadata without widening foreground limits |
| Cancellation | A stubborn native extension receives explicit remote cancellation; the retained child receipt reports cancelled execution and confirmed forced cleanup |
| Connection loss and runtime | Principal B's discarded submit reply does not renew its 2,000 ms runtime; later status reports timed out with cleanup confirmed |
| Crash and restart | SIGKILL stops the real runner while a native child remains alive. Restart reports recovery-required, returns the same admission on duplicate submit, and neither replays the child nor signals a persisted PID |
| Partial persistence | A real private 64 KiB filesystem fills during runner work; the server reports unknown write effects. Later submission is refused, restart refuses the interrupted store, and all 24 remaining files retain their bytes |
| Startup and shutdown | Occupied-listener failure discloses retained namespace writes; restart can use them. SIGTERM interrupts a stalled connection and exits 143 within the tested bound; scan cancellation preserves the caller's signal |
| Public result contracts | Every installed receipt passes canonical and runner schemas. Discovery advertises unclassified submit effects. Text agrees with job identity/state. A 1,024-byte detail budget omits the child result explicitly, retains its digest and does not skip undelivered events |
| Malformed authenticated responses | Fixed embedded schemas and semantic correlations reject invalid effects, nested receipts and mismatched bindings before importing peer data. Unsupported schema constructs also fail closed inside negative and conditional branches |

The installed mission passed **76 commands**. The full workspace passed **464 tests, zero failures,
26 ignored**. Ignored tests retain their existing explicit native-environment requirements.
The workspace includes 19 focused runner tests, the remote-job lifecycle regression and seven
public remote CLI tests. Formatting, strict all-target/all-feature clippy, ordinary CLI build
and specification checks passed.

Independent source review closed four P2 findings: unchecked peer receipts, startup write effects,
shutdown cancellation classification and uncertain admission after partial writes. No material
finding remained. Integration also corrected controller I/O cancellation, expired preflight
connection creation and unsupported-schema handling. Final workspace and installed checks cover
the small equivalent lint edits made after review. The installed verifier initially needed an
explicit empty OpenSSL configuration inside the sandbox; that fixture correction changed no Rust
product bytes and the final mission passed.

## Boundaries and remaining work

All executable checks used `tools/test-sandbox.sh`: private user/network/PID namespaces, read-only
source/toolchain, disposable files and synthetic credentials. No host listener, live target or
integration registration was created. Ordinary operator authority and process-group limitations
from the accepted job/extension contracts still apply.

The crash test retains a live pidfd while verifying that the fixture belongs to its runner. It uses
that handle only for test cleanup after proving restart leaves the old child alone. This is not
product reconciliation or permission to terminate an unknown historical process.

The fixed queue expiry is not a renewable distributed lease or fencing implementation. A timeout,
partition or runner death does not prove an external effect stopped. Handshake lists configured
grants; admission and dispatch validate actual installed bindings. A complete installed-capability
inventory, version compatibility matrix, certificate rotation/revocation rollout, additional
consumers and platform/live conformance remain separate increments. Server grant changes require
restart. Fleet aggregation, persistent scheduling and controlled-change approval/reconciliation
retain their own contracts and evidence requirements.
