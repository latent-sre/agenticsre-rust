# Capability specifications

All 25 capabilities are included in the requested product scope. Phase assignments specify delivery
order, not removal from scope. Each capability inherits the request, result, permission, evidence,
limit and compatibility rules in [contracts](contracts.md) and [security](security.md).
Every behavior below is proposed and unverified as product behavior.

## Complete scope and traceability

| ID | Capability | Requirement | First phase | Acceptance |
|---|---|---|---|---|
| CAP-01 | Common command execution and inspection | REQ-01 | P1 | AC-01 through AC-06 |
| CAP-02 | PowerShell and explicit shell execution | REQ-01 | P1 then P5 | AC-02, AC-03, AC-06 |
| CAP-03 | Grafana observations | REQ-02 | P2 | AC-07 through AC-10 |
| CAP-04 | Named scripts and tasks | REQ-03 | P2 | AC-04, AC-11 |
| CAP-05 | Human CLI and discovery | REQ-04 | P1 | AC-01, AC-27 |
| CAP-06 | Agent MCP interface | REQ-04 | P2 | AC-10, AC-27, AC-28 |
| CAP-07 | Service and environment context | REQ-05 | P3 | AC-12 |
| CAP-08 | Diagnostic packs | REQ-06 | P3 | AC-14, AC-15 |
| CAP-09 | Evidence collection and bundles | REQ-07 | P3 | AC-11, AC-13 |
| CAP-10 | Before and after comparison | REQ-07 | P3 | AC-17 |
| CAP-11 | Saved parameterized queries | REQ-08 | P3 | AC-08, AC-16 |
| CAP-12 | Guided runbooks | REQ-08 | P4 | AC-15, AC-16 |
| CAP-13 | Configuration checks | REQ-09 | P3 | AC-19 |
| CAP-14 | Offline investigation and replay | REQ-10 | P4 | AC-18 |
| CAP-15 | Reports and knowledge handoff | REQ-10 | P4 | AC-20 |
| CAP-16 | Toil measurement | REQ-11 | P4 | AC-21 |
| CAP-17 | Extensions and future capabilities | REQ-12 | P5 | AC-26, AC-29 |
| CAP-18 | Jobs and progress | REQ-13 | P5 | AC-22 |
| CAP-19 | Remote execution | REQ-13 | P6 | AC-23 |
| CAP-20 | Multiple target execution | REQ-13 | P6 | AC-30 |
| CAP-21 | Scheduled checks | REQ-14 | P6 | AC-31 |
| CAP-22 | Controlled changes and recovery | REQ-15 | P7 | AC-24, AC-25 |
| CAP-23 | Optional UI | REQ-16 | P8 | AC-32 |
| CAP-24 | Additional integrations and visual evidence | REQ-16 | P8 increments | AC-33 |
| CAP-25 | Product installation and diagnostics | REQ-17 | P1 onward | AC-34 through AC-36 |

## Common command execution

**CAP-01.** Operations process.exec and command.inspect accept a program, literal args, working
directory and limits. The user can run familiar installed tools; agents use a restricted configured
subset. Initial read forms cover git status/diff/log, rg searches, supported host/service status,
DNS/connectivity and disk observations. Selection of exact forms is a reviewed per-platform policy.

command.inspect explains availability and policy classification without executing the command.
An inspection result is advisory; execution revalidates the actual resolved command and target.
It is not a replacement for the installed hook guard or an authorization token.

The runner resolves trusted paths, suppresses uncontrolled pagers/hooks where its command profile
requires it, uses literal argv, preserves child exit status, and captures bounded stdout/stderr.
Human local policy can support broader authorized commands without making them agent-readable
operations. Generic execution defaults to unclassified effects until a specific profile establishes
otherwise. Missing binaries, invalid cwd, unsupported flags and policy denials occur before spawn.
AC-01 through AC-06 cover path/argument correctness, injected shell text and cancellation.

## PowerShell and shell support

