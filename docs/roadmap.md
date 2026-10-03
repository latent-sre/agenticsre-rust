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

## Remaining product queue

The remaining product queue is unchanged: native Windows and
restricted-profile namespace-refusal evidence; release/dependency clearance and the AC-36 human/
agent pilot; actual team source selection (DEC-09); and the P3 diagnostic/query/capture/compare
features in the delivery plan. The context-containing Linux artifact now has its own source/build
identity and installed mission evidence. Keep the earlier accepted archives and frontend/runtime
proofs as baselines. Source-aware UI/MCP context grants and live integrations remain separate work.
