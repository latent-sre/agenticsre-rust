# Durable local jobs verification

WORKBENCH-022 is accepted for the Linux x86-64 operator-local scope described in
[the jobs contract](jobs.md). This is evidence for CAP-18/AC-22 local lifecycle behavior, not
remote execution, Windows support, automatic target reconciliation or a production release.
Changes remain local on base commit `9016dcba4d42adbf3dd1972cb0f2dba6ed8fc664`.

## Frozen candidate

The final [source manifest](evidence/jobs-2026-10-11/source-manifest.json) covers 1,589 files.
The installed mission checked those bytes and all three executables before and after execution.
Acceptance prose and the evidence packet were added afterward; product code was unchanged.

| Artifact | SHA-256 |
|---|---|
| Source manifest | `4494d3f091296633024dc323a01eadd4f624311ed2a472eb1bb66f015140e969` |
| Ordinary `save` executable | `f461f116855124ef9f12171d7b4560f153fea5ee15a26f9531357ac7206462c1` |
| Native runner fixture | `968d4cd8902d5a886c8db1696d818f1539bf2ea68972996cd338dc50d7366277` |
| Capacity extension | `28061a389b8f28bdd6906a551ae3698291f1195101781c7b28dedf5712f035e8` |

The fixtures are verification inputs, not installed services. No executable or package payload is
stored in the evidence packet. See [artifact hashes](evidence/jobs-2026-10-11/artifact-sha256.json),
[check results](evidence/jobs-2026-10-11/checks.json) and
[installed command results](evidence/jobs-2026-10-11/verification.json).

## Evidence and limits

| Requirement or risk | Direct evidence |
|---|---|
| Submission does not execute; duplicates never redispatch | Installed marker remains absent while queued; identical submissions return the same handle before/after completion and at the 64-job cap; conflicting requests are refused |
| Real supported consumers | Installed native child, fixed error-budget task and exact-policy capacity extension complete through jobs |
| Separate runtime and authority | One-hour job receipt/protocol accepted; ordinary one-hour request and lower host-ceiling violation refused; changed extension policy refuses child dispatch |
| Ownership and cancellation | CLI tests refuse a simultaneous second worker while status/cancel remain available; queued cancellation never starts the child; running cancellation yields observed cleanup and a retained cancelled receipt |
| Reconnect and retention | Two-event retention discloses a gap; resuming after the returned cursor yields no duplicates; invalid future cursor is refused |
| Worker result consumer disappears | Installed worker exits 1 on delivery failure while the child completes and a later status read retrieves its committed result |
| Worker dies during execution | SIGKILL targets the known live worker; its bounded child completes naturally; subsequent reads and workers report recovery-required and never redispatch or signal a stored PID |
| Published result survives interrupted terminal commit | A crash snapshot is reconstructed from a real completed child's persisted receipt and preceding finalizing state; cancellation is recorded, then a new worker adopts the same digest and success outcome without replay |
| Interrupted pre-dispatch cancellation | A test-only fault after finalizing-state publication preserves attempted write effects and reconciles to recovery-required without child dispatch |
| Partial durable writes | Real private 64 KiB tmpfs ENOSPC leaves five store files; subsequent lookup refuses recovery-required and preserves every byte; atomic-publication unit faults also preserve the old state and staging |
| Capacity and output bounds | 64 admissions fill the store; the next is refused before a write while duplicates still resolve. Real noisy/control-byte children retain partial outcomes, truncation and byte-identical bounded receipts across lookup |
| Corrupt or contradictory records | Focused tests reject sequence overflow, invalid timestamps/identifiers/digests, impossible phases and disagreement between state and the immutable child result |
| Public contract | Every installed JSON receipt is schema checked; text lookup agrees with the retained job; discovered submit output schema validates the actual receipt; worker effects are unclassified |

The final mission passed **105 installed commands**. Its finalization case reconstructs a precise
persisted crash snapshot; it does not claim to have timed SIGKILL between those two filesystem
commits. The separate execution-crash case does kill a real worker during a real child's lifetime.

The full workspace passed **437 tests, zero failures, 26 ignored**. The ignored tests retain their
explicit native-environment requirements. Separately, the native extension job test passed, as did
three existing foreground extension protocol regressions. The workspace includes nine focused
job storage/lifecycle tests, seven ordinary job CLI tests, the discovery effect classification test,
and seven capacity-peer tests. Formatting, strict all-target/all-feature clippy and the ordinary
CLI build passed. Logs are retained in the packet.

The native job-extension test preceded an equivalent discovery conditional cleanup and refinement
of a sequence regression fixture. The final workspace covers those edits, and the frozen installed
mission again exercises native extension jobs. Adding the finalization crash-snapshot mission
changed only the verifier after those Rust gates; no product or Rust test bytes changed.

## Review and boundaries

Independent source review found and closed six P2 issues: sequence arithmetic, persisted metadata
and phase validation, state/receipt agreement, cumulative write-effect reporting, interrupted
queued-cancel finalization recovery, and bounded failure responses. No P0/P1 or unresolved material
source finding remained. Additional integration checks cover explicit null nested controls and
truthful outer recording consent while the nested operation retains `record=never`.

All product/test execution used `tools/test-sandbox.sh`: private user/network/PID namespaces,
read-only source/toolchain, writable target output, private temporary files and no host credentials.
The store and runtime use ordinary operator authority. Native executable/model paths retain their
documented mutable-path semantics; extension package/policy identities are checked on the actual
selection used for launch. Process groups do not contain deliberately escaping programs.

The store is bounded and does not delete admission records, replay interrupted effects or repair
corrupt writes automatically. Target-specific reconciliation, automated retention/migration,
additional consumers, hosted grants, remote ownership and native Windows remain open requirements.
