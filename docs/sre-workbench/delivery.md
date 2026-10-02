# Delivery plan

This is the dependency-ordered implementation plan for all requested capabilities.
Execution status remains in [WORKBENCH-001](../roadmap.md#workbench-001--bootstrap-the-product).
The owner authorized plan improvements and a local PoC on 2026-10-02. The concrete first increment
is [the PoC contract](../poc.md). Routine implementation choices and fixture checks can proceed
under that authority. Paid model campaigns, publication, deployment and live writes retain their
separate boundaries. Implementation evidence and release/adoption acceptance are distinct.

## Phase outcomes

| Phase | Outcome and scope | Prerequisites | Exit evidence |
|---|---|---|---|
| P0 | Concrete local product boundary and core contracts | This complete planning package and PoC authorization | Decisions required for the local increment resolved; schemas/examples and traceability checked; live pilot choices remain with their consumer |
| P1 | Useful local command runner and installable development artifact | P0 | CAP-01, initial CAP-02, CAP-05 and CAP-25 pass local/platform contract cases |
| P2 | First complete human/agent product demonstration | Stable P1 contracts for fixture work; scoped Grafana access and selected hosts for live acceptance | CAP-03, CAP-04, CAP-06; equivalent CLI/MCP fixture workflow first, then selected live workflow, failures and install/rollback evidence |
| P3 | Repeatable service investigations | P2 adapter contracts and realistic fixture records; real context for live acceptance | CAP-07 through CAP-11 and CAP-13; capture/compare one service with complete provenance; unavailable live access does not block offline implementation |
| P4 | Operational handoff and repeatable team use | P3 | CAP-12, CAP-14, CAP-15, CAP-16; runbook resume, offline replay and report/toil cases |
| P5 | Extensibility and long work proven | Stable P2 contracts; later investigation workflows inform compatibility | CAP-17, CAP-18 and expanded CAP-02; fourth capability and job recovery experiments |
| P6 | Managed execution across hosts and time | P5, accepted runner identity/topology | CAP-19, CAP-20, CAP-21; partition, fan-out and scheduling conformance |
| P7 | Specifically authorized operational changes | P6 or equivalent protected executor; named legitimate consumer | CAP-22; exact-plan approval, reconciliation and recovery demonstrated |
| P8 | Broader product surfaces and integrations | Stable core plus each connector's prerequisites | CAP-23 and CAP-24 increments; UI and connector acceptance, packaging and support updates |

P8 is a collection of bounded increments, not a requirement to finish every vendor connector before
the core is useful. All connector families remain planned. A selected connector can be pulled
forward once its prerequisites and review are satisfied. Native API and process adapters remain
independent so one unavailable integration does not block unrelated checks.

## Work packages

| ID | Work | Inputs and deliverables | Depends on | Proposed responsible role |
|---|---|---|---|---|
| WP-01 | Review scope and architecture | Resolve slice-specific DEC-01 through DEC-05 choices; separate fixture work from live pilot authority | Planning package and owner PoC authorization | Technical lead; product owner for live-target decisions |
| WP-02 | Bootstrap product repository and toolchain | Approved repository/license, locked Rust/dependencies, CI, schema fixtures, contributor guide | WP-01 | Implementer |
| WP-03 | Implement core contracts and supervisor | Dispatcher, safe config, policy seam, result/events, process/limit/cancellation tests | WP-02 | Implementer |
| WP-04 | Deliver CLI and common command adapters | Help/JSON, command.inspect, reviewed git/rg/native forms, PowerShell task path | WP-03 | Implementer and platform tester |
| WP-05 | Package and diagnose local installs | Doctor, artifact verification, platform matrix, upgrade/rollback experiment | WP-04 | Release owner and verifier |
| WP-06 | Implement native Grafana read adapter | Version/edition profiles, parity fixtures from Python helper, local HTTP failure server | WP-03 | Integration implementer |
| WP-07 | Adapt existing scripts | Structured result seams, fixed installed bindings, runtime detection and parity evidence | WP-03 | Implementer |
| WP-08 | Implement MCP and host integration | Schemas/tool mapping, host registration instructions, tool grants and conformance | WP-04, WP-06, WP-07 | Host integrator and verifier |
| WP-09 | Run first product pilot | Human/agent equivalent workflow on named service, negative cases, usability measurements | WP-05, WP-08 | Human SRE and independent verifier |
| WP-10 | Build service investigation features | Context resolver, diagnostics, saved queries, capture, config checks and comparison | Stable WP-06/07/08 contracts for fixtures; WP-09 for live acceptance | Implementer and service owner |
| WP-11 | Build team workflows | Runbooks, offline analysis, reports, knowledge proposals, toil metrics | WP-10 | Implementer and human SRE |
| WP-12 | Prove extension and job contracts | Package inspection/install, fourth capability, durable job recovery | WP-08, stable contracts | Implementer and security reviewer |
| WP-13 | Add remote/fleet/scheduling | Authenticated runner, per-target outcomes, frozen target sets, schedule ownership/time behavior | WP-12 | Platform engineer and verifier |
| WP-14 | Add a named controlled change | Bound plan/approval, target preconditions, ledger, readback, uncertainty and recovery | WP-13 or accepted equivalent | Change owner and security reviewer |
| WP-15 | Add UI and integration increments | Connector-specific design/research/tests; UI evidence viewer then permitted execution | Relevant core phases | Integration/UI owner |
| WP-16 | Productize each release | Docs, license/provenance, support matrix, migration/rollback and support handoff | Every releasing package | Release owner |

Roles are responsibilities, not assignments to named humans. Under the PoC authorization the
coordinating agent assigns bounded implementation and verification work. A builder cannot supply the only acceptance
judgment for a security-sensitive boundary. Review helpers return evidence to the owning caller;
their completion does not complete the parent product milestone.

## Critical path and parallel work

~~~text
Scope/contracts -> toolchain/core -> CLI/platform execution -> packaging
                             |-> Grafana adapter ---------|
                             |-> script adapters ---------|-> MCP/host checks -> pilot
Adapter contracts -> context/diagnostics/evidence fixtures -> runbooks/offline/reports/toil
Selected live pilot -> live acceptance of relevant investigation features
Stable core -> extensions/jobs -> remote/fleet/schedules -> controlled changes
Stable interfaces + data model -> UI and additional connector increments
~~~

Grafana, scripts and presentation can run independently only after common schemas and ownership
are stable. Serialize changes to shared contracts, policy, run-state storage and migrations.
Each implementation slice uses an isolated branch/worktree when concurrent work would overlap.
No parallel agent or model campaign is implied by this plan; choose resources explicitly.

## Indicative effort and staffing

These ranges are planning hypotheses for one experienced Rust implementer with part-time SRE,
security/review and platform verification support. They are engineering weeks, not calendar
commitments; access, feedback and host test availability may dominate elapsed time.

| Scope | Initial range | Main uncertainty |
|---|---|---|
| P0 accepted design and prototype boundary | 1 to 2 | Owner decisions and selected host/access path |
| P1 local runner and development packaging | 2 to 4 | Windows process tree, quoting and executable trust |
| P2 Grafana/scripts/MCP plus accepted pilot | 3 to 6 | Authentication, Grafana API/model compatibility, actual agent hosts |
| P3 investigation features | 3 to 5 | Real context quality and comparison semantics |
| P4 team workflows | 3 to 5 | Runbook branching, evidence privacy and reporting expectations |
| P5 extension/job maturity | 3 to 5 | Recovery and cross-language protocol compatibility |
| P6 remote/fleet/scheduling | 4 to 8 | Identity, network boundaries, leases and deployment ownership |
| P7 first controlled write | 3 to 6 | Target idempotency, concurrency and recovery |
| P8 initial UI or one connector | 2 to 5 each | Feature/edition/API scope and user research |

Re-estimate after WP-09 using measured delivery and maintenance cost. Do not add the connector
range to produce a false total for all unknown future integrations. No paid-model budget is
assumed; choose trial count, spend cap and stop conditions before any live model evaluation.

## Definition of done for an implementation slice

1. Selected requirement and acceptance IDs are satisfied or explicitly left open.
2. Public schemas, examples, CLI help, MCP metadata and documentation agree.
3. Focused tests include failure/denial and relevant platform cases.
4. Security/authority claims are supported by the actual execution boundary.
5. Changes are reviewed at the exact candidate revision with remaining risks recorded.
6. Install, upgrade and rollback are demonstrated for any shipping artifact.
7. For a release or live adoption, human acceptance is recorded for that candidate and target use.
   Local implementation and fixture verification proceed under existing PoC authority, without
   repeatedly asking a human to accept routine development slices.

A feature can be implemented but unavailable on an unverified host. Publish that distinction in
the support matrix. Evidence from local fixtures cannot close live authentication or target safety.

## Product repository transition

The owner selected this repository on 2026-10-02. The plan was imported from the committed Save
Toolkit candidate with provenance and rebased source links; see the [import record](../plan-import.md).
Product execution status is now maintained in this repository's [roadmap](../roadmap.md).
The source specification was still proposed at import, so repository selection does not accept
all architecture decisions or establish runtime readiness.

Save Toolkit receives narrowly scoped integration changes only after the product interface is
stable: skill usage, exact installed executable/tool names, supported host grants and tests.
Canonical edits regenerate their host adapters. Do not replace the Python guard as an incidental
part of installing the product; any guard migration has its own compatibility and host acceptance.

## Scope control

Every new idea gets a user task, operation boundary, inputs/outputs, effects, dependencies,
acceptance case and phase. Unknown features can extend the architecture, but must not inherit
credentials or execution rights by being discovered. Reopen a contract when concrete evidence
shows it cannot express the required behavior. Keep compatibility decisions and alternatives in
the decision register, and reject abstractions that have no demonstrated consumer.
