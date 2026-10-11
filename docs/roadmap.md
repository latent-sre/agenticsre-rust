# Product roadmap

**Status:** Live product work queue, established 2026-10-02. The owner selected
latent-sre/agenticsre-rust as the working repository. Scope and dependencies live in the
[planning package](sre-workbench/README.md); this file owns current execution status.

**Completed three-hour build session:** The owner resumed work from pushed checkpoint `719fee3` and authorized
a local review commit at the deadline. Start: **2026-10-02 22:17:57 UTC** (17:17:57 America/Chicago).
Stop/commit deadline: **2026-10-03 01:17:57 UTC** (20:17:57 America/Chicago). Reserve the final
15 minutes for cleanup, evidence and the commit. New full Cally jobs must start before
**00:42:57 UTC**, and all executable work must end by **01:02:57 UTC**, leaving cleanup margin.
Shorter late checks require their own bounded duration; do not start a job that can overrun the deadline.
Finish the remaining MCP acceptance, then build the Linux development bundle. Keep passed checks,
resolved product findings and existing execution boundaries. This session authorizes committing
the review checkpoint, without a new push or live installation.

**Review checkpoint:** MCP fixture acceptance, Linux/stateless development packaging and offline
core/CLI context resolution are complete for their stated slices. Native product execution ended
by 00:45:39 UTC; cleanup, sealed evidence and final documentation checks passed before the reserved
commit period. Final checks covered 25 schemas, 24 positive/11 negative fixtures and 25 repository
documents; 197 product/assets/schema/tooling files matched the approved R1 capture. The local
three-hour checkpoint was committed locally as `837c217838c7e6950e59f8e143daf60f4fe9615d`
at 01:18:53 UTC, with a clean working tree, then paused for owner review.

**Resumed work:** The owner requested continuation after that checkpoint. The earlier job and
commit cutoffs are historical; no new timed session or publication boundary was requested.
The selected increment is a context-containing Linux development bundle from committed source
`837c217`, with a new source/build/archive identity and installed mission evidence on Cally.
Reuse accepted product reviews and runtime checks where source bytes are unchanged. Preserve both
earlier archives for upgrade/rollback. On 2026-10-03 the owner requested committing this evidence
and opening a pull request. The review branch `codex/context-linux-bundles` targets `main` and
includes the unpublished implementation checkpoint `837c217` plus the installed-bundle evidence.
The actual archives remain local data artifacts; live installation and release remain separate.

