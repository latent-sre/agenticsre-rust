# Product roadmap

**Status:** Live product work queue, established 2026-10-02. The owner selected
latent-sre/agenticsre-rust as the working repository. Scope and dependencies live in the
[planning package](sre-workbench/README.md); this file owns current execution status.

## WORKBENCH-001 — bootstrap the product

**Status:** Local PoC complete and independently verified. Broader product implementation continues.
**Owner:** Codex coordinates implementation and evidence; human owner retains live adoption/release decisions.
**Outcome:** Establish this repository as the product home, preserve the complete plan and its
provenance, and make the first command-runner slice concrete and reviewable.
**Next action:** The selected browser PoC under WORKBENCH-006 is complete. Use the documented launch
workflow; the next planned implementation increment is the deferred bounded MCP adapter. Reuse the accepted
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
| Adapter baseline | Dashboard, error-budget and native Grafana fixture adapters accepted; provenance retained in [adapter notes](adapter-notes.md) | Restricted-command evidence and local browser interface; MCP deferred |
| Hosted CI | First implementation push `93fc339` passed formatting/lint, then six read-profile fixtures failed while resolving required tool paths before product dispatch | Add explicit Git/ripgrep/Bubblewrap prerequisites and bounded build settings; verify a fresh hosted run |

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

**Status:** Deferred behind the standalone GUI by the owner's explicit choice on 2026-10-02.
The [implementation boundary](mcp.md) is prepared. Official references and the published 3.5.0
checksum were checked; bounded read-only SDK inspection is retained under
`/tmp/agenticsre-rmcp-source-rz_317ls`. It supports current and legacy revisions, but its default
framing/dispatch do not establish the required application bounds. No SDK execution, repository
dependency, MCP registration or live connection was created.
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