**CAP-02.** Initial powershell.run and shell.run operate named installed tasks with typed
parameters. PowerShell cmdlets are executed through a reviewed script/module entrypoint; profiles
are disabled. Supported PowerShell versions and argument behavior are explicit in metadata.
No general expression evaluation, arbitrary module loading or caller-chosen script path is implied.

Later operator shell execution may accept explicit shell source under a distinct grant. Its effects
are unclassified unless a specific task contract establishes them. Agents never gain this route
as fallback from a rejected direct command. Pipelines retain every stage's status and cancel owned
descendants; output redirection is an explicit file effect. AC-03 includes quotes, Unicode, spaces,
metacharacters, profiles, pipeline failures and Windows batch routing.

## Grafana observations

**CAP-03.** Initial operations grafana.dashboard.get and grafana.query accept a configured target,
dashboard or datasource identity, dialect and fixed time window. Native Rust behavior preserves the
current helper's organization/datasource validation, bounded query, error handling, credential
masking and coverage limitations. Prometheus and Loki are the first query backends.

The adapter uses a tested API compatibility profile for each supported Grafana version/edition.
Legacy /api routes and newer /apis routes cannot be assumed interchangeable. Unknown server
profiles return unsupported or require an explicitly tested compatible profile; no speculative
endpoint fallback that changes scope. New dashboard models need their own parsing tests.

Later read increments include dashboard search/history, selected alert definitions/evaluation state,
datasource inventory/health and plugin information. Every operation has resource and pagination
bounds. A successful /health or HTTP response does not prove panel correctness or service health.
Query-level errors under HTTP 200 fail the requested query. Empty data, no traffic and missing
telemetry remain distinct. Rendered appearance requires CAP-24 visual evidence.

Dependencies are CAP-25 target configuration and the security provider decision. AC-07 through
AC-10 exercise org/data-source mismatch, TLS/redirect behavior, response bounds, semantic query
failure and human/agent parity. Live acceptance uses a specifically selected read-only target.

## Named scripts and tasks

**CAP-04.** task.run selects a registered task ID/version and typed inputs. First adapters are the
existing dashboard hygiene checker and error-budget calculator. Their calculations and documented
limitations remain intact. Human-readable output is not scraped into authoritative findings;
introduce a structured result seam and retain parity fixtures before exposing structured agent data.

Task metadata fixes interpreter, installed entrypoint, package digest, argument mapping, permitted
environment keys, effects and limits. Python, PowerShell and Bash are optional dependencies of
their tasks. Missing runtime returns missing_dependency with no installation side effect.
Workspace replacement, malformed results and descendant processes are acceptance failures.
AC-11 verifies result compatibility, unsupported dashboard models, failed calculations and bounded
script execution. AC-04 verifies exact installed-file selection.

## Human interface and discovery

**CAP-05.** capability.list/describe and the CLI expose descriptions, inputs, effects, dependencies,
platform support and availability. Describing a capability does not execute it. Discovery includes
why an installed capability is unavailable without disclosing protected credentials/configuration.

Help includes examples, exit codes, side effects, configuration precedence and output formats.
The text and JSON views share one result; stderr contains diagnostics. Non-TTY behavior is
noninteractive, honors NO_COLOR, and handles a closed pipe without an exception trace. AC-01 and
AC-27 validate usable help and failure output as well as successful commands.

## Agent interface

**CAP-06.** The MCP adapter provides the tool mapping in [interfaces](interfaces.md), with input
and result schemas, small descriptions and bounded results. Host-selected capability groups keep
discovery relevant. Agent identity/grants come from the trusted host, never request JSON.

CLI and MCP normalize into the same operation path. Protocol negotiation, tool names, cancellation,
errors and artifact access are tested against each actual supported host. Copilot terminal absence
is not solved by declaring an executable; an accepted MCP registration is a separate path.
AC-10, AC-27 and AC-28 require parity and denial tests, including malicious content in tool results.

## Service and environment context

**CAP-07.** context.resolve accepts service, team where needed, environment and optional deployment.
It returns resource aliases, ownership, dependency references and runbook links with source revision,
last-reviewed time and evidence state. Initial adapter reads the team's existing approved records;
it does not create a second authoritative service catalog.