**Current continuation:** The owner reported merging PR
[#2](https://github.com/latent-sre/agenticsre-rust/pull/2). GitHub readback confirms MERGED at
2026-10-03 03:11:46 UTC, with current `main` at `9f77f8a9c24b0c83aa9a574e831751fa7e84a17f`.
Its tree `416f88984e9ce24b6592de686a306b6c6cb4ab2a` exactly matches the reviewed `2c4e422`
branch. The merge changes history without changing accepted file bytes; existing Rust/specification
and installed-bundle evidence retains its original identity. Earlier open-PR entries are historical
and the owner-review item is closed. The next publication checkpoint must include merged-main
history; no extra product verification is required solely for this identical-tree merge.
The Python/Go language discussions authorize no migration. Continue the Rust checkpoint and
preserve its API/schema boundaries so a future language evaluation can reuse the same acceptance
missions and browser assets. The active goal continues in the separate implementation worktree
`/home/hawkfire/rusty/agenticsre-rust-diagnostics`, branch `codex/service-diagnostics`, based on
`2c4e422`. This worktree's roadmap is the sole live record for the new increment. Local changes
and isolated Cally checks remain authorized; no further commit/push/merge/live action is inferred
from the completed PR request. The historical three-hour cutoffs do not apply to this continuation.

**Owner pause checkpoint:** The owner requested a stop after the browser diagnostics milestone.
WORKBENCH-010 core/CLI and WORKBENCH-011 browser diagnostics have qualified native acceptance;
both affected browser reviews accepted R1, final API/Playwright execution and exact cleanup passed.
All run-owned jobs/resources are stopped. Source and evidence are saved locally in the diagnostics
worktree; no new commit or push was requested. WORKBENCH-012 has a prepared contract only and has
not begun implementation. Resume with installed saved queries, preserving the named resolver,
platform/live, release and broader product requirements below.
Final pause checks passed: 29 schemas, 27 positive/12 negative fixtures, 15 package documents plus
31 repository documents, all planning IDs and `git diff --check`. The diagnostics worktree remains
on `codex/service-diagnostics`, HEAD `2c4e422`, with the completed implementation/evidence saved
as local uncommitted changes. No builder or verifier task is running.

**Owner-authorized local cleanup:** Unused caches from other projects were included. Native UV
pruning removed 24 MiB of dangling entries; the separate Dualla cache had no unused entries.
The expired Trivy database/scan cache and regenerable node-gyp, pip and Vitest caches were cleared.
Two identical readonly binary/source-archive copies now share storage, preserving every logical
path, byte, size and mode; inode/link-count/ctime/shared-timestamp changes are recorded explicitly.
Measured free space rose from 8,929,361,920 to 10,504,310,784 bytes, a 1,574,948,864-byte gain.
All 688 repository paths/refs/status, 1,374 shared browser-runtime files and 181 retained proof
artifacts matched after cleanup. Rust/dependency caches, shared Python/browser environments,
uncommitted work, screenshots and unique evidence remain. Record
`/tmp/agenticsre-local-cleanup-gq_xicdc.json`, SHA-256
`38a28c3a4bc096384ac2c0ed3fec4b47970f071ea9143457d904cfa23e29579b`.
This entry is the only subsequent intentional source-document change; product work stays paused.
The next Trivy scan requires refreshing its removed database.

## WORKBENCH-001 — bootstrap the product

**Status:** Local PoC complete and independently verified. Broader product implementation continues.
**Owner:** Codex coordinates implementation and evidence; human owner retains live adoption/release decisions.
**Outcome:** Establish this repository as the product home, preserve the complete plan and its
provenance, and make the first command-runner slice concrete and reviewable.
**Next action:** The selected browser PoC and bounded MCP fixture slice are complete. Use the
documented launch workflows; Linux development packaging includes the independently accepted
offline context core/CLI. Continue toward P3 diagnostic/query/capture/compare fixtures. Reuse the accepted
core and Cally tooling; rerun affected checks for subsequent changes. Native Windows and full P1
acceptance remain open.
**Evidence:** [Import record](plan-import.md), [capability specifications](sre-workbench/capabilities.md),
[delivery plan](sre-workbench/delivery.md), and the local specification checks.
**SRE task:** Run the same useful operational checks directly or through an agent, with explicit
targets, dependable results and extensible capabilities.

All 25 capabilities remain in the product scope. Their P0 through P8 sequence is described in
the delivery plan. Individual implementation items are added here when selected; planning an
integration does not imply it is installed, authorized, connected or operational.

## Current execution

| Item | Status and evidence | Next boundary |
|---|---|---|
| Plan review / WP-01 | Reviewed against product, contracts, delivery and acceptance catalog by an independent design reviewer; local decisions recorded in [PoC contract](poc.md) and decision register | Build under existing authorization; target/naming decisions block only dependent live/distribution work |
| WP-02 through first WP-04 increment | Complete for Linux PoC: independent review APPROVE and executable verification PASS; 19 Rust tests, 77 independent CLI invocations and 14 schema cases. Review P2 late-signal mapping and verifier-found Cargo default were fixed | [Acceptance record](poc-verification.md); preserve broader platform/policy gaps |
| Specification baseline | Grafana snapshot passed 20 schemas, 21 positive and 9 negative fixtures, all IDs; Python 3.14.7 and dependencies pinned | Validate changed documentation at integration boundary |
| Cally assistance | Grafana snapshot passed 68 Rust tests and 96 HTTPS cases; minimal lane passed 17 native HTTPS tests without Python and refused both Python tasks. Both containers removed | Reuse bounded lanes for material changes; [acceptance record](grafana-verification.md) |
| Adapter baseline | Dashboard, error-budget, native Grafana fixtures, bounded stdio MCP and offline core/CLI context accepted; the context-containing Linux archive passed installed missions | P3 investigation fixtures; native Windows and live/installed-host acceptance remain separate |
| Hosted CI | Completed checkpoint `93fc339` and prerequisite repair `7b6aed1` pushed to main; [Rust PoC](https://github.com/latent-sre/agenticsre-rust/actions/runs/37041979990) and [specifications](https://github.com/latent-sre/agenticsre-rust/actions/runs/37041980033) both passed on the repair commit | Owner requested a PR for the implementation checkpoint and bundle evidence; native/Cally acceptance is retained separately, with no new hosted result claimed |

Execution authority: local source changes, disposable checks and bounded Cally build assistance
are authorized. On 2026-10-02 the owner explicitly requested committing and pushing the completed
implementation checkpoint. Paid model campaigns, live integrations and production changes retain
their separate authority boundaries. Original review and executable verification bind to snapshot commit
`c469973422cac1745ce35ee3c3502ef91395a247` in a disposable repository, not a product commit.
Review/fix rounds used: 1 (resolved); incomplete builder returns: 0. All remaining P1–P8
capabilities, native Windows evidence and live pilot acceptance stay open until actually verified.

## WORKBENCH-002 — fixed offline tasks and adapter foundations

**Status:** Dashboard increment complete: independent review APPROVE and execution PASS;
[acceptance record](dashboard-verification.md) binds 34 tests, public contracts, 36 independent
boundary cases, 25 protocol cases and both Cally lanes to frozen source. Error-budget increment also
complete: [independent review and verification](error-budget-verification.md) passed 46 Rust tests,
96 public CLI results, 48 independent numerical cases and 27 protocol cases. Both Cally lanes passed.
**Completed corrections:** Decimal threshold comparison fixes the upstream exact-boundary case;
accurate float roundtrips preserve normalized input identity; explicit object-shape guards reject
positional arrays before dispatch. Reported arithmetic remains binary64. Shared supervisor unchanged.
**Outcome:** Deliver dashboard-hygiene findings and error-budget calculations with immutable installed
bindings, runtime discovery, bounded execution and explicit unsupported/missing-dependency results.
Preserve the helper contracts while documenting the numerical correction and stronger input guards.
**Scope:** Initial CAP-04 and CAP-13, AC-04/11/19 subsets. Use [the pinned adapter baseline](adapter-notes.md),
existing task/result schemas and the verified supervisor. Extend discovery and the CLI through the
same core; introduce no arbitrary task registration, shell source or live target selection.
**Then:** Native Grafana fixture adapter; restrictive command profiles
and stdio MCP parity. Windows work can proceed when a native test environment is available. Keep
the remaining P1–P8 capabilities in their dependency order, and separate fixture from live acceptance.
**Task baseline:** Accepted task snapshot is `87f5162cf256e53e39dbf644af4dbc03ef6866d9`;
the verification snapshot predates the product implementation commit. Preserve its acceptance and
rerun affected checks for new work.
No approval is needed for the next authorized local implementation slice; live/publication boundaries
remain separate.

## WORKBENCH-003 — native Grafana fixture adapter

**Status:** Complete for controlled fixtures. Correctness/security review APPROVE; independent
scoped R2 execution PASS with unchanged R1 evidence explicitly retained. Final builder and Cally
full suites passed 68 Rust tests; 96 public HTTPS cases passed locally, independently and on Cally.
The [acceptance record](grafana-verification.md) identifies each check's revision and scope.
**Outcome:** Shared CLI/core observations with org/datasource binding, fixed legacy profile,
scoped credential references, bounded/cancellable HTTP and explicit redaction/coverage semantics.
**Scope:** CAP-03 and initial CAP-25 fixture work under [the contract](grafana.md). No live target,
credential setup, endpoint fallback or supervisor change. Live compatibility and Windows remain open.
**Resolved findings:** Fully finalized receipt size, normalized-origin length and strict lossless
RFC3339 admission. Two review/fix rounds, all resolved; incomplete returns: 0. Original failed
snapshots and independent witnesses are retained in the acceptance record.
**Security gate:** Separate manual security-only code-reviewer pass and affected-path reviews;
dedicated auditor spawning was unavailable because of the host's agent-thread limit. No scanner
run is claimed. Tested canonical-encoding limits remain explicit.
**Safe resume:** Latest accepted snapshot `e22a1fe4193058b96afbca4a3e5dedc63dc7fbb9`, tree
`0e23352e0c508589c42b208f65156e3648b7df2d`; 168-file archive SHA-256
`2848dd7f72969a492188f1f39408432489a4fd9f1e334f63f07466ae5ef6431d`.
Verification used a disposable snapshot before the product implementation commit. Both final Cally containers and independent execution trees
were removed. Preserve accepted evidence and rerun affected checks for later changes.

## WORKBENCH-004 — restricted Linux command profile

**Status:** Final R1b static correctness and security reviews APPROVE under
[the bounded contract](read-profile.md). Coordinator-owned Cally execution passed 81 ordinary
Rust tests, all 17 native proofs, 51 schema-valid profile receipts and 96 Grafana cases.
The masking-only nonshipping control failed at the intended helper-execution assertion,
establishing the strengthened hostile-Git regression's sensitivity. Final independent executable
acceptance on the original profile-only snapshot remains separate. The later independent GUI
integration run passed all existing profile suites and exact receipt schemas on unchanged profile
implementation plus the GUI's trusted core seams. Actual namespace-setup failure remains unverified.
The [verification record](read-profile-verification.md) distinguishes these results and identities.
R1 local checks also passed formatting, lint and 22 schemas / 22 positive / 9 negative fixtures.
Correctness review found a P2: explicit ripgrep metadata paths bypassed the promised `.git`
exclusion. The correction adds component admission and fixes nested traversal exclusion; focused
red/green evidence is retained. No supervisor, schema or dependency changed in this correction.
Resumed after the owner's pause and Adama cleanup on 2026-10-02. Cleanup recovered 6.9 GiB;
source, accepted archives, logs and binaries were retained. Compilation uses two jobs, no incremental
cache and no development/test debug symbols. Cally helpers remove disposable extraction/build
archives on exit while retaining source identity and logs, and require 8 GiB local free space.
The helper correction passed scoped independent review at disposable commit
`c181f8e296c034d02b7708bd7d1705ced46f2e65`; one cleanup-error-status finding was reproduced
and fixed. [Staging tests](evidence/cally-staging-cleanup-2026-10-02.json) and
[cleanup-record failure tests](evidence/cally-cleanup-record-failure-2026-10-02.json) retain scope.
**Owner:** Builder owns core/CLI, descriptor integration, schemas and Rust regressions; coordinator
owns this roadmap, documentation, independent public checks and the Cally acceptance lane.
**Outcome:** A launcher-supplied read profile for a small Git/ripgrep grammar, descriptor-pinned
roots/executables, mandatory filesystem/network/process isolation and honest bounded receipts.
**Next action:** Preserve the approved R1b source, coordinator native/control evidence and subsequent
independent integrated profile checks. Resolve the namespace-failure test prerequisite without weakening
the current boundary before claiming full profile acceptance. Existing operator-local commands and accepted
adapters retain regression coverage. Host-policy changes, live credentials and external deployment
remain outside this local implementation scope.
**Boundary evidence:** Local nested namespaces were denied. Cally's unchanged rootless boundary
works with UID/GID 1000 and no inner procfs; it denied writes, an outside symlink and a connection
to the outer private listener. Follow the contract's required product tests before acceptance.
A subsequent filesystem Unix-socket probe bypassed that template's network isolation; required
syscall-filter acceptance now includes this channel, FIFO writes and alternate ABIs.
The [first native product checkpoint](evidence/read-profile-native-checkpoint-2026-10-02.txt),
source archive SHA-256 `d7ac925995d6ce224728bc25b12a0634095d10bb68f30d8a6ab6c89e2648890a`,
passed existing regressions and 9 of 11 explicit profile tests. Two Git tests failed because
the write-open filter also denied Git's `/dev/null` access. This failed checkpoint and the later
correction retain separate identities. Its staging was removed automatically and Adama stayed
below its warning threshold.
The confirmed mechanism is Git's unconditional read/write open during startup. The selected
correction is a fixed, running-image-bound internal launcher with required Landlock write rules,
allowing only the private null device while retaining seccomp and namespace controls. No extra
installed helper, caller-selected launcher, weaker fallback or host-policy change is authorized.
The new setup/exec channel and truthful receipt distinction are part of the frozen review scope.
The [Landlock prerequisite](evidence/read-profile-landlock-prerequisite-2026-10-02.txt) passed
with ABI 8 in Cally's unchanged boundary, including all required ABI 5 rights and positive/negative
null/file/FIFO/device controls. The first integrated-launcher checkpoint, archive SHA-256
`43a6b2dfc99d933fb617ecd2d82e7039bea858c23de524208b3634d466e64354`, failed because
the fixed runtime closure omitted `libm.so.6`; receipts correctly left tool execution unconfirmed.
After adding that explicit binding, checkpoint archive SHA-256
`382a115401ebbd5120ab3fe6b2a6b8bb6989980069cb4321bfeca925d88468d3` passed 77 ordinary
Rust tests, all 13 required native proofs, 51 public profile cases and 96 Grafana regressions.
All 51 exact returned receipts also validated against the schemas in that archive. Evidence is
retained under `/tmp/agenticsre-profile-cally.rQGvA4qx`; its container, derived image and staging
were removed. Final metadata/grammar fixes and strengthened native assertions are later bytes.
The new Cally lane enforces root-owned read-only source and a separate writable build tmpfs.
Its [controlled staging checks](evidence/cally-readonly-image-staging-2026-10-02.json) passed;
the earlier helper review does not cover this material boundary change, which joins final review.
The verifier's custom-probe attempt was stopped by an automatic tool-content filter. It will not
be retried or executed; the independent run is narrowed to existing repository acceptance suites
and exact receipt-schema checks. Any criterion missing from that evidence remains open.
The independent existing-suite run of snapshot `7a907cf3f07ef02202af879b6f3f2fe4529066cc`
(archive SHA-256 `6f68b208439297aebdbf0d909b7b0b4a323d800df0f3fbabd9112827174d31ee`)
passed all executed tests, 14 native proofs, 51 schema-valid profile receipts and 96 Grafana cases.
The overall verdict was INCONCLUSIVE because required namespace failure, missing runtime,
unsupported layouts and read-only FIFO deadline coverage were incomplete. Packet:
`/tmp/agenticsre-read-profile-verify-rjilbkgh/packet.txt`, SHA-256
`5737dd213825fb5618bdcf73848931fa3990f0971ff7a21eabac3d335ddf0aba`.
R1 adds ordinary regressions for the latter three gaps. The standard namespace-disable
prerequisite failed because the Cally container's `/proc/sys` is read-only; that boundary remains
unchanged. Evidence: `/tmp/agenticsre-namespace-prerequisite-_ae_47j8/result.json`. Actual
namespace-setup-failure acceptance remains open; positive execution cannot establish that case.
**Gates:** Frozen correctness/security review and independent executable containment/CLI checks.
Review/fix rounds: 1 (source findings resolved; static re-review and coordinator native execution passed);
incomplete builder returns: 0. A verifier restart hit the agent-thread limit; coordinator execution
is explicitly not substituted for an independent verdict.
Reuse accepted supervisor evidence except for affected
descriptor, wrapper, cleanup or output behavior. Keep the verifier's existing bounded execution
authority, including its scoped 1,024 shared-UID process/thread ceiling.
**Then:** Standalone GUI under WORKBENCH-006, selected by the owner ahead of MCP on 2026-10-02.
Actual host registration,
native Windows, live targets and publication retain their separate prerequisites.

## WORKBENCH-005 — bounded stdio MCP parity

**Status:** Complete for the bounded Linux fixture slice. [Independent acceptance](mcp-verification.md)
passed with earlier unchanged evidence retained and remaining host/platform/profile gaps explicit.
Product reviews approve R2 and the R2b/R2c test/tooling deltas, with product bytes unchanged. The earlier
owner-prioritized GUI and checkpoint `93fc339` remain accepted within their recorded scope.
The [implementation boundary](mcp.md) now specifies both tested protocol revisions, immutable
root/target/configuration authority, one owned worker and bounded pipe transport. A source-backed
design review selected pinned SDK models with an application-owned loop; the SDK service loop's
detached tasks would otherwise require a second lifetime model. Credential-free Grafana discovery
and actual-load configuration/CA identity checks are implemented while preserving existing wrappers:
101 affected core/CLI checks passed, with 18 existing native tests explicitly ignored, plus focused
Clippy. Guard-removal controls failed at the expected config/CA identity assertions and were restored
before regression checks. Official references and the published 3.5.0
checksum were checked; bounded read-only SDK inspection is retained under
`/tmp/agenticsre-rmcp-source-rz_317ls`. It supports current and legacy revisions, but its default
framing/dispatch do not establish the required application bounds. No MCP registration or live
connection has been created. Minimal exact `rmcp=3.5.0` model-only dependency resolution added 15
external packages without upgrading existing ones; ten Linux archives totalled 1,397,027 bytes.
The first adapter walking skeleton passed three real stdio tests for modern/legacy numerical calls,
discovery and startup refusal; later lifecycle additions remain under test. Independent raw-client
checks found unknown methods with parameters incorrectly returning invalid-parameters instead of
method-not-found; the builder corrected routing, pending fresh execution. Disposable Cargo output
was cleared after preserving the walking-skeleton binary and all accepted evidence, recovering
approximately 1.25 GiB. The corrected adapter passed 20 focused Rust tests (one native proof ignored)
and 23 independently authored raw-client groups with schema validation. The real HTTPS MCP fixture
client passed 18 checks / 11 schema-valid receipts, including both protocol eras, CLI parity,
identity pinning, busy/discovery responsiveness, cancellation without a final reply and connection
closure on EOF/stdout loss/signals. These are coordinator/builder checks, not the final independent
verdict. Source formatting and all specification checks passed. Workspace Clippy passed after an
adjacent unsafe-block comment correction in a test. Full workspace regressions passed 127 tests,
with 19 native tests explicitly ignored. The bounded Cally MCP lane is added. Local command fixture
inspection is unavailable because the existing private namespace maps trusted system bindings to
UID/GID 65534; this refusal does not establish native command acceptance. Cally must demonstrate
successful inspection and execution under its existing root-owned fixture image. The local command
client passed eight admission checks / nine schema-valid receipts, explicitly reporting successful
inspection unavailable and native execution skipped. All adapter and public-client source is frozen
for candidate capture; native command checks, reviews and independent executable gates remain pending.
Product review/fix rounds: 2 completed, with all source findings resolved in R2.
Incomplete builder returns: 0.
**Frozen candidate:** R0 snapshot `fa41391a36306a0c531f85251e5b8000277982c8`, tree
`10d77c58fcde66388621555229799c51dd26f998`, 273 files, under
`/tmp/agenticsre-mcp-r0-snapshot-h1i8xg5o`; capture manifest
`/tmp/agenticsre-mcp-r0-snapshot.json` SHA-256
`4ba3624e00b21ed4e98f3812c709c93ab0ffd2e1f07e617a5f02cabd23bd39ac`.
R0 correctness review requested changes: two P1 lifecycle defects and one P2 notification-validation
defect, all independently found. The separate static security audit passed with zero validated
security findings; this does not waive correctness findings. Independent executable verification
was initially queued after an agent-thread-limit rejection and now waits for the corrected candidate;
coordinator results do not replace its verdict.
Cally preflight succeeded and found no remaining `agenticsre-` test containers. Original frozen
source stays unchanged; final evidence will distinguish later documentation from product bytes.
**R0 findings:** Serialized worker results discarded the core's `cleanup_unconfirmed` state, allowing
new admission or successful EOF shutdown after uncertain cleanup. Stdout saturation had a deadline
only after a reply was queued, allowing an active long-running operation to continue while output
was blocked. Cancellation and initialized notifications accepted malformed scalar `_meta` values
and changed state. The builder owns corrections and focused red/green proofs; native acceptance
waits for these changes rather than treating the existing idle-pipe check as sufficient.
**R1 progress:** Worker outcomes now retain cleanup confirmation, uncertain cleanup closes admission,
active output saturation has its own five-second deadline, and notification metadata is validated
before state changes. Four focused assertions failed on the original lifecycle behavior; 26 Rust
checks pass after correction, with one native case ignored. Independently authored real HTTPS
regressions also failed on retained R0 bytes: malformed metadata suppressed an active request's
reply, and active HTTP survived blocked stdout beyond seven seconds. Both now pass on R1; the full
raw client passes 23 groups and the expanded HTTPS client passes 23 checks / 15 schema-valid receipts.
R1 binary SHA-256 is `5ffb248ddd3eb5d74757a8c682339cfac18ffcde3c1e73a8a05e13c2534a0a6a`,
retained under `target/mcp-evidence/save-r1`. Old generated Cargo output was removed after retaining
that binary and all evidence, restoring the Cally staging floor. No new host setting or isolation
exception was used. Native process-lifetime checks and re-review remain pending.
The native public client now defines six actual ripgrep/FIFO lifecycle cases, with a reader-open
witness and PID/start-time/pidfd tracking before each cancellation or transport failure. These are
source-complete but unexecuted until Cally. Offline vendoring preflight identified five remaining
locked non-Linux dependency archives; a bounded fetch added 76,876 bytes without dependency changes,
and the complete locked offline fetch now passes.
**R1 frozen candidate:** `663e30c05a8172ed9a98c321d4fa948b809c5f06`, tree
`b30d02e6e249534ecba9262d18836652acc7e899`, 273 files, under
`/tmp/agenticsre-mcp-r1-snapshot-drqib70r`. Capture manifest
`/tmp/agenticsre-mcp-r1-snapshot.json` SHA-256
`a485b285d9dec0fbc2a5cd6d499fc13b06a40286b983b8c01c2fcecbf47bb88b`.
Correctness re-review and independent native verification are assigned. The verifier will stage
with its own recorded data-only driver and reuse the cached pinned image, executing the same MCP
container lane without running repository shell scripts on Adama or pulling an image. Runtime
and cleanup bounds remain unchanged. Security re-audit of the two changed runtime files is queued
after another agent-thread-limit rejection; R0 security coverage does not automatically cover R1.
**Earlier R1 pause boundary (superseded below):** R1 correctness verdict was REQUEST CHANGES. The three original mechanisms were
addressed, but malformed frames such as `[]` still enqueue errors before the uncertainty-admission
guard. A client draining output while supplying such frames can keep extending shutdown after
cleanup is unconfirmed. Close admission before any error-producing branch, or stop reading during
the bounded uncertainty drain, and prove malformed traffic cannot extend that drain. This P1 is
source-backed; its new regression and correction are not yet implemented.
Independent Cally verification stopped at a real prerequisite check: the cached pinned Rust image
does not include `cargo-clippy` for Rust 1.98.0. The added MCP lane therefore cannot run unchanged.
No MCP build or native suite ran on Cally. Resolve the lint environment without changing host
packages or weakening runtime checks, then execute the frozen corrected candidate. Evidence is in
`/tmp/agenticsre-mcp-r1-verifier-2pz2lsuu`. R1 security re-audit and independent executable acceptance
remained open. Work paused at that checkpoint at the owner's request, before the later resume below.
The verifier confirmed exact-name removal of its prerequisite container; no verifier job, disposable
container or derived image remains. The cached base and retained evidence were preserved.
**Current resume work:** The builder owns a failing regression and correction for the remaining
malformed-input shutdown bypass. The Cally prerequisite is resolved by restoring the already
documented check split: formatting/Clippy run in the private local sandbox with the installed pinned
toolchain, while Cally runs native/runtime acceptance. The accidental Cally-only Clippy invocation
is removed; fresh local lint remains mandatory for the corrected source. No component installation,
container boundary change or skipped lint result is substituted for that gate.
R2 reproduced five failing regression cases before correction, including a real pipe drain that
emitted extra responses beyond the original uncertainty receipt. The corrected terminal route
handles closed admission before ordinary error responses and latches an absolute drain deadline.
All 25 MCP crate tests now pass, including malformed JSON, non-object input, invalid IDs/envelopes,
matching cancellation, exact original output and paced input during partial delivery. Evidence:
`/tmp/agenticsre-mcp-r2-terminal-red.log` and `/tmp/agenticsre-mcp-r2-terminal-green.log`.
R2 affected CLI/crate checks passed 33 tests with one native proof ignored. Workspace Clippy passed
inside the private local sandbox for all targets/features with warnings denied, and formatting
passed. Coordinator raw-client checks passed 23 groups; the full HTTPS client passed 23 checks /
15 schema-valid receipts. Logs are `/tmp/agenticsre-mcp-r2-focused.log`,
`/tmp/agenticsre-mcp-r2-clippy-green.log`, `/tmp/agenticsre-mcp-r2-public.log` and
`/tmp/agenticsre-mcp-r2-grafana.log`. The binary is retained as `target/mcp-evidence/save-r2`, SHA-256
`b74516725a023fb1451b28afc5d321cf6f16b8d846481a03018a69f91af744b0`. Disposable Cargo output was
cleared after these checks to restore the Cally staging floor. Final re-review and independent
native execution remain pending; no acceptance claim follows from the local counts alone.
R2 correctness re-review and scoped security re-audit both APPROVED the exact frozen source, with
zero unresolved findings. Independent Cally execution then passed 140 workspace tests (19 native
tests ignored there), 19 native CLI profile tests, 10 core profile tests, 51 public profile receipts
and 96 Grafana cases. It stopped at the new native MCP test: the fixture omitted required `-e`
before its ripgrep pattern and correctly received `read_command_denied`. This is a test-contract
error; the profile grammar is unchanged. The fixture now uses the documented explicit-pattern form.
The helper also runs this feature-enabled Rust test before restoring the ordinary CLI build, so
later public MCP clients and the recorded binary digest refer to the normal product build.
These are test/tooling corrections; R2 product bytes remain unchanged. Retain the passed native
baseline and rerun the corrected MCP target plus the three pending public MCP clients. Evidence:
`/tmp/agenticsre-mcp-r2-cally-ddehfp91/cally.log`; original run and exact cleanup both completed.
Test correction rounds: 1; product review/fix rounds remain 2, with source findings resolved.
**Latest pause checkpoint — R2b:** Frozen snapshot `/tmp/agenticsre-mcp-r2b-snapshot-xfy0xf51`,
commit `55ba7961421ed8c8e481e8730807595deaf22771`, tree
`8b672fb6ee63f23991529812b9e2913ef27005e0`. The 273-file manifest
`/tmp/agenticsre-mcp-r2b-snapshot.json` has SHA-256
`696f8cb999951b2bb79ed4498b0b636c4b963b79cd3d3b49d02a8c62c89ec7b2`.
Independent comparison confirmed that all product bytes match approved R2; only the native test,
Cally helper and roadmap changed. The bounded delta review APPROVED with no findings. Fresh
workspace/all-target/all-feature Clippy passed in `/tmp/agenticsre-mcp-r2b-clippy.log`.
Independent Cally execution passed all nine native MCP tests and all 23 raw protocol groups.
The raw command client then stopped at `tools/verify-mcp-commands.py:494`: its semantic comparator
removes invocation times and PIDs but retains `sources[0].observed_at`. The captured Git MCP and CLI
receipts differ only in that observation timestamp after the existing exclusions. This is an
acceptance-client defect, with no product divergence established. No correction or retry was made
before the owner's requested break. Five command-client groups passed; the remaining command
checks, all six native lifecycle cases and the independent raw HTTPS MCP client remain pending.
The final in-container residue check reported no surviving product processes. Container, derived
image and disposable staging cleanup are confirmed; evidence is retained under
`/tmp/agenticsre-mcp-r2-cally-aahi7evy`. Retain earlier passing R2/R2b evidence at resume.
All nine exported command receipts passed the frozen canonical schemas. The independent packet
`/tmp/agenticsre-mcp-r2-cally-aahi7evy/VERIFICATION.md` has SHA-256
`63075e66b9cee630d5e42d92cd9d7fc005b23240e6815bdc6312ad742f0aafd2`; its overall required-lane
verdict remains FAIL due to the comparator defect. `parity-comparison.json` preserves the exact
receipt-pair comparison. No run-owned job or process remains.
**Next authorized work on resume:** Correct only the comparator's volatile source timestamp
handling, check it against the captured receipt pair, then freeze and independently run the pending
command/HTTPS clients. Keep complete receipt schemas and all semantic fields checked. Do not
restart already passed suites without a material reason. Finish MCP acceptance before packaging.
**R2c completion in the three-hour session:** The comparator regression failed before correction
and all three tests passed afterward, including semantic-difference and original-receipt guards.
The exact retained receipt pair compares equal. Independent delta review APPROVE; every product
byte remains unchanged from approved R2. Cally passed 15 native command checks (all six lifecycle
cases), 23 HTTPS checks, all 31 exported receipts and 20 advertised tool-schema observations.
Final residue was empty and exact cleanup passed. Packet/source identities and retained R2/R2b
coverage are in [MCP verification](mcp-verification.md). Product fix rounds remain 2; test/tooling
correction rounds total 2. Actual host installation, Windows/live compatibility and the original
namespace-refusal acceptance gap remain open. Continue with Linux packaging below.
**Outcome:** Launcher-granted discovery and equivalent core receipts for restricted commands,
the numerical task and explicit Grafana fixture targets, with bounded framing and cancellation.
**Gates:** Declare tested protocol versions; freeze and independently review/verify the adapter
and affected core. Actual host registration/version acceptance remains separate.

## WORKBENCH-006 — standalone graphical interface

**Status:** Complete for the agreed local browser PoC. Final R2 correctness review and dedicated security
audit APPROVE; independent focused native execution passed six UI library tests, seven HTTP/native
tests and all five actual browser flows. The [acceptance record](ui-verification.md) binds source,
local/native binaries, retained regression evidence, corrections, test boundaries and remaining limits.
The owner chose a local browser app for checks and results/history ahead of MCP, and accepted the
presented light/dark mockups with “continue.” The actual embedded React/TypeScript app now completes
the numerical task, exact receipt download, history/reopen/clear, themes, keyboard and mobile flows
in cached Playwright/Chromium inside the private local sandbox. Lost admission responses reconcile
with the same submission ID; uncertain admission blocks another submission. No-grant and invalid-token
states are exercised. Screenshots and results are in `target/ui/`.
Initial backend checks passed 95 workspace tests (18 native tests explicitly ignored), then six focused
HTTP tests after the isolated signal-exit correction; live OpenAPI response validation passed 15
checks. Formatting and Clippy passed. Initial frontend checks passed 28 tests; final R2 checks
passed 34 tests, lint, strict TypeScript, build and generated-type parity.
GUI work reuses the core and introduces no alternate privileged execution path.
The first frozen Cally GUI run (`8c44553a2f2f3a7dce8ab5df8d162740615c53e1`) passed the ordinary,
native, GUI API, 51 profile and 96 Grafana checks. The actual browser completed native Git/ripgrep
and obtained a cancelled receipt, then stopped on an ambiguous test locator matching both the
status badge and error heading. Product behavior passed that assertion; final download/timeout
browser assertions were not reached. The test now scopes status to the result header; the shared
completion helper is corrected for the same timeout ambiguity. Final R2 rerun passed both assertions.
Dedicated independent static security audit APPROVE on that snapshot; zero validated findings.
Correctness review requested three P2 corrections: preserve original receipt text for downloads
and the complete JSON view (JavaScript can round large inode integers); append diagnostics rather
than reopening a redirected log at offset zero; and assert honest incomplete tool confirmation
after FIFO timeout. The log overwrite was reproduced and its append correction passed six UI
library tests plus Clippy. Receipt preservation and the final focused integrated rerun passed.
The independent first-run packet is `/tmp/agenticsre-gui-verify-5ew3zhrn/packet.txt`, SHA-256
`28ff1ae772e60d67af2034e30707ed90f5e8ab86ca5d0a3ab9523d68ce902ae0`; it records 96 ordinary
and 18 native outcomes, exact schema validation, the browser test failure and confirmed cleanup.
R1 corrected all three original findings, but its raw JSON view bypassed the existing Unicode
direction-control escaping. R2 restores display escaping while retaining untouched download text;
the new display/download regression failed before the correction and all 34 frontend tests pass
after it. The actual native browser fixture also passed direction-control display and original
download contents. R1's focused native run completed six UI library tests, seven native HTTP tests
and all five browser flows before its requested withdrawal took effect; these are retained
observations, not acceptance of the known R1 display defect. Final R2 source is commit
`76e6a3f316da685e12f3e741935f6dd55d9e64c0`, tree `9bde9db7f06e19e8b6e527e2f7b31934f5dd6776`,
archive SHA-256 `c0f571485b61025e88ca2b067b2acfd1bea15f4736eb0526cab65085e480f404`.
The final independent verdict is PASS for scoped GUI acceptance; packet
`/tmp/agenticsre-gui-r2-verify-c_pfywgk/packet.txt` has SHA-256
`114893b8219e6437a0d863da9e6d85a2bf91cd0172c985c7066114ba27f059cb`.
Exact-name cleanup readbacks confirmed that all disposable run containers/images and staging were removed.
Test correction rounds: 1; product review/fix rounds: 2 (R2 display regression corrected).
**Outcome:** A usable local interface for the agreed first workflow, with readable results,
designed failure states, keyboard access and light/dark themes.
**Next action:** Use the [local launch instructions](development.md#local-browser-interface). Keep the
known runner namespace-failure gap and broader platform/live-adoption work explicit. The authoritative
[interface contract](ui.md) and [OpenAPI schemas](ui/openapi.json) define scope and bounds.
**Gates:** Passed for this GUI slice: approved visual specification, real end-to-end workflows,
browser-render/keyboard verification, and independent correctness, security and executable gates.
Existing live/publishing/host-change authority is unchanged.

## WORKBENCH-007 — Linux development artifact and rollback

**Status:** Complete for the Linux/stateless development slice: independent R1 review APPROVE and
installed acceptance PASS. This does not close native Windows, release clearance or the AC-36 pilot.
**Outcome:** A reproducible development bundle with exact binary/source identities, documented
runtime prerequisites, and a versioned disposable installation that can be upgraded and rolled back.
**Scope:** Initial CAP-25 packaging/diagnostics and the Linux portions of AC-34/AC-35. AC-36 remains
the separate five-task human/agent pilot and cannot be closed by packaging. Build from the accepted
source; exercise install, missing dependencies, upgrade and rollback inside the existing isolated
test environments. Use explicit local paths, no host package manager, service registration, global
PATH edits or production configuration. Preserve the embedded browser and stdio interfaces.
**Gates:** Verify the packaged mission transactions and executable trust/modes after installation;
bind evidence to the actual artifact and tested Linux environment. Publication, live installation,
actual MCP-host registration and Windows runtime evidence remain separate acceptance requirements.
**Implementation contract:** [Linux bundle](packaging.md). A portable archive with a source-qualified
artifact ID, manifest/checksums, license/notices, compatibility/dependency notes and explicit
versioned paths can use the existing offline doctor and embedded task/UI/MCP interfaces without
core changes. Rehearsal must use two distinguishable artifacts, reject corruption before activation,
preserve synthetic external config/evidence, and rerun the packaged mission after rollback.
**First builder checkpoint:** Deterministic bounded USTAR creation, digest-bound verification,
versioned staged installation and separate atomic activation/rollback are implemented in
`tools/package-linux.py`. Public CLI integrity/selection tests passed 23 cases on Python 3.12.3
and 3.14.7; hash-gate mutation controls failed as expected. Root's two selected rollback/tamper
checks passed. Exact product execution and missing-Python behavior were pending Cally at this checkpoint.
Root's checksum-bound notice preparation includes project/upstream notices, all locked Cargo
registry packages, 17 pinned runtime npm packages and selected Tailwind CSS/Vite contributors
(19 npm notice records), with distribution gaps explicit.
**R0 review:** REQUEST CHANGES. A P1 permits same-size input replacement between hashing and
validation/installation; a P2 permits declared dependency notice paths absent from the bundle.
The builder owns deterministic regressions and corrections; no native packaging job launched
before correction. Review/fix rounds: 1 underway; incomplete builder returns: 0. R0 capture:
`/tmp/agenticsre-package-r0-snapshot-o9ehx0jr`, commit
`5bd0693cc2968c864972ca0fda7a193481ca5708`, tree
`88a33326274ba69eddad949c94ea5cc1cbdbdbfb`; 298-file manifest SHA-256
`9371a1198444d00250fa707e07f3c00ef5e8e6a7f626dc09567546dff2ab3f40`.
All Rust/embedded frontend bytes match accepted MCP R2c.
**Artifact export prerequisite:** Independent real-engine preflight passed using only fixed
synthetic data files and the bounded export barrier/controller. Exact bytes/digests matched;
container cleanup/readback passed. Evidence: `/tmp/agenticsre-package-export-preflight-rekfq7uk`.
Actual builds and installed-product transactions awaited the corrected frozen candidate at this checkpoint.
**R1 correction:** The archive operations now hash/parse/install an unnamed private bounded copy
of the external input, so a writable original cannot replace validated bytes between passes.
Actual same-size file replacement reproduced both verify/install failures before the correction
and retained authorized bytes afterward. Declared dependency notice references now require safe
paths to included files across creation, verification, installation and selection. Four public
missing-notice cases failed before correction and now refuse. All 31 tests passed on Python
3.12.3 and 3.14.7; root's two race cases and missing-notice spot check passed. Evidence:
`/tmp/agenticsre-package-r1-p1-{red,green}.log`,
`/tmp/agenticsre-package-r1-p2-{red,green}.log`, `/tmp/agenticsre-package-r1-green.log` and
`/tmp/agenticsre-package-r1-root-spot.log`. Independent R1 re-review APPROVE; both source findings
are resolved, with zero new or unresolved findings. Product/runtime driver bytes remain unchanged.
The corrected 301-file source capture is commit `9c1ee1c7e058f48afbb99bcbb867a519392ba066`, tree
`81d6b895a346475bc9017a6c39778fc2b4208df6`; capture manifest SHA-256
`d264a8f5c105ba8276fd27714102850b82caac8ebc2e8639094850c624ee1009`.
Packaging review/fix rounds: 1 completed; incomplete builder returns: 0.
**Real Cally artifacts:** Current and previous ordinary binaries built sequentially from exact accepted
captures; deterministic repack and all eight installed positive checks passed. Installed CLI/tasks,
current stdio MCP/UI, explicit upgrade/rollback, hashes/modes and unchanged external canaries passed.
External DATA-only validation checked both exported archives and 17 exact schema-valid receipts.
Archives and build metadata are retained in `target/packages/linux-r1`, with full identities in
[packaging verification](packaging-verification.md). The first lane's overall exit was 1 because
nested proc setup failed before the missing-Python payload; a follow-up likewise stopped on an
extra verifier assertion. Those failed setups remain preserved. A separate COPY-only disposable
image with non-executable fixed Python bindings completed the scoped negative payload exit 0;
its five receipts are independently schema-valid. All four focused groups and cleanup passed,
giving 22 validated receipt observations in total. The combined sealed packet is
`/tmp/agenticsre-package-unavailable-python-mhf8ewgw/VERIFICATION.md`, SHA-256
`d5fd9c9e29e4f91ef96db21602d4a2948c995b9dd637942f661b8a4255dc9044`.
Its 19-artifact digest manifest is
`ff650194e0056f62ede74fa1ed91f3e3fd25bc5c6fe18ec781788ed45492269d`.
No run-owned jobs, containers or disposable images remain. This does not repeat the accepted
positive builds or weaken the existing outer execution boundary. The fixture proves unavailable
fixed runtime bindings; a renamed authentic interpreter runs the verifier.

## WORKBENCH-008 — sourced service context fixtures

**Status:** Complete for the offline core/CLI fixture slice: R1 static review APPROVE and independent
Cally acceptance PASS. The [fixture contract](context.md) fixes source shape, trusted age policy,
authority and bounds; [native verification](context-verification.md) preserves scope and evidence.
**Outcome:** Resolve explicit service/environment metadata from a bounded operator-selected record
export, with source/review identity and honest stale/retired/ambiguous/partial states. No live
catalog or automatic target dispatch. Actual team source/adapter remains the separate DEC-09 decision.
**Scope:** First offline CAP-07/AC-12 slice through the existing core and CLI. Source paths remain
trusted startup configuration; UI/MCP context exposure, catalog writes, credentials and live
integrations stay outside this increment. Preserve the existing operation and grant behavior.
**Time boundary (completed session):** Builder returns by 00:15 UTC for independent review and bounded Cally acceptance.
Keep the session's 00:42 full-job cutoff and 01:02 executable cutoff; unfinished acceptance must
remain explicit at the review commit rather than overrunning the three-hour deadline.
**Builder checkpoint:** Core/CLI resolution, discovery, three typed schemas and fixtures are
implemented. Seven public CLI tests passed, including structured-call parity, exact observed
source identity, ambiguity/missing selectors, stale/retired/future records, incomplete bindings,
optional omissions, malformed/null input, FIFO/devices and all-component symlinks. Source access
pins with `O_PATH` and inspects the regular inode before data-open; it rechecks the descriptor's
device/inode identity. Builder workspace tests passed 79 cases with six existing ignored tests;
all-target/all-feature Clippy, formatting and specification checks passed (25 schemas, 24 positive
and 11 negative fixtures). Root's selected descriptor/FIFO/symlink check passed. Evidence:
`/tmp/agenticsre-context-regressions.log`, `/tmp/agenticsre-context-clippy.log` and
`/tmp/agenticsre-context-specs.log`. At this checkpoint, review/native acceptance, cancellation/
deadline and selected output/record-count boundaries were still unverified; the first frozen review
was next. The accepted Linux bundle above predates this new feature and retains its separate
source/binary identity.
**R0 review:** REQUEST CHANGES; three independent P2 findings, zero P0/P1. Positional arrays can
enter Serde-derived object positions contrary to the export schema; the pinned time parser accepts
lexical forms outside the declared timestamp grammar; fixed-date freshness tests expire after
30 days. Correction round 1 is underway with public red/green regressions, strict shape/timestamp
guards and generated review timestamps for positive tests. No R0 Cally payload was run. R0 source:
`/tmp/agenticsre-context-r0-snapshot-hovm9lf_`, commit
`afd2929d80e5fa176eb39f3be942a7197726a8ee`, tree
`76f41cf3aeb76f254bb6bbe01a7d4122f6840a83`; 315-file manifest SHA-256
`ea9777675fc18e0983215305649e5337a11d10abc9aac275d9d0c63d2bf79236`.
**R1 correction:** Public controls reproduced accepted positional source arrays and invalid `X`
timestamps before correction (exit 0 rather than the required refusal). Explicit object guards now
cover each declared object position; lexical timestamp checks enforce the published UTC grammar
and maximum length before parsing. Positive tests generate current review timestamps and normalize
only observation time and its validated age for parity. All ten focused CLI tests, all-target/
all-feature Clippy, formatting and diff checks passed. Only `context.rs` and its CLI tests changed.
Evidence: `/tmp/agenticsre-context-r1-{red,green,clippy}.log`. Correction round 1 implemented;
fresh frozen re-review and native acceptance follow below.
**R1 review:** APPROVE for the exact two-file correction; all three original P2s are resolved,
zero new/unresolved findings and zero independent P0/P1s. Root's three selected new rejection/
valid-time regressions passed. Frozen R1: `/tmp/agenticsre-context-r1-snapshot-yvmh2dtw`, commit
`e0d223c5ae6c2192417515d749f9ce912fe1dd1a`, tree
`b16a46821256077181c5566cbd814c065a697d6c`; 315-file manifest SHA-256
`fab13d9470f8d6c27a580d90fd7e41bc538fd5da68183e32036e2e6d7cde51e5`.
Context review/fix rounds: 1 completed; incomplete builder returns: 0.
**Native acceptance:** The bounded Cally job started before 00:39:55 UTC against approved R1;
its full workspace/all-feature and public context results are recorded below. No R0 native run occurred.
Run-owned container `agenticsre-context-8stdgxse`, derived image
`sha256:0d43b080deaeabe7d986f6e1b501934ce44e266d7dc68112cba092fb0d248861`;
evidence `/tmp/agenticsre-context-cally-8stdgxse`. The strict pre-staging 8 GiB floor passed after
root cleaned only the completed builder's ignored Cargo cache, preserving all accepted binaries,
archives and evidence. No new full Cally job was started after that session's cutoff.
**Final native observations:** The first R1 job passed 151 workspace tests (19 explicitly ignored),
the selected native MCP grant test and 12 public cases. It then hit a verifier expectation error:
JSON CLI usage errors emit canonical `invalid_usage` receipts rather than empty stdout. Product
behavior was correct and unchanged. One focused late check, authorized for a 120-second staging/
420-second runtime bound with launch before 00:48 UTC, rebuilt only the ordinary CLI and ran the
remaining 80 cases. It finished by 00:45:39 UTC. Both builds have identical SHA-256
`22b8a16efbfce9fc03f1e6ed7ac0d64437757a38137124ba355f8bef36321153`.
All 92 distinct public checks passed. All 90 public and two direct cancellation/deadline checkpoint
receipts are schema-valid. Source/external canaries stayed unchanged, metadata loopback connections
were zero and no product residue remained. Exact cleanup and all 315 source hashes passed; no
run-owned containers/drivers/staging remain. The original incomplete run is preserved.
Combined sealed packet: `/tmp/agenticsre-context-cally-ycwbvgb2/VERIFICATION.md`, SHA-256
`d5cf0ebe34db98e721c6bec4080cabeb17abb0c729c9665f999686d333d2a28e`; 35-artifact manifest
`36a2b5a92a8836e7c5322ec7f9b7bada8ebc8b7d123ddcb7676013d2c6919a98`.
Unchanged packet/receipt copies are tracked under `docs/evidence` for review. The one injected
Cargo test covers two deterministic checkpoints; it does not establish public in-flight signal timing.
Context executable-client correction rounds: 1; product review/fix rounds: 1. No new executable
checks are needed for the accepted fixture slice. Evidence and the session's review commit were
completed as `837c217`; the later installed artifact mission is separate WORKBENCH-009 evidence.

## WORKBENCH-009 — context-containing Linux development bundle

**Status:** Complete: independent WORKBENCH-009 verdict PASS; packet and exact receipts sealed.
Final integration checks passed: 25 schemas, 24 positive/11 negative fixtures, 15 package documents,
25 repository documents and all planning IDs; `git diff --check` is clean. The ordinary CLI was built from exact commit
`837c217` with the accepted package tools and existing bounded Cally environment. The actual
context-containing archive was exported before disposable build cleanup.
The 8 GiB staging floor passed with 9,046,966,272 bytes free after removing only the completed,
unused frontend npm download-content cache. Source, installed dependencies, cache logs/index,
available notices and accepted binaries/archives were retained; their protected digests are
unchanged. Cleanup evidence: `/tmp/agenticsre-context-bundle-cache-cleanup.json`.
**Outcome:** A separately identified development bundle that resolves explicitly selected offline
context after installation away from source, with schema-valid provenance and refusal behavior.
**Acceptance:** Verify deterministic repacking, installed hashes/modes, existing CLI/tasks/MCP/UI
missions, installed context CLI/structured parity and stale/ambiguous/missing refusal, explicit
upgrade from the retained MCP bundle and rollback, unchanged external canaries and exact cleanup.
Retain prior context matrix, native grant and packaging integrity evidence when the corresponding
source bytes are unchanged; do not rerun full suites solely for another archive identity.
**Owners:** Coordinator owns documentation and this roadmap. Independent verifier owns private
build/installed acceptance drivers, immutable evidence and artifact export; product/tool changes
are not planned. Require at least 8 GiB free before local staging; preserve accepted data artifacts.
**Limits:** Linux development packaging only. No release clearance/signatures, global installation,
live integration, UI/MCP context exposure or native Windows claim. Review/fix rounds: 0 for this
increment; any material product/tool change re-enters its affected independent gates.

**Actual artifact:** `target/packages/linux-context-r1/current.tar`, SHA-256
`1ed48ee349b9c51e0be98bb0ada2ce1fd2577773f91d273f709ad669298917dd`, 40,572,928 bytes;
artifact `save-0.1.0-src-5e4001ba226190e0-bin-22b8a16efbfce9fc`. Exact source-manifest SHA-256
`5e4001ba226190e0d27a5dae7950985c97eadf42cdbfe8c20f35a4b08b8baf4f`; ordinary binary SHA-256
`22b8a16efbfce9fc03f1e6ed7ac0d64437757a38137124ba355f8bef36321153`, matching the two
accepted context builds. Root's retained DATA copy passes unchanged package verification; the
older MCP and GUI artifacts remain intact with their original hashes.
**Installed acceptance:** Deterministic repacking and all nine installed groups passed: explicit selection,
CLI/tasks, exact hello receipt, current stdio MCP/browser assets/authenticated API, context direct/
structured parity and provenance, stale/ambiguous/missing refusal, distinguishable upgrade/rollback
and unchanged external canaries. All 26 exact receipts are schema-valid; five context observation
records and the raw receipts are copied unchanged into `docs/evidence`. All 318 source hashes
match committed Git blobs; 115 product/build/frontend/third-party files and five package tools match
accepted baselines. Full existing suites were reused rather than repeated for another archive ID.
Final independent readback confirmed empty residue, exact container/image/staging/driver absence,
unchanged cached base and retained prior archive. [Acceptance details](packaging-verification.md#context-containing-refresh)
and [installed commands](development.md#development-artifact-and-rollback) retain the tested scope.
**Sealed packet:** `/tmp/agenticsre-context-bundle-cally-isacf5wl/VERIFICATION.md`, SHA-256
`b8c56039ad431889a6488e8c09207cfb934f336e36138b1c11161f7afdf4fc1d`; 46-artifact manifest
`6bf5d842f217b8314d4bf7d37661611646fc87418a84e2777e7dd96dcfe20638`.
The unchanged packet is copied into `docs/evidence`. The bundle refresh introduced no product/tool
change or live installation. The subsequent owner-requested review handoff commits its documentation
and evidence on `codex/context-linux-bundles`, alongside the existing implementation checkpoint.

## WORKBENCH-010 — configured service diagnostic pack

**Status:** Qualified PASS for the required core/CLI diagnostic mission and affected Grafana
regression in the tested private Linux boundary. Independent affected correctness/security reviews
accept frozen R1, and native execution closes the required DNS/TCP/TLS/HTTP/process, body,
deadline/cancellation, consumer-loss and regression criteria. Positive system-resolver first-server/
no-fallback traffic remains INCONCLUSIVE because UID 1000 could not bind private port 53; this
specific fixture gap is excluded from the pass. The [verification record](diagnostics-verification.md)
retains exact source, original failed attempts, final accepted selection and cleanup. The active
checkout now also contains the separate WORKBENCH-011 candidate; the core/CLI acceptance does not
accept those later adapter/frontend edits.
Configuration/receive semantics are in
[the finished profile](diagnostics-config.md), with four strict machine schemas.
Deliver the full first pack's DNS/TCP/TLS/HTTP and configured local-process observations through
the core/CLI, then integrate its results into the browser in a dependent increment. The fixed pack
has an explicit target/address policy, real TLS verification, bounded asynchronous DNS/network IO,
one parent deadline, at most two concurrent checks and honest failure/blocked/partial results.
**Mission:** A named installed pack runs from outside source against controlled private DNS/HTTPS
fixtures and their actual kernel process identity. Its direct and structured calls agree and return
schema-valid observations/provenance. Unauthorized addresses/redirects/proxies/substitution are
refused; independent branches, cancellation and cleanup are verified.
**Ownership:** Builder owns the new core diagnostic module, its immediate core/CLI seams, schemas,
examples, Rust tests and public fixture checker. Coordinator owns this roadmap and the initial
contract; builder proposes contract changes before incompatible behavior. Reviewer and independent
verifier own read-only review and private acceptance/evidence, respectively. Existing process,
read-profile, context, Grafana, UI/MCP implementations remain accepted baselines; modify them only
when an evidenced required seam demands it. There is one Cargo builder and no local listener.
**Boundary:** Existing read-only local test sandbox and COPY-only offline Cally lane, synthetic inputs,
private PID/network, no host credentials/mounts or package/service changes; require 8 GiB free
before staging and preserve prior artifacts/evidence. Reuse passed checks for unchanged paths.
**Gates/cadence:** Independent correctness and affected network/security review, then independent
executable acceptance on a frozen source capture. Product fix/review rounds: 1 resolved; incomplete builder
returns: 0. No source commit/push until separately authorized. No new global time limit was requested.
**Next boundary:** Correct the private capture assertion, finish the remaining native criteria
using the same frozen R1 binary and retain every completed proof. UDP/TCP and bounded alias
handling remain supported.
Keep Linux/platform and live-scope
gaps honest; do not replace required checks with fake success or claim overall product completion.
**Walking skeleton before review:** Public CLI and structured calls both succeeded with complete coverage and
actual DNS/TCP/TLS/HTTP/kernel-process observations. Their receipts passed the envelope and
diagnostics schemas; fixture witnesses observed A/AAAA, separate TCP/TLS probes and one HTTP GET
per run to the pinned peer. Source/CA/leaf identities matched. Log
`/tmp/agenticsre-diagnostics-build.cqSqi8TS/walking-skeleton-r1.log`, SHA-256
`198a02f86c233d7889f5c72c9ba1c029b06e4d2f5ab5c0e95db2fe70390fb8ee`.
The initial mission failed safely on Hickory's special-use localhost synthesis before any service
connection. The builder traced the upstream behavior, switched the real DNS fixture to a matching
`diagnostics.workbench.test` certificate/name, and rejects synthesized reserved service names.
That failed setup evidence is retained; it is not relabeled as a successful configured DNS query.
That pre-review implementation added only two direct cached dependency edges, without version
changes. Its verifier preparation confirmed cached archives/base identity and prepared a separate
framing/signal/dispatch/process matrix. Review/fix rounds were 0 at that boundary; ordinary
build/fixture corrections are retained in the builder evidence. The later R0 review and R1
correction statuses below supersede this historical checkpoint. No Cally product job had started
at that pre-review boundary.

**Final local R0 implementation:** Builder packet
`/tmp/agenticsre-diagnostics-build.cqSqi8TS/PACKET.md`, SHA-256
`0c45d4ddad07d6b6bda7158ffe4df414e74f032447a48d4f2a3753a1b4931285`.
All final own exits were 0: 158 Rust tests passed, 19 existing native proofs ignored; 63 exact
public receipts passed both schemas from a byte-matched private copied installation; human output,
canaries and joined fixture cleanup passed. Full Clippy/format/specification/diff checks passed
(29 schemas, 27 positive/12 negative examples, 28 repository documents). The packet retains exact
commands, source hashes, logs and failed attempts. The optional-process stop tripwire found and
corrected a scheduler predicate before review; its red binary/log remain preserved.
Root's focused final sandbox mission independently observed all four actual network checks in
stop mode with the process honestly unconfigured, exact config/CA identities, three connections
and one GET. Log `/tmp/agenticsre-diagnostics-root-optional-stop.log`, SHA-256
`72bdc05cc1306e3dd1cf0033b6ae8f27fb12fe4e5423246647a4bef40212cf35`.
The earlier root five-check spot predates that correction and is not relabeled as final-source proof.

**Completed cache cleanup:** All builder sessions/processes ended. Root checked the 26 owned source
hashes, preserved final/red ordinary binaries as mode-0444 DATA, and removed only this worktree's
completed ignored `target/debug`. Free space rose from 7,880,065,024 to 8,782,110,720 bytes,
passing the 8 GiB capture/staging floor. Three prior archives retain their exact accepted hashes,
and source Git status is unchanged. Evidence `/tmp/agenticsre-diagnostics-cache-cleanup.json`.
Final binary `target/diagnostics/save-candidate`: SHA-256
`20308895711abe40d277d601be439f07a3bb3f85320e73d2a87599ce0c5eacfe`, 38,941,952 bytes.

**R0 proof limits at local return:** Native independent acceptance and affected R1 reviews had not run.
Controlled system-resolver file variants and unavailable/inaccessible proc views need their own
supported native fixtures; local missing-resolver proof remains separately attributed. Synchronous
pinned regular-file/proc reads cannot hard-preempt an uninterruptible kernel/filesystem stall.
The prepared independent matrix includes actual expired-certificate and raw kernel witnesses,
five public in-flight signal cases and consumer loss; it has executed no native product bytes.
Those local results attest the R0 bytes and do not establish acceptance of R1 edits.
No new commit, push or live change occurred.

**Immutable R0 review target:** `/tmp/agenticsre-diagnostics-r0-snapshot-y9c8z6o5`, candidate
`46bb04f5e07d411d23bd1138574d9454cedf3498`, tree
`7eabfb31f74dacb2fec7537fbc2a86310300c8d4`, parent `2c4e422`.
Manifest `/tmp/agenticsre-diagnostics-r0-snapshot.json`, SHA-256
`b53c66ef641dc18ec62f3a9e7fc3b1a5b96cedd74494b34d7f88b2ed6e73900f`:
338 regular files, 3,959,808 bytes; 334 Git mode 100644 and four mode 100755. The original is sealed
read-only; source refs/status/bytes/modes stayed stable. Root matched all 338 hashes, including all
26 builder-owned paths and its own contracts/roadmap. Execution must use a separate copy.

**Independent R0 findings:** Correctness REQUEST CHANGES, independent P0/P1 count 1. One P1:
Hickory synthesizes loopback reverse-zone/CNAME answers without the configured resolver while
the receipt claims an actual lookup. Three P2: retry settings and underlying transport/reconnect
logic contradict the single-attempt profile; unrelated answer owners/types can select the service
peer; chunk trailing CRLF bytes escape the aggregate framing counter. These were independently
traced through complete diagnostic/immediate-caller paths and locally cached locked libraries.
The separate security auditor found no P0/P1 and confirmed the retry/profile discrepancy as a
low-severity P3; it established no destination-policy bypass. That security verdict does not
override the correctness gate or authorize acceptance of subsequently changed safety-sensitive code.

**R1 correction scope:** The owning builder retained R0 source/binaries/logs, produced public
red/green packet/association/synthesis/framing witnesses, preserved bounded CNAME behavior
and both transports, and returned new source/binary identities with the affected required gates.
Only necessary already locked cached dependency edges are permitted; no package/version download,
source commit/ref move, host listener or live change. Re-enter both affected independent gates.
Fix/review rounds: 1 open, maximum 2; incomplete builder returns: 0. Cally staging/runtime stays
pending; the fresh 8 GiB floor applies before capture and staging.

**Completed local R1 correction:** Packet `/tmp/agenticsre-diagnostics-r1-build.31pxy06M/PACKET.md`,
SHA-256 `9465b11dc86387f2e062b7e8fda6911d1ae402a4ca6d112c8046bfad51791922`.
All required own exits were 0: 167 Rust tests passed, 19 existing native tests ignored;
63 public receipts and 38 focused DNS/HTTP/alias-deadline witnesses passed their schemas and
fixture assertions. Clippy, format, specification and whitespace checks passed. The retained
R0 binary reproduces all eight original red witnesses; its expected checker exit is 1.
The corrected low-level DNS exchange retains its Tokio provider, uses one TCP connection and
zero UDP retries, accepts only service-associated owners/types and follows at most eight CNAME
hops. Attempted-question metadata describes transport requests, not proof of packets transmitted.
Every header/chunk-framing byte now counts toward 8192 bytes: exact passes, crossing fails.
No dependency package, version or feature changed during R1.

Root matched both returned manifests (eight changed paths and 70 production/build/test paths),
the machine packet and the ordinary candidate. A focused private copied-installation spot passed
five actual observations with four distinct DNS questions on one TCP connection. A separate
8193-byte framing witness failed with `http_headers_too_large`, as expected. Logs:
`/tmp/agenticsre-diagnostics-root-r1-cname.log`, SHA-256
`f194497bee3e4165cbe9d6559a318ab61bcb86181f58441ca3679a2e1b8ae795`, and
`/tmp/agenticsre-diagnostics-root-r1-framing.log`, SHA-256
`7a98e89f4f8ce1079110b25594060725a3c86ce74729018b0418c96a4106647d`.
Candidate `target/diagnostics/save-r1-candidate`: SHA-256
`fcb5a72bd48f3c7faa69cc0ab99b0b18cfb3463fac453c469b3d6401745d95d3`, 38,975,168 bytes.

**Completed R1 cache cleanup:** No builder or fixture process remained. Root removed only the
completed ignored `target/debug`, retained all three diagnostic binaries as mode-0444 DATA and
verified all three earlier development archives unchanged. Source refs, status, bytes and modes
for all 340 files remained unchanged. Free space rose from 7,595,450,368 to 8,697,610,240 bytes,
passing the 8 GiB floor. Evidence `/tmp/agenticsre-diagnostics-r1-cache-cleanup.json`, SHA-256
`b5eeb9166a34a69c6a2ab1bf3b609cc4e16a58133112e1f2a075afd37f349f56`.
Local results alone do not accept WORKBENCH-010 or enable hosted diagnostic grants.

**Immutable R1 review target:** `/tmp/agenticsre-diagnostics-r1-snapshot-oknmjwv0`, candidate
`0e7ec5388bcc96def61571b500e8159deea00621`, tree
`d9d64fc09a80f27a327718fa7892fba927c9d688`, parent `2c4e422`.
Manifest `/tmp/agenticsre-diagnostics-r1-snapshot.json`, SHA-256
`c2a4100c4f209575e7f47e58a45e5df551e3a03824fff4192a395df9fc9416ce`:
340 regular files, 4,003,181 bytes; 336 Git mode 100644 and four mode 100755. Root and the
independent verifier matched all captured bytes; source refs/status/bytes/modes and the R0 original
stayed unchanged during capture. The R1 original is sealed read-only and clean. Ten paths differ
from R0: eight builder correction paths and two coordinator-owned prose documents.
Readback `/tmp/agenticsre-diagnostics-r1-verifier-prep-v5ub1bzt/capture-readback.json`, SHA-256
`6b482a33ca89810c45fa15bd305c99bb105687b992f5e5a201108e5f97f19584`.
Affected reviews use that exact original; native staging and runtime remain gated.
This subsequent roadmap entry records the capture without changing
frozen product bytes; no source commit, push, deployment or live integration occurred.

**Independent R1 correctness:** ACCEPT/APPROVE, no reportable findings, independently found
P0/P1 count 0. The reviewer traced the changed DNS/HTTP implementation and locked runtime/transport
libraries, schema/provenance agreement, whole-address/selected-peer policy, deadline/cancellation,
stop/concurrency/receipt handling and immediate CLI/hosted authority seams. No mid-edit file was
skipped. This is static evidence on `0e7ec538`, not an execution PASS.

**Independent R1 security:** ACCEPT, P0/P1 count 0, with no validated remaining findings across
all severities and no unresolved P0/P1 candidate. The auditor matched all 340 frozen files and
traced the complete changed DNS/HTTP paths plus immediate dispatch/lifetime/authority seams,
reusing accepted coverage of unchanged input/configuration/process/TLS/hosted paths. DG-SEC-01
and the reverse-zone conformance concern are resolved in source. This is a scoped static gate;
actual packet counts, deadlines, signal/consumer cleanup and platform prerequisites still require
independent execution. Fix/review round 1 is resolved; no second round is needed at this boundary.

**Native release boundary:** Both independent affected reviews accept exact candidate `0e7ec538`.
Root rechecked that sealed commit/tree/manifest and a fresh 8,686,460,928 free bytes at
2026-10-03 06:15:59 UTC, passing the 8 GiB pre-staging floor. The independent verifier may now
run one prepared COPY-only Cally lane using a separate execution copy and the existing bounds.
Standard private resolver/proc fixture views may supplement the matrix without adding host
mounts, capabilities, network or sysctl changes. An unsupported fixture prerequisite remains a
named gap; it is never substituted for successful product execution. No native result was claimed
at this release boundary.

**Cally attempt 1:** The exact released controller (session 15536) is terminal exit 1, not a
polling timeout. Evidence `/tmp/agenticsre-diagnostics-cally-rb6dntnd`, image
`sha256:f127ed4bfe9dcfd5aabef2480ef20a70af2cbd2f3a089024bd677dcefc3d10d2`.
Workspace/all-feature tests, the native MCP grant regression, ordinary build, exact literal mission
and public five-check skeleton passed; preceding independent cases included actual IPv6.
The independent matrix then stopped in fixture creation: Cally OpenSSL rejects `x509 -days -1`
with `Error: end date before start date`. No expired-certificate product invocation occurred.
This is a verifier fixture failure, not a source finding or accepted diagnostic result.
The ordinary Cally binary is retained as mode-0444 DATA, SHA-256
`b33d7f2dc713cfb4e2682d6db48d9a2c91a45771c0412c126ceda92480674eea`, 38,908,104 bytes.

The final report also exposed two verifier lifecycle defects: Python PID 1 retained 19 adopted
zombie-only test descendants; local deletion encountered readonly copied `.git` directories.
The verifier removed only its exact terminal staging, confirmed sealed source bytes unchanged,
and independently confirmed container/image absence and cached base unchanged. All namespace
processes disappeared with container removal; the earlier internal residue check remains FAIL.
Raw commands, failed receipt coverage and cleanup reconciliation remain preserved in that packet.
Prepare ordered past validity dates, explicit adopted-child reaping and copied-directory cleanup,
then resume the remaining matrix with the same verified Cally binary. Reuse passed cold build/tests;
no product edit or second review/fix round is warranted by these fixture/lifecycle failures.
No continuation job is released until the corrected private driver and partial packet return.

**Sealed partial verdict:** `/tmp/agenticsre-diagnostics-cally-rb6dntnd/VERIFICATION.md`, SHA-256
`6d4a91fdc9760bd7287cb06ccc5a33e4d76c12677c99019e9683a8aaeac9bda5`.
Overall product acceptance is INCONCLUSIVE; verifier execution is FAIL. The completed cold lane
passed 167 tests (19 existing ignored), one native MCP grant case and the installed missions.
The independent portion produced 114 receipts, including all 33 R1 DNS/framing cases; with two
builder receipts and the literal mission, all 117 exact canonical receipts and 51 diagnostic data
objects passed their frozen schemas. Root matched the ordinary Cally binary and all 79 sealed
evidence files (45,194,780 bytes), including those exact JSONL receipts. Artifact manifest SHA-256
`ad3bbc93d5c22ec2aa9459e4f0a4e689e0f31280da70ef67eed953722f869a23`.
Expired-leaf product refusal, the broader HTTP/body matrix, process-state negatives, public signal/
consumer closure, supplemental resolver/proc views and public Grafana regression remain pending.
The corrected private continuation is preparation only; passed native proof will not be rebuilt.

**Repaired continuation release:** The private preparation passed 12 DATA-only checks and retained
all sealed source/first-packet bytes. Root matched all 23 declared support identities and inspected
the artifact/source/runtime guards, explicit ordered-date/chain/name/key witness, PID 1 adoption/
Z-only reaping and owned-directory cleanup. Preparation
`/tmp/agenticsre-diagnostics-cont-verifier-prep-o9re3q3s/repair-preparation.json`, SHA-256
`6e7854ceade7f08f05fda024f341d1e15f8c487fc7d836d25a8c8da41c143c8b`;
repair-data evidence `e30ecd863c6464988a076dbba540f31deb5ceeb21a227d5d748e84a0c8dd0d57`.
Root released one artifact-only COPY lane after a fresh 8,638,484,480 free bytes passed the floor.
It reuses exact source `0e7ec538` and ordinary Cally binary `b33d7f2d`, performs no Cargo/vendor
build, and lowers the outer ceiling to 420 seconds (240-second matrix and 90-second Grafana).
Actual certificate/reaping witnesses and remaining acceptance are still pending. A failure stops
that lane without blind retry; completed first-attempt proof keeps its original attribution.

**Artifact continuation attempt 2:** Handle 12923 reached terminal exit 1 in 1.35 seconds, before
any new product invocation. The native ordered 2020–2021 leaf creation and same-key checks passed;
OpenSSL 3.5.6 rejected its chain/name self-check with error 79 at CA depth 1 (`invalid CA certificate`).
The fixture's unextended `req -x509` generator suppresses ambient configuration; actual failed
CA/leaf PEM bytes were not exported before assertion, so their extension contents still need a
native diagnostic witness. No product source or trust/expiry finding is established.
The real PID 1 adoption → zombie → `waitpid` exit-23 proof passed, final residue was empty, and
all owned staging/container/tag/image-ID cleanup passed with source and first-packet bytes intact.
Sealed packet `/tmp/agenticsre-diagnostics-cont-cally-nirie7sk/VERIFICATION.md`, SHA-256
`2474ea20d6178df52ffada5c20bf88b2e53159e0ab295a5b54d1ef44d6a3c8d6`;
65-file manifest `242fc4820b3f0c5fb1afd4e2f32c2f37eb94d3a3c16470f02f67988a6814662d`.
Root matched all 65 artifact hashes. Overall product acceptance remains INCONCLUSIVE; no new
receipts were produced and the original 117/33-case proofs remain unchanged.

**CA fixture diagnosis:** Root authorized one standard Python/OpenSSL fixture-only red/green
checkpoint on the exact cached Cally runtime, with no product invocation or Cargo build and a
120-second maximum. It must retain actual unextended-CA certificate bytes/extension facts and
reproduce the error before proving explicit CA constraints/signing usage, normally valid CA/leaf
chain/name/key and a time-only expired-leaf failure. No reference CA/name policy may be weakened.
The full remaining product matrix stays unreleased until this diagnosis/proof returns; this is
verification fixture work, not another product fix/review round. All original scope remains open.

**Fixture-only instrumentation correction:** Handle 85339 stopped before any CA or product
command because a global logging hook intercepted Python's internal `uname -p` platform query.
The failure packet remains sealed at
`/tmp/agenticsre-diagnostics-ca-proof-cally-bx2ijvi0/VERIFICATION.md`, SHA-256
`48e70891aff5327cf2f60db6db8ccd0c127426fbc53962e18af3178e4b97ed2f`;
root matched all 26 artifact files and exact owned cleanup passed. A module-local run logger and
`os.uname()` provenance replace the global hook; approved CA helpers and 120-second driver are
byte-identical. Root inspected preparation `636f65f9f76810f670c51f0d259d5935bef5bd2826b38e87756378318f621f3b`
and released one corrected fixture proof after the fresh 8 GiB staging floor passed.

**Native CA red/green proof:** Handle 79172, evidence
`/tmp/agenticsre-diagnostics-ca-proof-cally-t_kyyd89`, returned own exit 0 after 44 actual OpenSSL
commands and zero product invocations. The old V3 CA lacked Basic Constraints and Key Usage and
failed chain validation with error 79 at depth 1. Explicit critical CA/signing extensions passed
normal current CA self-signature and leaf chain/purpose/name verification; wrong name failed with
error 62 at leaf depth 0. The same-CA/key expired leaf passed only time-disabled chain/name
verification and normal verification failed solely with error 10 at leaf depth 0. Actual public
PEM/DER/extension and command evidence is retained. Final independent readbacks confirmed exact
container/tag/image-ID absence, cached base unchanged and empty owned residue. Root matched all
42 artifact hashes and six public PEM/DER digests. Sealed packet SHA-256
`163212bb918ac1b50aec8451b0f713ebf40be423eda268d4195ada15ee08e9fe`, manifest
`80fc75dd6eb12e692afe8274b210ec43402c08dd8c664d4e1ff9842f9b499574`.
The exact packet/native observations are copied into `docs/evidence` and linked in the verification
record. Product source and fix/review counters remain unchanged.

**Remaining product lane released:** Root inspected preparation
`/tmp/agenticsre-diagnostics-cont-ca-fixed-prep-89dpw0fu/repair-preparation.json`, SHA-256
`7d96c1dba6ff4222e0c2de8fc3344fd40557f9fb47c8f8b6fbb897932de018c5`, all 23 declared
support identities, the CA-only diff and 12 DATA-only checks. The accepted private controller,
lane and exporter are unchanged; only the two proven CA-generation extensions differ from
attempt 2. Root released one same-binary artifact-only remaining matrix after fresh
8,593,874,944 free bytes passed the pre-staging floor; the driver must check the floor again.
The exact frozen source, b33d7f2d binary, cached runtime and 420/240/90-second ceilings are retained.
No Cargo/vendor work or passed-prefix replay is required; current actual execution/terminal results
must be reported before any acceptance claim.

**Artifact continuation attempt 3:** Handle 69881 reached terminal exit 1 at a private verifier
assertion, after actual expired-certificate refusal and 27 HTTP parsing negatives passed.
Evidence `/tmp/agenticsre-diagnostics-cont-cally-byxkg61s`, image
`sha256:f5dce2d72a89d7256c1449ad633de6b39bf507822cce19743aaffab372a18644`.
The test explicitly requested body capture for a 301 response but incorrectly required null
content. Its actual receipt reports opt-in, bounded base64 `b29wcw==` (four untrusted bytes),
failed non-2xx observation, no redirect/proxy/retry and no denied-endpoint effects. The frozen
configuration profile explicitly permits opt-in body bytes and independently marks non-2xx
observations failed. This is a verifier expectation mismatch; no product source finding is
established. All 29 new canonical/diagnostics values passed their frozen schemas; remaining body,
process, signal/consumer, private-view and Grafana criteria are still open. Final residue was
empty and driver cleanup passed; sealing and independent readback are pending integration.
The verifier must retain the original red assertion, prove the narrow corrected expectation on
the captured receipt as DATA, and check every remaining assertion against the binding contract
before another native lane. Completed criteria are retained without replay.

The third packet is now sealed: verification SHA-256
`844a4d7007147ce9406093692692cf99fd0e70761ad1c68f0b03a30b6ca5cbb2`, 70-file manifest
`34097e658c842ec29394b945a0059368e97bd7c9165c5acb2eb88e48dd05a73f`.
Root matched all 70 hashes and the exact opt-in body/no-extra-effects witness. Independent cleanup
confirmed exact container/tag/image-ID absence, cached base and all frozen sources/prior artifact
bytes unchanged; final residue was empty. Its packet and 29 exact receipts are copied unchanged
into `docs/evidence`. Combined schema-valid evidence is 146 canonical/80 diagnostic data records.
The bounded comparison of all remaining private assertions found no further definite contract
mismatch; refine signal timing to exclude subsequent fixture drainage and require actual headers
sent plus HTTP deadline state before labeling a timeout as body-phase. The proposed remaining-only
suffix retains all earlier passes and adds one missing capture-off non-2xx control.

**Bounded evidence storage consolidation:** Repeated exports held three identical non-executable
copies of ordinary binary b33d7f2d. Root atomically replaced only the second/third DATA paths with
hard links to the first retained copy, preserving every regular-file path, 38,908,104-byte content,
SHA-256 and mode 0444. No unique source, archive, receipt or failed evidence was removed. Inode,
link-count, ctime and shared donor-mtime changes are explicitly recorded; parent permissions and
timestamps were restored. All 79/65 first/second sealed artifact hashes and both manifest bytes
remain unchanged; the third DATA file was matched before its seal. This released 77,824,000 bytes,
raising free space from 8,543,043,584 to 8,620,867,584 bytes and restoring the next pre-staging
floor. Record `/tmp/agenticsre-diagnostics-data-dedup-yvo3fhf4.json`, SHA-256
`26b7ccc9a0c87858b58521cbf181d2da72a1abfe5a7eb086596dfe5c4aaac612`.

**Audited remaining suffix released:** Root matched all 26 support hashes in preparation
`/tmp/agenticsre-diagnostics-tail-verifier-prep-_optmcvm/repair-preparation.json`, SHA-256
`34fe700ee24f7671ad86563d625d805483991e6cc0cf548ca222c7e365724d64`, read the complete
remaining-assertion comparison and narrow checker changes, and accepted 21 DATA-only checks.
The original 301 expectation is retained red, corrected semantics pass on the exact captured
receipt and semantic mutations are rejected. Driver, exporter and CA helper remain unchanged.
Root released one remaining-only lane after 8,618,942,464 free bytes passed the staging floor;
the original 146 receipts/33 DNS-framing cases and passed expiry/parsing work are not replayed.

**Remaining suffix / observer failure:** Handle 31890 reached terminal exit 1, but its main
diagnostic suffix returned own exit 0: 28 public invocations, 27 receipts and the deliberately
undeliverable closed-output case. Body bounds, actual process identities/stopped/zombie/gone,
signals/deadlines, consumer closure and genuine inaccessible/non-proc metadata views passed.
The valid system-resolver fixture hit a genuine prerequisite gap: UID 1000 cannot bind its
private port 53 (EACCES 13). That specific positive/fallback traffic proof remains INCONCLUSIVE;
the boundary is unchanged and no capability, UID or sysctl relaxation is authorized.

The subsequent Grafana regression reached the private log observer's 128 KiB per-line bound
on a large selected-data record. The 145,925-byte partial line remains retained and uncounted.
The observer's first force-removal returned 125 and masked its primary exception; final driver
cleanup reconciled exact container/image removal. The Grafana terminal status, post-identity
checks and final DATA export were not reached at that attempt. The final continuation below
retains every complete exact record, proves a bounded observer correction against the frozen
Grafana limits as DATA and records primary/cleanup errors separately. Positive system-resolver
traffic remains a named prerequisite gap.

**Final qualified acceptance:** Handle 76710, final controller and both remaining native checkers
returned own exit 0. The completed Grafana regression produced 96 schema-valid canonical receipts,
56 dashboard-data and 33 query-data values. Malformed/empty system-resolver views passed without a
port-53 listener, with unchanged private IPv4/IPv6 UDP-send counters and no DNS/service effects.
Final source/binary postguards, empty native process inventory, fixed DATA export and independent
exact container/tag/image-ID/staging cleanup passed. No product source or ordinary binary changed
during the continuations; passed 167-test/33-DNS-framing/expiry/suffix evidence was reused.

The final packet `/tmp/agenticsre-diagnostics-cont-cally-i5soqfo6/VERIFICATION.md`, SHA-256
`685da9ea4aa1b038b01966b90052cf666b484162ac9a61e3f46d02920379884b`, is copied unchanged into
`docs/evidence`. Root matched all 70 final artifact hashes; manifest SHA-256
`59d99a3d33ede3f771eab03aae449ddad13b11924e04fb1e9101fafeec5a309b`.
The copied accepted index, SHA-256
`cdd2813a9da2dde9c52dd77001ca64efe5f37154d31699d0a55911d72102edd6`, selects 271 canonical
receipts (175 baseline/diagnostic/resolver plus 96 full Grafana), 109 diagnostic-data and 89 full
Grafana-data values. Earlier interrupted Grafana observations and its partial line stay preserved
separately. Exact suffix/final resolver receipts are also copied into `docs/evidence`.

The private observer's repaired line limit is 6,295,552 bytes, derived from the frozen 2 MiB UTF-8
envelope and JSON ASCII-escape expansion; the total log cap remains 16 MiB. Nine observer DATA,
21 other DATA and 19 fixed export-parser checks passed before the native green. The full final
8,724,397-byte raw log validated against frozen schemas, record SHA-256
`d54b434c5ca22c5fc881b63014b6cdf60c74f86a242ebf0e9a282b99e6eead1f`.
Final independent cleanup readback is
`9f282a80253916b8ea894c887780935a47188e3e75a3714129e5f4e0876edc18`.

Two further root-owned readonly DATA consolidations preserved every logical path, size, byte hash
and mode, with inode/link-count/ctime/shared timestamp changes explicitly recorded. Identical
archive copies released 80,736,256 bytes, record
`/tmp/agenticsre-diagnostics-archive-dedup-g06ybszw.json`, SHA-256
`180f6acb38f18646674cafa8ccd5a13abfd317abab77ba4c0242f40dc4bf7b18`.
The final ordinary b33d7f2d export was hard-linked to its first identical DATA copy, releasing
38,912,000 bytes, record `/tmp/agenticsre-diagnostics-final-data-dedup-g1_lhktg.json`, SHA-256
`7db102eb3e3e27396fef894d15e20847d3ba6987ee9eb729afcb0af9947ea506`.
All protected archives and sealed manifests still match; future changes must replace these
shared readonly inodes rather than overwrite them. These historical floor recoveries do not
establish free space for a later staging operation.

## WORKBENCH-011 — browser diagnostics

**Status:** Local backend/frontend checks passed and the R0 candidate was frozen. Independent
security ACCEPT found no validated or unresolved findings. Independent correctness requested a
scoped failure-caption correction (one P2, P0/P1 count 0); fix/review round 1 is resolved.
R0 native execution was never released. Corrected R1 passes both affected static gates and now has
qualified native API/browser PASS. Earlier private staging/assertion failures remain recorded below;
the same-binary suffix finished all remaining criteria with exact guards and independent cleanup.
The owner paused at this completed milestone. See [browser acceptance](diagnostics-ui-verification.md).
**Mission:** An explicitly granted alias runs the real connectivity pack through browser → HTTP →
core, renders all five outcomes, preserves the exact receipt in session history/download and
cancels witnessed in-flight network work. The existing light/dark/mobile/keyboard design is the
accepted visual baseline; this adds controls/results within it.
**Contract/owners:** [Browser diagnostic contract](diagnostics-ui.md) specifies trusted identity,
submission and result behavior. Backend builder owns the finished OpenAPI artifact, UI server
admission/state, minimal CLI/core identity seams and their tests/checker. Frontend builder owns
generated TypeScript consumer, form/result/history components, styles and actual browser fixtures.
Coordinator owns this roadmap/initial contract; there is one owner for each shared artifact.
Backend finishes the concrete API example/normalization contract before frontend work consumes it.
Then independent file ownership permits parallel implementation; Cargo still has one builder.
Session configuration/CA identity means exact pinned bytes plus the accepted regular-file/path
checks. Changed bytes or invalid replacement are refused; a valid byte-identical replacement
does not expand authority and is not an inode-based identity change. This clarifies the future
contract before implementation. WORKBENCH-010's sealed accepted bytes remain unchanged.

**Development milestones:** OpenAPI artifact 0.1.2 retains wire `api_version` 0.1 and exposes
explicit target grants, the installed fixed pack and separate target/root submission identities.
Backend workspace checks passed 175 Rust tests with 19 existing native tests ignored. The local
raw API proof passed real five-check execution, exact receipt history/download and CLI/structured
parity, defaults/effective-limit deduplication, conflicting aliases/limits, configuration/CA
mutation refusals, byte-identical replacement, partial/stop/body behavior and witnessed cancellation.
The final ordinary candidate's API checker exited 0 with 235 schema validations and 16 exact
receipts; this remains development evidence for those bytes, not independent final acceptance.

The first actual Playwright matrix passed all five observations, CNAME/original-name separation,
light/dark/mobile/keyboard flows, history/download equality, lost-admission reconciliation,
partial/stop/serial controls, inert opt-in body capture and cancellation after actual GET.
Frontend final inspection then corrected independent root/target URL-selection preservation and
rounded observation numbers that could corrupt displayed PID/UID identities. Focused red/green
regressions are retained and pass. The final asset/browser rerun is complete: 49 frontend tests,
typecheck/lint/generated/build and actual Playwright exited 0. Fourteen exact receipts passed 28
schema validations. The executed privately installed copy came from readonly DATA
`target/diagnostics-ui/save-final`, SHA-256
`368f58838ae68dc31c716269c51f0a71af53d084d0a1c502c8525223c3e8ecbe`; manifest
`target/ui/diagnostics-frontend-exact-identities/diagnostics-browser-receipts.json`, SHA-256
`590ddc3ac1b05826ca384d73b9c3e67f3dd80ff45d6ea3b30a96d423ef902aa3`.
Cached dependencies and existing private sandboxes are reused; no download, host listener or
privilege change occurred. Both builders are quiescent; immutable capture and affected gates are
next, with a fresh 8 GiB floor required before capture/staging.

**Completed cache and retained DATA:** Root removed only this checkout's completed `target/debug`
after both builders confirmed no users/jobs remained. All 358 source bytes/modes/refs/status and
187 current binary/evidence/archive paths matched before/after. The 1,236,299,776-byte free-space
gain still left the floor unmet. Record `/tmp/agenticsre-diagnostics-ui-cache-cleanup-yhypez0w.json`,
SHA-256 `f0a1c41f81d062fab7bb1a7c2e55b3d89a5c46328375e13974bcb251baafa8c1`.

Two historical debug-heavy Grafana baselines are now retained as mode-0444 lossless DATA archives
under the original checkout's `target/baselines/`: `grafana-initial-save.data.gz` and
`grafana-r1-save.data.gz`. Their decoded SHA-256 values are respectively
`686cf188c73975adc549b319c2a37ead4d14af89fff4e95d0d5ee975298c41a1` and
`67befd5ec2c18787aeb59c867393c983b40963d6ef06b150182a997711c69ae5`, verified against the actual
125,398,792/125,438,032-byte originals before retiring the uncompressed path representations.
This changes storage representation, not unique binary content; `target/baselines/retention.json`
records restoration size, hash and original mode. Decode into private scratch and verify those
facts before any future executable use. Current candidates and all three development release
archives remain unchanged. Net logical storage saving is 186,123,630 bytes; actual free space
reached 8,618,618,880 bytes. Record
`/tmp/agenticsre-debug-baseline-lossless-retention-suhva2tv.json`, SHA-256
`4d48c80427ee5d60bc0f058cf72f7e67942b392c9f8ce8973517ab249c489243`.

Five identical readonly source-archive DATA copies, SHA-256
`434a1cfd039b3a6d56ab4875a2f586c24bdf33775e8c0816e8ba4df0da41e0ec`, were atomically hard-linked
to their first retained copy. All logical paths, bytes, sizes, modes and sealed manifest bytes
remain; archive inode/link-count/ctime/shared timestamp changes are explicit. Snapshot source
inodes are untouched. This released 17,088,512 bytes, reaching 8,635,555,840 bytes free. Record
`/tmp/agenticsre-diagnostics-source-archive-dedup-w3bwx8yz.json`, SHA-256
`eee3b1b08638ee92ffd5fedec2ce330ea1eef67f0821d6cf5e1a05d0eb677e1b`.

**Immutable browser review target:** `/tmp/agenticsre-diagnostics-ui-r0-snapshot-44t1hasz`, candidate
`c72af6c653f66cfadc7d6e0c9fd69145fd12b296`, tree
`d3b97c3b2a201d252088997483c9666f80424d61`, parent `2c4e422`.
Manifest `/tmp/agenticsre-diagnostics-ui-r0-snapshot.json`, SHA-256
`85dfa0f05da6de95b931376660cd5969fdb5bfdd8ff1d7667611589719e36600`: 358 regular files,
5,385,643 bytes. Fresh free space was 8,634,654,720 bytes before capture. The original is sealed
readonly and Git-clean; root matched all source hashes and the accepted R1 original unchanged.
Readback `/tmp/agenticsre-diagnostics-ui-r0-capture-readback-v94ov_61.json`, SHA-256
`61595dce63c35bce6c689b287e353ece00ec1edf495a9a5d6455779396b75101`, lists all 29 affected
product/build/frontend paths. Both independent affected static reviews completed against this
exact original. This disposable capture changes no working source ref or published commit.
Root's subsequent progress entry is outside frozen executable source.

**Independent R0 gates:** Security ACCEPT, zero validated findings and zero unresolved candidates
(P0/P1 count 0), after the complete affected authority/input/worker/history/lifecycle/rendering/
provider source-to-sink trace and immediate callers. It matched all 358 frozen files, unchanged
dependency locks, selected cached Rust archives/source and frontend package manifests. The existing
unverified npm-archive integrity and runtime/platform limits remain explicit. This is static
evidence, not executable acceptance.

Correctness REQUEST CHANGES found one P2 at `web/src/DiagnosticResult.tsx`: failed TCP observations
retain an attempted peer, and denied DNS observations retain their answers, but the UI captions
claimed "Connected peer" and "Accepted service addresses". The full affected review found no
additional reportable defect and independently counted P0/P1 as 0. Frontend owns the neutral
captions, focused renderer red/green cases retaining genuine failed observation fields and rebuilt
dist. No backend/core/network source change is required. The accepted backend/local predecessor
evidence remains attributed to its old assets; final ordinary assets will be built and exercised
on Cally after the corrected snapshot's affected gates pass. Fix/review rounds: 1 in progress;
incomplete builder returns: 0.

**R1 builder correction:** Failed observations now use "Attempted service peer", "Observed service
addresses" and neutral "Service peer" detail. Nonempty retained observations no longer say
"No observation returned"; their summary says "Observation details below". Focused renderer cases
preserve denied DNS addresses with null selected peer and refused TCP peer/hostname/port fields.
Proper caption/fallback red evidence is retained; 15 renderer tests, typecheck, lint and asset build
exited 0 through the existing private cached-provider sandbox. Only the renderer, its tests and
dist changed; Rust/backend source and existing API proof are reused. Final JS
`web/dist/assets/index-C3-PEPjo.js` has SHA-256
`c563559765d64b7623776a3d5da071b0b95ed12440056ba1ec56c7c174fc54ce`.
No new ordinary binary or browser PASS is claimed before the corrected frozen Cally build/run.

**Corrected immutable R1:** `/tmp/agenticsre-diagnostics-ui-r1-snapshot-0rzjumlc`, candidate
`bca5b19a8d448ac72202f30ff120446127582a38`, tree
`4f221ff5cad9fe78342e25c866a0258529e4622c`, parent `2c4e422`.
Manifest `/tmp/agenticsre-diagnostics-ui-r1-snapshot.json`, SHA-256
`0b942d654f4013ed59e3b9e895ca08ee108eb4bd3a1c6c1dea7a7fb08bf11477`: 358 files, 5,391,853 bytes.
Fresh 8,613,580,800 free bytes passed the capture floor. Original is readonly and clean; all source
hashes match. Readback `/tmp/agenticsre-diagnostics-ui-r1-capture-readback-zqainor2.json`, SHA-256
`c12877a98c02fe00fadcbac972416326f6862cceb6d46f49dbe10e148dcab7a6`, confirms only renderer/tests,
dist index/old-new JS and roadmap differ from R0; both frozen originals remain unchanged.

**Independent R1 gates:** Correctness ACCEPT/APPROVE resolves the P2 and reports no new findings,
independent P0/P1 count 0. Security ACCEPT reports zero validated findings and unresolved candidates,
P0/P1 count 0; the whole new bundle equals R0 with exactly four reviewed caption/fallback
substitutions. Complete affected R0 coverage carries for identical backend/authority/API/lifecycle/
dependency/provider bytes. These are static gates, not a runtime PASS. Fix/review round 1 is resolved;
no second round is needed at this boundary. Incomplete builder returns remain 0.

**Frozen native release:** Root matched all 19 sealed support files and read the exact driver/lane/
resource/cleanup plan. Preparation
`/tmp/agenticsre-diagnostics-ui-r1-release-prep-lQUHzLBM/release-preparation.json`, SHA-256
`8fe74e3479414858f1959a28ad52a4e27f5b16b4d90412a286987c2e8d4b3124`, binds the affected cold
workspace/native MCP/ordinary build and independent actual API/browser mission to R1. Six minimal
rebind DATA checks passed; the earlier 26+19 parser, 31 runtime-stream and 12 integration checks
retain their original attribution. The corrected browser includes real denied-DNS/refused-TCP
caption assertions. It retains the fixed nine-PNG DATA export, receipt/manifest bounds, 900-second
outer/800-second work ceiling, 180-second staging and existing private resource/isolation limits.
Sealed source and the cached browser runtime stream directly into the COPY-only image; no duplicate
local product/runtime archive is created. Driver rechecks the actual 8 GiB floor before allocation.
Root releases exactly one prepared lane after a fresh passing free-space readback; the native
command exits, final binary, screenshots and cleanup are still pending. Passed core/Grafana full
matrices are reused, and the positive system-resolver port-53 fixture gap remains excluded.

**R1 Cally staging failure:** Authoritative handle 67887 reached own exit 1 before native launch.
The COPY-only context reached image commit/blob copying, but the private driver's post-stream
`wait(timeout=20)` expired before its declared 180-second stage limit. No build command terminal
status was retained; it cannot be called successful or timeout exit 124. There were zero Cargo/
product/API/browser invocations, canonical receipts or screenshots, and no product finding.
An independent readback caught a late image after the initial absence check: exact owned tag
`localhost/agenticsre-diagnostics-ui:0rwh7ay6`, ID
`sha256:72ae02f732700ad0f59c9e2290a67b3c9074d3348a550033f3fe46c4a99498e9`, matching the R1 label.
Only that verified late image was removed; repeated exact tag/ID/label/container absence, cached
base identity, local staging/job cleanup and all frozen source/preparation bytes were reconciled.
The initial absence race and primary failure are retained rather than rewritten as clean execution.

Root matched all 44 failed artifacts. The unchanged summary is copied into `docs/evidence` from
`/tmp/agenticsre-diagnostics-ui-cally-0rwh7ay6/VERIFICATION.md`, SHA-256
`61c5828d3bac8f7fb58daa6602499727f9d61ea8e8ac87dc0920440d755fab2e`; artifact manifest
`8e3b7de23b5ae0ff8fbb6f20e9b962ddfe83af60d97fc256f05150e5b15e0c12`.
The same immutable product/static gates remain accepted. The private verifier must correct the
single staging deadline and prove remote build/group quiescence before cleanup and another lane;
no retry was released at that boundary. This is a verifier repair, not product fix/review round 2.

**Repaired staging lane released:** Root matched and inspected the sealed 20-file preparation
`/tmp/agenticsre-diagnostics-ui-stage-repair-prep-258mryHc/repair-preparation.json`, SHA-256
`9eb457a54cfd47738af5077a1cd45dad2c353dcd81fbbe0df4606b68876364f2`. Thirty-three DATA checks,
including seven modeled control flows, passed. One 180-second deadline now covers ownership
witness, streaming and commit. A standard remote timeout sends TERM at 160 seconds and allows
10 seconds before KILL; bounded read-only tag/PID/process-group observations must establish remote
quiescence before any owned-image cleanup claim. The extra 30 seconds is cleanup observation only,
not added successful staging time. All product, native lane, API/browser, runtime/export bounds and
cached-source bytes remain unchanged; both accepted R1 static gates carry.

Fresh 8,593,944,576 free bytes passed the 8 GiB floor. Root release
`/tmp/agenticsre-diagnostics-ui-r1-repaired-native-release-0btidzdt.json`, SHA-256
`ca84cee2dcebaa7a4bc06a683abe5ad526fb6873e45d118314b0fdc14d48cbdc`, authorizes one corrected
lane, authoritative handle 59978, evidence `/tmp/agenticsre-diagnostics-ui-cally-zg8qx5ix`.
The cached base and exact-name preflights passed. Corrected staging reached own exit 0 and witnessed
timeout/sudo/podman group 553232, then established exact tag-process and group absence before native
launch. The native workspace (175 passed, 19 existing ignored), selected MCP regression (one passed),
ordinary build and literal installed mission each exited 0. The exported ordinary binary is
38,993,736 bytes, SHA-256
`566b320c424ecc583a846baeda4fccaa594ed264e6d122dbed3b321ec253344d`.

The controller ultimately exited 1: its independent API check required startup exit 2 where the CLI's
server-error branch returns 1. The actual grant-without-target refusal returned 1, empty
stdout and a clear diagnostic; no browser run occurred. This is a private checker defect, not a product
finding. Existing public fixture checks already accept the two startup refusal branches. Native PID-1
cleanup ended with no residue, and the driver removed only its owned container/image after the known
build quiescence. Independent exact container/tag/ID/label/process-group and local-job readbacks
confirmed absence; the cached base and all 358 sealed source identities remain unchanged. Root
matched all 60 sealed artifacts. The partial verification summary is copied unchanged into
`docs/evidence/diagnostics-ui-cally-partial-verification-2026-10-03.txt`, SHA-256
`dfe2b289ff7d2bee7a6a6c9fa44469598ddc53349f65d37cc6286aecb0aa9d63`; artifact manifest
`cacf96e3bc216caae3175e76523b45ad88cbfe0019874b7979fcdf7d88a908cf`. The exact single literal
receipt passed its canonical schema; zero API/browser records or PNGs exist for this partial run.
Retain the completed native criteria; the next prepared lane must reuse this verified binary for
only the remaining API/browser mission, rather than repeat Cargo or alter accepted product source.

A bounded static comparison of the private remaining assertions found additional harness-only
mismatches before another run: continue mode with an independently successful process produces
`partial`; capture-off retains body metadata with no content; installed pack discovery legitimately
names its `process` step while hiding actual configured selectors; process observation locators must
exclude nested vantage fields; and the history link's accessible name includes its live count.
Corrections and DATA controls belong only in the private verification preparation. These are not
newly executed product results or product findings, and no suffix was released by that entry.

**Same-binary suffix:** Root read the full four-file private diff and contract audit and matched all
23 sealed preparation files. Preparation
`/tmp/agenticsre-diagnostics-ui-tail-prep-os11isrh/repair-preparation.json`, SHA-256
`b86d38f8b3f4c56b1c3ac992062cbd1d698cf50be9e2ba1a9cae4462a0e6cccd`, retains six assertion/locator
corrections, a witnessed active-body cancellation precondition and 52 passing DATA controls with
three test-authoring failures retained. No product source, fixture helper, runtime stream, export
parser or remote quiescence helper changed. Native limits tighten to 420 seconds outer/390 work;
API and browser each get at most 180 seconds from the remaining work deadline. There is no Cargo,
vendor, build or passed mission replay and no local source/runtime/binary duplicate.

Fresh 8,629,514,240 free bytes passed the floor. Root release
`/tmp/agenticsre-diagnostics-ui-tail-release-h1_n0ism.json`, SHA-256
`25c085cfc84a40d24aec803a86230e1d99ea6429340ee541c2660fe8eb15942a`, releases one lane,
authoritative handle 97467, evidence `/tmp/agenticsre-diagnostics-ui-cally-jp3hu_hv`.
COPY staging, ownership/quiescence and initial frozen source/runtime/same-binary guards passed.
All six actual independent API groups exited 0 in 6.27 seconds. Real browser execution, exact
schemas/screenshots, final guards and independently reconciled cleanup remain pending.

**Final qualified acceptance:** Handle 97467 reached own exit 0. The independent actual browser
exited 0 in 7.04 seconds after all six API groups passed. Final native source/browser/binary guards
reached success and the PID-1 adopted-child reaper ended with no residue. Independent readback
confirmed exact container/tag/ID/label images, build group 565999 and local staging/jobs absent,
the cached base unchanged and frozen source/preparation/checkpoint bytes preserved. No runtime
work remains pending for this released scope.

Frozen schema validation passed 28 new canonical values, 20 diagnostic-data values, 110 API records
and 25 exact receipt-byte records. Seven browser receipts are included in the 28; the carried
literal makes 29 integrated records. Nine actual PNGs and their complete 1,914,880-byte archive
passed export validation and visual inspection; root viewed final light/mobile images. The ordinary
binary remains SHA-256 `566b320c424ecc583a846baeda4fccaa594ed264e6d122dbed3b321ec253344d`.
Root matched all 77 sealed artifacts, manifest SHA-256
`8e804cbeb05ddb83fa80f06f9719279aa322b8abf876a183b316fdd77fe2dd7a`, and copied the unchanged
final summary/index/integrated receipts into `docs/evidence`. Final verification SHA-256
`1ba15b9b7b82644b30cabc7dee12319d342ff677c064692daef63895ed095b88`; accepted index
`94fec124e54266145b32d4df8efead8cd7c491c28da4db78496f8dd2c239356c`. The named inherited positive
system-resolver port-53 fixture gap remains open and excluded. Review/fix round 1 is resolved;
incomplete builder returns remain 0. The [acceptance record](diagnostics-ui-verification.md) binds
each completed criterion to its actual source/build/evidence scope.

**Precursor DATA retention:** Three old local UI precursor binaries (`save-before-assets`,
`save-assets-1`, `save-candidate`) are retained as readonly lossless `.data.gz` archives in
`target/diagnostics-ui/`. All original decoded hashes/sizes were verified before retiring their
uncompressed representations. The 88,410,272-byte net saving restored 8,639,311,872 free bytes.
`target/diagnostics-ui/retention.json`, SHA-256
`e34429e9f72aa8f81fdf305ef6e7e40183740254bf07f94a0169498ca34efde3`, records exact archive/original
identities and private-scratch restoration. The accepted local `save-final`, new Cally binary,
frozen source and all failed evidence remain unchanged.

**Boundary/gates:** Reuse accepted core/network and browser evidence for unchanged bytes.
Require a real end-to-end walking skeleton, affected correctness/security review, frozen-source
independent execution and actual Playwright screenshots/receipts. No host listener, durable store,
AI chat, live service onboarding or MCP diagnostic grant is inferred. Review/fix rounds: 1 resolved;
incomplete builder returns: 0. Publication still needs its own owner request.

## WORKBENCH-012 — installed saved queries

**Status:** [Initial core/CLI contract](saved-queries.md) is prepared; implementation follows
WORKBENCH-011 acceptance when the owner resumes. Implementation has not started. It retains
CAP-11 / REQ-08 / AC-08 / AC-16 scope with two embedded
versioned PromQL/LogQL definitions, literal typed parameters, explicit target/datasource and one
resolved parent time window. Existing Grafana transport, permission, redaction, limits and
cancellation are reused; browser/MCP grants remain a dependent increment. No query operation,
binary, network result or executable acceptance is claimed yet. The contract and its specification
registration are subsequent planning changes outside the frozen WORKBENCH-011 source.

## Remaining product queue

The following ledger preserves the complete requested scope. Accepted slices are evidence for
their named boundaries, not completion of an entire capability or its live/platform acceptance.
Requirement and acceptance IDs remain authoritative in [capabilities](sre-workbench/capabilities.md)
and [verification](sre-workbench/verification.md).

| Capability | Evidence available | Requirement still open |
|---|---|---|
| CAP-01 command execution/inspection | [Linux runner](poc-verification.md) and [restricted Git/rg](read-profile-verification.md) | Remaining reviewed host/service/disk forms and native platform cases |
| CAP-02 PowerShell/shell | Generic human literal-argv runner exists | Named reviewed PowerShell/Bash entrypoints, native quoting/profile/pipeline tests and later explicit shell grant |
| CAP-03 Grafana observations | [Dashboard/PromQL/LogQL controlled fixtures](grafana-verification.md) | Selected live compatibility; search/history/alerts/inventory increments |
| CAP-04 named tasks | [Dashboard hygiene](dashboard-verification.md) and [error budget](error-budget-verification.md) | Additional reviewed task/runtime/platform bindings and compatibility |
| CAP-05 CLI/discovery | Accepted public CLI, help, JSON, availability and refusal paths in the linked slice records | Discovery/help for each future capability and native host usability |
| CAP-06 MCP | [Bounded stdio tasks, restricted commands and Grafana](mcp-verification.md) | Actual selected agent-host registration; context/diagnostic and future operation grants |
| CAP-07 context | [Offline core/CLI source resolver](context-verification.md) | Approved real team source, hosted context grants and live target selection |
| CAP-08 diagnostics | WORKBENCH-010 core/CLI and [WORKBENCH-011 browser](diagnostics-ui-verification.md) qualified native acceptance with frozen correctness/security reviews | Positive system-resolver fixture gap and later live/platform bindings |
| CAP-09 evidence collection | Core bounded receipts and session-only browser history exist | Product collection, durable versioned bundles, privacy/quotas and partial-store recovery |
| CAP-10 comparison | No product comparison is accepted | Compatible bundle/run validation, named normalization and honest missing/incompatible comparisons |
| CAP-11 saved queries | Bounded raw Grafana query adapter is accepted | Installed versioned PromQL/LogQL catalog, typed dialect-safe parameters and one resolved time window |
| CAP-12 runbooks | No product runbook execution is accepted | Reviewed observation-only steps, typed branches/checkpoints and version-aware resume |
| CAP-13 configuration checks | Dashboard task implements a supported offline validator slice | `config.check` contract, declared kind/version/digest and other supported validators |
| CAP-14 offline investigation | No product evidence replay is accepted | Bundle validation and deterministic no-network analysis with original observation times |
| CAP-15 reports/handoff | No product report generation is accepted | Evidence-backed Markdown/JSON, missing checks/redaction and reviewable knowledge proposals |
| CAP-16 toil | No product toil aggregation is accepted | Opt-in metadata, baseline/candidate samples, retention and cost-aware arithmetic |
| CAP-17 extensions | Compiled built-ins exist | Reviewed external package protocol/inspection/install, independent grants and fourth-capability experiment |
| CAP-18 jobs | Foreground cancellation and session invocation handles exist | Durable jobs/events, reconnect, duplicate admission and restart reconciliation |
| CAP-19 remote execution | Cally is a development verification boundary | Product runner identity/protocol, target-side policy and partition/cancellation conformance |
| CAP-20 multiple targets | No product fleet collection is accepted | Frozen bounded target sets, per-target outcomes and fair deadline/failure control |
| CAP-21 scheduling | No product scheduling is accepted | Owned persistent read schedules, timezone/overlap/misfire/restart rules |
| CAP-22 controlled changes | No live write or change grant exists | Named legitimate consumer, accepted executor, exact-plan approval, readback/reconciliation/recovery |
| CAP-23 browser UI | [Local browser commands/task/results/history](ui-verification.md) and [diagnostics](diagnostics-ui-verification.md) | Context and evidence/compare/report views, then permitted remote/team surfaces |
| CAP-24 integrations/visual evidence | Diagnostic network/process and Grafana API fixture slices exist | Every planned connector family retains its separate API/identity/limit/acceptance increment; requested operational visual capture remains separate |
| CAP-25 installation/diagnostics | [Linux development archives, doctor and private install/rollback](packaging-verification.md) | Release/dependency clearance, platform artifacts, online doctor, store migration/uninstall and human/agent pilot |

The [browser diagnostics increment](diagnostics-ui-verification.md) is now qualified PASS. It grants
explicit target aliases, pins configuration/CA identities and exposes five actual observations
through the existing form, result and session-history flow. Generic hosted/MCP diagnostic refusal
remains its existing boundary. The owner paused before the next implementation increment.

After the browser increment, build saved queries before evidence collection so captured queries
have an installed definition, typed inputs and one parent time window. Then implement collection
and comparison against those versioned receipts, followed by runbooks, offline replay, reports and
toil metadata. Complete the configuration-check operation alongside these investigation features.
Extensions/jobs, remote/fleet/scheduling and a named controlled change retain the delivery plan's
dependency order. Connector work and native/live acceptance keep their own prerequisites; an
unavailable live source does not block authorized offline implementation.

The remaining product queue is unchanged: native Windows and
restricted-profile namespace-refusal evidence; release/dependency clearance and the AC-36 human/
agent pilot; actual team source selection (DEC-09); and the P3 diagnostic/query/capture/compare
features in the delivery plan. The context-containing Linux artifact now has its own source/build
identity and installed mission evidence. Keep the earlier accepted archives and frontend/runtime
proofs as baselines. Source-aware UI/MCP context grants and live integrations remain separate work.
