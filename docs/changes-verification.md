# Approved rule-file change verification — 2026-10-15

WORKBENCH-026 is accepted for the Linux x86-64 disposable-file scope in the
[change contract](changes.md). This is evidence for the named Prometheus-rule replacement consumer,
not full CAP-22 completion or authority to deploy changes to a live service.

The [retained packet](evidence/changes-2026-10-15/summary.json) contains raw installed receipts,
filesystem observations, test logs and the independent boundary review. The source manifest covers
2,449 files. Acceptance prose and evidence were added afterward without changing product bytes.

| Artifact | SHA-256 |
|---|---|
| Source manifest | `a4fd2c478c0510709c6961b6e244b7c9523808e5f9e9b897f82263b75102227c` |
| Ordinary installed CLI | `a90cc135646beeb38b8d7641d9a43e2e6577f002838013778876a1c3d2a6b98c` |
| Reviewed promtool | `c736d55d3ccd959fe48329965fb5cb671d45585a9483954766448035100ff75c` |

## Established behavior

| Boundary | Observed evidence |
|---|---|
| Approval and validation | Real pinned validator and mTLS requester/approver identities; self-approval, unrelated principal, wrong digest, no-op and modified originals refused; planning/approval preserve target bytes |
| Exact effect | Applied file matches prepared inode, bytes and metadata; duplicate delivery preserves that inode; recovery uses a new separately approved plan |
| Expiry and ownership | Restart loses unconsumed monotonic write authority; backward clock invalidates anchors; grant, policy, certificate, validator, ledger and implementation identities are bound |
| Filesystem preconditions | Tests reject stale original identity, matching content on a different inode, symlink/hardlink/FIFO, substituted parent, preparation and lease entries, unsafe permissions and oversized files |
| Concurrency | Two simultaneous apply calls produce one prepared inode; an overlapping call can receive explicit store-busy refusal; subsequent delivery returns the same attempt |
| Actual interrupted publication | Inotify observed consumed-intent publication; SIGSTOP/SIGKILL caught `dispatching` before target rename. Restart, status, reconciliation and repeated apply preserved the original inode and reported `not_applied` |
| Actual disk failure | Private 64 KiB filesystem exhausted during intent persistence. The target stayed unchanged, partial ledger bytes were retained, further operations refused, and restart preserved the bytes and refused recovery implicitly |
| Readback and response failure | Modified target reconciles to unknown; real ENOSPC while saving reconciliation cannot return historical verified state; incomplete validation receipts and mismatched peer identity are refused |
| Output and discovery | CLI, structured calls, discovery and text expose the same identities. Small output limits omit the plan as a whole with partial coverage and preserved digest. Lost replies have schema-valid uncertain receipts |

The installed mission passed **19 commands, with zero remaining test gaps**. Its exact lifecycle
observations are in [lifecycles.json](evidence/changes-2026-10-15/installed/lifecycles.json).
The workspace run passed **538 tests, zero failures, 33 ignored**. Ignored cases retain explicit
native, validator or small-filesystem requirements. Separate sandbox runs included the relevant
fixtures: **29 focused core cases** and **7 CLI cases** passed. After the final nullable lost-reply
schema correction, all **22 runner** and **25 MCP** schema-consumer tests passed, and the ordinary
binary was rebuilt before the installed mission. Formatting, strict workspace clippy and schema
verification passed; the latter covers 108 schemas and 135 fixtures.

Independent review closed three P2 findings: expiry after intent persistence, stale verified
reconciliation on target/persistence failure, and incomplete validation-receipt acceptance.
The ENOSPC reconciliation regression was demonstrated failing before its fix. No material scoped
finding remained after the focused closure review.

Broader checks also found missing propagation of the new schemas into MCP and runner validation.
The fixes embed the complete schema dependencies; runner schema preflight supports reference cycles
while checking unsupported sibling constraints, with runtime depth limits unchanged. Existing
receipt mutation and MCP discovery tests pass. Earlier failed checks and verifier runs are retained
in the packet and are not counted as accepted runs. The concurrency verifier was corrected to admit
the documented nonblocking store-busy refusal while requiring one verified replacement.

## Limits

All execution used the existing Bubblewrap boundary: private user/network/PID namespaces,
read-only source/toolchain, synthetic certificates and disposable files. No host listener, daemon,
live target, integration or service reload was configured. Changes remain local and uncommitted.

Distinct certificates establish configured principals, not independent human approval custody.
Unconsumed plans expire for writes across restart. Recovery writes need a new approved plan.
Private directories and leases coordinate cooperating writers; rename cannot provide compare-and-swap
against arbitrary malicious writers using the same account. No automatic pruning, takeover,
rollback of external effects, native Windows support or live cross-host conformance is claimed.