Partial optional mappings are reported; absent or ambiguous required target/environment/org
bindings stop dispatch. Stale records may be displayed as sourced history but cannot silently
select a live target. Context never contains approval or credential values. Changes to mappings
are reviewable local data changes, not automatic promotion from an agent answer.
AC-12 covers ambiguity, retired services, stale revisions and differing forward/reverse dependencies.

## Diagnostic packs

**CAP-08.** diagnostics.run accepts a named pack and bounded typed inputs. First pack combines
DNS resolution, TCP connection, TLS certificate/hostname validation, HTTP response observation and
supported local service status. Checks run only against permitted hosts/ports with explicit vantage.

Each step reports its observation, timing and failure. DNS success cannot stand in for application
availability. HTTP body capture is opt-in and bounded; redirects, proxies, certificate trust and
private-network destinations follow target policy. It is a scoped diagnostic, not a network scan.
Packs have bounded parallelism and independent branch continuation; stop rules are explicit.
AC-14 and AC-15 cover partial success, parent deadlines and target substitution.

## Evidence collection

**CAP-09.** evidence.collect binds a service/environment/window to a selected pack and produces a
versioned bundle of individual results. The bundle includes source identities, observation times,
query/command versions, coverage, errors, redaction/truncation notices and artifact digests.

Capture is explicit and bounded. Original sensitive data is not automatically persisted before
redaction; keep only representations permitted by the output policy. Failed/missing checks stay in
the manifest. Store-full and permission errors result in partial or failed collection with a clear
list of retained artifacts. AC-11 and AC-13 cover path traversal, quotas and incomplete bundles.

## Comparison

**CAP-10.** evidence.compare takes two bundle/run references and comparison options. It validates
service/environment, operation schema, units, windows, sample coverage and normalization policy
before computing changes. Volatile fields such as run IDs are excluded only by named rules.

The result shows added/removed/changed observations and incompatible or missing comparisons.
Rate changes require comparable denominators/windows; missing data is never zero. A change does
not establish causation or successful remediation. Preserve both source references and the
comparison algorithm version. AC-17 covers unit mismatch, sampling differences and partial bundles.

## Saved queries

**CAP-11.** query.run resolves an installed/reviewed query ID/version, typed parameters, target,
data source and time window. Initial catalogs support PromQL/LogQL; later dialects arrive through
CAP-24. Query definitions include units, purpose, expected shape, limits and known blind spots.

Resolve relative time once per parent run. Parameters are escaped according to the actual dialect,
not substituted into arbitrary text. Unresolved dashboard macros and unsupported expressions fail
before network work. Query defaults do not select production implicitly. AC-16 covers injection,
missing parameters, template edits, unsupported dialects and excess work; AC-08 covers bounded reads.

## Guided runbooks

**CAP-12.** runbook.show renders a reviewed version; runbook.run executes its supported steps
against an explicit target. Runbooks combine explanatory text, prerequisites, bounded checks,
simple typed branches and human checkpoints. Every step names a capability and expected evidence.

Text instructions remain data until the runbook version and step are authorized. A denied step
cannot be converted to an alternate shell command. The first release is observation-only.
Resumption verifies runbook/target/input versions and avoids repeating completed steps by default;
fresh observations are explicitly new attempts. Cycles and recursive invocation are rejected.
AC-15/AC-16 cover blocked prerequisites, unsupported steps, interruption and changed definitions.

## Configuration checks

**CAP-13.** config.check validates a declared kind and file/bundle. First kinds are the existing
dashboard checker and supported alert/config validator adapters. Each result identifies validator
version, input digest, implemented rules, findings and unsupported sections.

Executing a third-party validator is code execution and requires its reviewed task environment.
Offline syntax/rule validation never claims live deployment correctness. Unsupported schemas fail
clearly; a dashboard with zero recognized panels cannot pass by omission. AC-19 verifies negative
fixtures, findings exit behavior and no mutation of the original input.

## Offline investigation

