# Bounded multiple-target verification

WORKBENCH-024 has Linux x86-64 isolated-process acceptance for the [fleet contract](fleet.md),
supplying CAP-20/AC-30 evidence. The supported consumer is the installed error-budget calculator
over supplied measurements. This does not establish live telemetry collection, target health,
native Windows support or production fleet operation. Changes remain local on base commit
`9016dcba4d42adbf3dd1972cb0f2dba6ed8fc664`.

## Frozen candidate

The [source manifest](evidence/fleet-2026-10-12/source-manifest.json) contains 2,072 files.
The installed mission checked those bytes and the ordinary CLI before and after execution.
Acceptance prose, the verifier's documentation list and this evidence packet were updated afterward;
product, test and installed-verifier bytes remained unchanged.

| Artifact | SHA-256 |
|---|---|
| Source manifest | `59feef6e4b5b8d31cf5854e5deeafd48aa3536cdf8fa63adaf004d5994822903` |
| Ordinary `save` | `cebf539b6fe67492666afab255987d75db7f73be8e6f4c8d4b3c1d513f2c684e` |

See [installed results](evidence/fleet-2026-10-12/verification.json),
[lifecycle observations](evidence/fleet-2026-10-12/lifecycles.json),
[checks](evidence/fleet-2026-10-12/checks.json),
[review closure](evidence/fleet-2026-10-12/review.json) and
[artifact hashes](evidence/fleet-2026-10-12/artifact-sha256.json).
The packet retains no private key, certificate payload or executable.

## Direct evidence

| Requirement or risk | Observed behavior |
|---|---|
| Real installed path | Ordinary copied CLI, unrelated working directory, two independent mTLS runner processes and real remote task execution |
| Explicit selection | Empty, duplicate and unknown targets refuse without effects; selected unreachable runners remain visible alongside successful targets |
| Immutable parent | Identical requests return the same parent and child jobs; changed selection conflicts; replacing current inventory cannot alter historical target rows |
| Honest aggregate | A successful child plus a real 1 ms runtime timeout produces partial status; no health conclusion is inferred from calculator values or management RPC success |
| Admission bounds and fairness | Concurrency-one failure threshold stops the second admission; persisted ordinals rotate the first target across repeated collections; all target rows remain present |
| Output and discovery | Installed receipts pass canonical and fleet schemas; text retains parent/target/outcome identity; a 1,024-byte detail budget omits whole optional receipts while retaining digests and evidence references |
| Controller death | After a real child admission, stop the owned runners, SIGKILL the controller, resume runners and recover using original request IDs; remote job sets do not grow |
| No replay protocol | A focused controller peer records only `status` during resume, including an original uncertain intent and a never-submitted queued target; job-count comparisons alone are not treated as proof of no submit RPC |
| Authority drift | Profile-byte drift refuses recovery; remapping the same client certificate to a different principal returns the server's precondition refusal and creates no job in that namespace; restoring the original owner permits same-identity recovery |
| Refusal preservation | An authenticated authority refusal stops repeated polling for that target in the current call, remains durable, and leaves the child unknown; a later explicit recovery call can retry observation |
| Cancellation | Active cancellation is committed while runners are stopped; the controller observes it after they resume and accounts for every target; a store-lock regression prevents admission when cancellation committed first |
| Deadline | A stopped runner and 300 ms global budget return within four seconds including bounded cleanup; unresolved work cannot become aggregate success |
| Persistence integrity | Partial store additions are refused without changing retained bytes; real private 64 KiB ENOSPC reports unknown effects and preserves interrupted files; focused tests reject mutated receipts and invalid terminal evidence |

The installed mission passed **26 commands**. The final workspace passed **484 tests, zero failures,
26 ignored**. Existing ignored cases keep their explicit native/fixture requirements. The workspace
includes nine fleet core regressions, eight public fleet tests and eight public runner tests.
Formatting, strict workspace/all-target/all-feature clippy, ordinary CLI build and specification
checks passed. Exact commands, exit statuses and logs are retained in the checks packet.

Independent review closed two P2 fleet defects: cancellation committed before child intent could
still allow admission, and a retained receipt digest was not recomputed on load. Prerequisite
review removed a controller-local Python dependency and corrected reported effective RPC timeouts.
No material finding remains in the reviewed boundary.

Integration also exposed unsupported `dependentRequired` validation in legitimate calculator
results, delayed profile-drift recovery, and loss of durable parent metadata on a persistence error.
Focused regressions cover the corrections. Installed execution then exposed job preflight expiry
misreported as a binding change and repeated principal-refusal polling that erased the useful
diagnostic. Both were fixed, independently reviewed and covered by focused regressions plus the
final workspace and installed mission. The fixture watchdog was corrected to include the requested
operation deadline and cleanup allowance. Earlier failing logs and receipts remain in the packet;
they are not acceptance results.

## Boundaries

All executable checks used `tools/test-sandbox.sh`, with private user/network/PID namespaces,
read-only source/toolchain, disposable stores and synthetic credentials. No host service, live
target or integration registration changed. SIGCONT/termination only affect test-owned processes.

The controller's deadline limits admission and observation. Remote runtime starts at dispatch,
and the runner's fixed queue expiry is separate; neither establishes that external effects stopped.
The whole cleanup allowance is 2,500 ms, with time reserved for persistence. A partition or identity
drift can retain an unresolved child and a recoverable parent. Recovery never submits another child,
extends its runtime or takes over an old attempt.

Store capacity is bounded to 32 parents and 64 MiB, with a per-parent reservation before intent.
There is no automatic repair, eviction, retention migration or arbitrary selector expansion.
Additional observational consumers, live inventory integration, hosted UI/MCP grants, cross-platform
operation and load/soak coverage beyond these bounded fixtures remain separate work. Persistent
scheduling and controlled changes keep their own ownership and authorization contracts.
