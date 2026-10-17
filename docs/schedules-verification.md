# Persistent observational schedule verification

WORKBENCH-025 is accepted for the Linux x86-64 isolated-process scope in the
[schedule contract](schedules.md), supplying CAP-21/AC-31 evidence. The supported operation is the
fixed error-budget calculator over supplied measurements, dispatched through authenticated fleet
runners. Live scheduling, notifications, native Windows and production deployment remain unverified.
Changes remain local on base commit `9016dcba4d42adbf3dd1972cb0f2dba6ed8fc664`.

## Frozen candidate

The [source manifest](evidence/schedules-2026-10-12/source-manifest.json) covers 2,220 files.
The installed mission checked these bytes and the ordinary CLI before and after execution.
Acceptance documentation and this packet were updated afterward without changing product, test or
installed-verifier bytes.

| Artifact | SHA-256 |
|---|---|
| Source manifest | `586737a67829564953c9264226e20d2b8097b4efa551a847ef5ed63b05ea58d8` |
| Ordinary `save` | `e36a390bbd1d71fc2e093e6a68415de97f6ee5a0b85726a656ad66c46ec0b288` |

See [installed results](evidence/schedules-2026-10-12/verification.json),
[lifecycle observations](evidence/schedules-2026-10-12/lifecycles.json),
[checks](evidence/schedules-2026-10-12/checks.json),
[review closure](evidence/schedules-2026-10-12/review.json) and
[artifact hashes](evidence/schedules-2026-10-12/artifact-sha256.json).
Private keys, certificate payloads and executable payloads are not retained.

## Direct evidence

| Requirement or risk | Evidence |
|---|---|
| Public management | Copied installed CLI creates, lists, reads, previews, updates and pauses definitions, queues manual work, requests cancellation and runs an explicit worker; each JSON receipt passes the canonical and schedule schemas |
| Explicit worker authority | Generic structured `schedule.work` is refused; the operator CLI invokes the separate worker entry, with explicit startup stores and no service installation |
| Revision and generation | Stale update CAS refuses; definition updates increment generation and preserve history; repeated manual request IDs return the original occurrence even after update |
| Real consumer | Two independent synthetic mTLS runners execute the installed calculator and return successful child outcomes under the frozen fleet request |
| Calendar | Nine injected-clock cases cover fractional anchored intervals, inclusive grace, forward/backward jumps, spring gaps, earlier repeated-time selection, a half-hour gap, a skipped civil date and UTC range boundaries |
| Misfires and overlap | Persisted automatic rows expire at locked admission; paused automatic rows cannot block explicit manual work; unresolved older generations prevent overlap; compressed skips and cursors survive clock changes |
| Fairness and identity | Persisted worker ordering advances past repeatedly unresolved schedules; distinct schedule instances cannot produce the same automatic fleet request merely by sharing name/generation/due ordinal |
| Process ownership | Store leases reject concurrent workers. SIGKILL after real remote admission preserves the original fleet request; another fleet store is refused without creation, and recovery produces no extra child jobs |
| Pause versus cancel | Lock-based regressions make pause win before intent publication. Installed cancel-active reaches an existing fleet controller, records cancellation and leaves the undispatched target skipped |
| Worker shutdown | SIGTERM returns 143 with signal metadata, leaves the fleet's cancellation flag false, and later recovery retains the same request and child job set |
| Authority drift | Changed profile pin or malformed private key causes a definitive fresh refusal with no fleet admission; restoring credentials does not replay that manual ID, while an explicit new manual request succeeds |
| Honest uncertainty | A fresh receipt proving no admission becomes resolved `failed`; missing results after uncertain intent remain `unknown` and block overlap. Both failed and unknown worker results have canonical diagnostics |
| Integrity | Altered retained receipts, mismatched aggregates and false terminal states are rejected. Omitted full receipts retain a digest-bound minimal observation rather than uncorrelated success fields |
| Quotas | A one-occurrence limit persists through definition updates and preserves deduplication history. A near-capacity unit case proves reserved receipt and metadata growth allows terminal publication |
| Partial persistence | Unknown files are refused without modification. A real private 64 KiB filesystem fills during schedule creation, reports unknown effects and preserves interrupted bytes on subsequent refusal |
| Human output and bounds | Text preserves canonical refusal diagnostics and previews both UTC/local times with the rule version. Bounded JSON omits optional receipt detail while preserving occurrence identities and digests |

The installed mission passed **76 commands**. The full workspace passed **508 tests, zero failures,
26 ignored**, including 24 scheduler tests. Ignored tests keep their existing explicit native or
fixture requirements. Strict all-target/all-feature workspace clippy, formatting, ordinary CLI
build and specification checks passed. Exact logs and counts are retained in the checks packet.

Independent review closed seven P2 findings: stale queued admission, a paused row blocking manual
work, permanent blockage after definite refusal, receipt/summary mismatch, cross-store occurrence
ID collision, missing canonical outer diagnostics and incomplete metadata quota reservation.
Additional corrections cover fair processing, cancellation between intent and fleet admission,
and human output. No material finding remained in the reviewed scope. Small equivalent lint edits
and the text renderer correction preceded the final installed run and full regression suite.

The first installed run completed its scenario assertions but failed the verifier's final binary
hash check because a local dictionary shadowed its executable path variable. The verifier-only
correction changed no Rust bytes; the final run passed all checks. Its earlier failure log is
retained and is not counted as a passing mission.

## Limits

All executable verification used `tools/test-sandbox.sh`: private user/network/PID namespaces,
read-only source/toolchain, disposable stores and synthetic credentials. No host listener, service,
live target, notification destination or integration registration was configured.

The bundled timezone rules are explicitly **IANA 2025b**, supplied by pinned Chrono-TZ 0.10.4.
Definitions bind that version; preview does not claim later political rule changes are included.
Native Windows, other deployment environments, timezone-rule upgrade/migration and production
clock synchronization remain separate acceptance requirements.

`retention_limit` is a hard ledger cap, not automatic expiry. No eviction or repair can erase
uncertain occurrences or deduplication identities. The scheduler's bounded store does not reclaim
the independent fleet store's 32-parent capacity. Automated retention/migration and broader load
or long-running operational trials remain open. Scheduling installs no daemon; an explicit worker
performs a bounded pass. Notifications and additional consumers need their own bindings and grants.

Worker completion is distinct from child success: a terminal fleet failure remains visible in the
occurrence's `fleet_aggregate`. A scheduler crash before a persisted intent reaches fleet admission
can leave that intent unresolved indefinitely. Recovery does not infer permission to replay from
`not_found`, elapsed time, restored credentials or a new definition generation.