**CAP-14.** evidence.open and evidence.replay verify a bundle then run deterministic analysis with
network access disabled by the execution profile. Replay cannot execute commands embedded in
evidence or contact the original source. Live recheck is a separately requested new run.

Results preserve original observation times and add analysis time/tool version. Corrupt, oversized,
incomplete or incompatible bundles are rejected or opened in an explicitly limited inspection mode.
Replay cannot turn a historical observation into a current-state fact. AC-18 proves no-network
execution, tamper detection and correct distinction between observation and analysis.

## Reports and operational knowledge

**CAP-15.** report.create renders Markdown/JSON from selected verified bundle structure with
findings, evidence links, missing checks, timing and next questions. Optional model interpretation
is separately labeled and references the underlying facts. Reports support existing bridge/TLC
updates, handoffs and postmortem preparation.

Knowledge suggestions may propose updates to service cards, alert cards or runbooks with exact
source evidence and a human review disposition. They never mark themselves approved, merge files
or send messages. External publishing is a separately named integration/effect. AC-20 covers
contradictory evidence, redactions, unsupported claims and preservation of source labels.

## Toil measurement

**CAP-16.** toil.summary aggregates opt-in task metadata: duration, frequency, retries, failures,
manual interventions and maintenance effort. It compares recorded baseline and candidate workflows
with declared sample sizes. Raw command/query content is not needed for routine aggregate metrics.

Automation savings subtract build/maintenance/incident costs and distinguish estimates from
measurements. No employee ranking or default external telemetry. Small or biased samples carry
limitations rather than confident benefit claims. AC-21 checks units, missing data, opt-out,
retention and break-even arithmetic.

## Extensions

**CAP-17.** extension.inspect/list/install manages reviewed capability packages with manifest,
schemas, compatibility range, executable digest and provenance. Installation and enabling are
separate states. Installation must not run arbitrary post-install code or widen grants.

Built-in Rust modules and external process adapters share semantic contracts. Later distribution
can add a catalog/marketplace, signatures, revocation and update policy after trust ownership is
settled. An offline package path remains supported. Unknown required fields/protocol versions fail
closed. AC-26 tests malicious manifests and replacement; AC-29 adds a fourth unrelated capability
to measure extension cost and checks old-client compatibility.

## Jobs

**CAP-18.** job.submit/status/watch/cancel provides long-running bounded work with durable handle,
owner and execution location. Clients can reconnect and request events after a sequence number.
The result distinguishes queued, running, finalizing, terminal and recovery-required internal states.

Duplicate submission is resolved by request fingerprint and operation semantics; it cannot silently
repeat work. On restart, reconcile rather than mark all running jobs successful or restart them.
Cancellation remains best-effort until termination/reconciliation is observed. AC-22 exercises
disconnect, process crash, output loss, duplicate requests and cancellation during finalization.

## Remote execution

**CAP-19.** runner.submit sends a versioned request to a configured authenticated runner. The runner
independently checks caller, operation, target, budget, local policy and installed capability version.
It uses its own scoped credential references; local secrets are not forwarded by default.

Support on-prem Windows/Linux workers before considering additional deployment platforms. No
self-managed Kubernetes dependency is implied. Control-plane loss, expired lease or reconnect
cannot authorize replay. Results identify remote vantage, clock uncertainty and collection gaps.
AC-23 tests identity failures, authorization disagreement, partitions and target-side cancellation.

## Multiple target execution

**CAP-20.** fleet.collect resolves a bounded immutable target set before admission, then dispatches
per-target operations with concurrency and failure thresholds. The result includes succeeded,
failed, unknown and skipped targets, plus per-target evidence references.

Selector changes after planning require a new set; empty selection differs from failed discovery.
Stop scheduling when limits are hit while allowing safe completion/cancellation of admitted work.
This capability initially supports observations only. AC-30 verifies set drift, fairness, deadline
propagation, partial failure and no concealed targets.

## Scheduled checks

**CAP-21.** schedule.create/update/pause/run-now operates a versioned schedule with owner, target
set, operation version, timezone, cadence, overlap policy, missed-run policy and retention.
Initial scheduled work is read-only. Scheduling is an explicit persistent state change.

Default overlap policy is skip with a recorded reason; catch-up is disabled unless configured.
Clock changes, daylight-saving transitions, restart and credential expiry have deterministic tested
behavior. Notifications require an explicit destination and sending grant. AC-31 tests overlap,
misfires, duplicate dispatch, pause races and time-zone boundaries.

## Controlled changes

**CAP-22.** change.plan/apply/reconcile implements the contract in [security](security.md). Initial
candidate operations must be named by the owner; Grafana dashboard or temporary-silence changes
are possibilities, not granted functionality. Each operation has target-specific preconditions,
verification, concurrency and recovery rules.

Apply requires valid exact-plan approval and revalidation. Lost responses become unknown, with
target readback or supported idempotency before retry. Readback disagreement prevents success.
AC-24 and AC-25 cover expired/replayed approvals, stale plans, partial effects, unknown outcomes
and actual recovery. This phase depends on a separate accepted execution identity and workflow.

## User interface

**CAP-23.** A later local UI displays discovery, target selection, runs, evidence comparisons and
reports using the same core contracts. It adds no privileged alternate path. A read-only evidence
viewer is the first slice; operation launching and remote/team views follow separately.

A local HTTP UI binds loopback by default, validates origin/authentication and resists cross-site
requests. Reports escape untrusted text/HTML. Accessibility includes keyboard access, focus,
readable status and useful narrow-screen layouts. AC-32 verifies parity, injection resistance,
file access boundaries, accessibility and inability to launch a denied operation.

## Additional integrations

**CAP-24.** Each connector is an independent increment with its own API/version/edition research,
target/credential binding, limits, result schema and conformance tests.

| Integration family | Planned useful observations | Required discovery before implementation |
|---|---|---|
| Splunk | Bounded saved log searches and result coverage | Supported API/auth, search cost, index access, job lifecycle |
| Prometheus/Mimir and Loki | Direct approved queries where Grafana is unnecessary | Origin/tenant isolation, limits, retention and query cost |
| Wavefront/DX OpenExplore and PCF App Metrics | Existing metrics and service observations | Entitlement, supported access path and target-specific version |
| Tempo | Trace lookup and bounded trace searches | TraceQL/API contract, retention and sensitive attributes |
| PCF/TAS | Selected application/events/log observations | Existing Apps Manager/CLI access and protected output; no BOSH administration |
| GCP | Selected logging, monitoring, trace and application observations | Approved project/runtime, API permissions, quotas and credential path |
| Windows/Linux hosts | Service, disk, DNS/TCP/TLS and process observations | OS permissions, remote execution identity and safe output projection |
| PostgreSQL/SQL Server | Named operational diagnostic queries | DBA-reviewed queries, statement limits, read permissions and load impact |
| ThousandEyes/Akamai/Moogsoft | Existing tests, delivery/security observations and alert context | Supported read path, tenancy, licensing and output classification |
| Browser/visual evidence | Requested Grafana view and image capture | Dedicated read-only session, origin/org binding, screenshot handling and image inspection |

No connector is declared live because guidance exists. Integrations expose unsupported operations
honestly. Browser evidence remains separate from query/configuration evidence and never silently
saves dashboards, silences alerts or signs in. AC-33 requires a per-connector acceptance record.

## Installation and diagnostics

**CAP-25.** Product releases include platform artifacts, checksums/provenance, compatibility matrix,
dependency list, license, migration notes and rollback instructions. doctor performs local checks
by default; explicit --online performs bounded connectivity/auth-scope probes for a named target.

Installation chooses a trusted location and does not alter unrelated PATH, shell profiles, host
agent grants or credentials silently. Upgrade validates configuration/store compatibility and
preserves recoverable prior state. Uninstall offers explicit retention/export of user evidence.
AC-34 through AC-36 cover fresh installation, missing runtimes, offline operation, upgrade,
rollback, documented examples and product usefulness.
