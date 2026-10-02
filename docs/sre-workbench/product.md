# Product requirements

SRE Workbench gives a human responder and their agents one consistent way to perform operational
checks and carry the resulting evidence into a decision. Its value is reduced setup, repeated
lookups, command construction, and ambiguous results. A language migration is not the success metric.
All product behavior described here is proposed; the [baseline](sources.md) distinguishes code
that already exists from the product that must be built.

## Users and workflows

| User | Need | Successful workflow |
|---|---|---|
| Human SRE | Inspect a service quickly and understand limits | Select the target, run checks, compare evidence, prepare a bridge update |
| Investigating agent | Discover and call permitted operations reliably | Resolve inputs, perform bounded reads, retain failures and source references |
| Tool author | Add a command, script or API capability | Implement one adapter and its contract tests without changing unrelated integrations |
| Team maintainer | Distribute and operate a dependable tool | Install a verified version, configure targets, diagnose failures, roll back |
| Change owner | Eventually authorize a specific operational change | Review a bound plan, execute under the permitted identity, reconcile and verify |

The human retains incident coordination, severity decisions, recovery declarations and release
ownership. Existing bridge/TLC processes remain the coordination channel. Reports are prepared
for that channel; automatic sending or paging is a separate integration and explicit operation.

## Required outcomes

| ID | Requirement | Primary specifications |
|---|---|---|
| REQ-01 | Run familiar installed commands with explicit args, directory and execution limits | CAP-01, CAP-02 |
| REQ-02 | Read Grafana dashboards and bounded metrics/log queries, then expand to supported observations | CAP-03 |
| REQ-03 | Run existing reviewed Python, PowerShell, Bash or compiled tasks with clear dependencies | CAP-04 |
| REQ-04 | Expose equivalent operations to humans and agents with discoverable input/output contracts | CAP-05, CAP-06 |
| REQ-05 | Resolve service, environment, ownership and resource context without inventing missing mappings | CAP-07 |
| REQ-06 | Run reusable diagnostic packs with per-check evidence | CAP-08 |
| REQ-07 | Collect bounded evidence and compare observations across runs | CAP-09, CAP-10 |
| REQ-08 | Reuse parameterized queries and guided runbooks | CAP-11, CAP-12 |
| REQ-09 | Check configuration and distinguish validation from live health | CAP-13 |
| REQ-10 | Investigate offline, prepare reports, and propose evidence-bound knowledge updates | CAP-14, CAP-15 |
| REQ-11 | Measure toil and automation maintenance cost without claiming unmeasured savings | CAP-16 |
| REQ-12 | Add unknown future capabilities through versioned extension contracts | CAP-17 |
| REQ-13 | Support long work, progress, cancellation and later remote/fleet execution | CAP-18, CAP-19, CAP-20 |
| REQ-14 | Support scheduled checks with explicit ownership and overlap/recovery behavior | CAP-21 |
| REQ-15 | Plan a future path for authorized changes with reconciliation and recovery | CAP-22 |
| REQ-16 | Preserve a path to a UI and more integrations without coupling the core to one host/vendor | CAP-23, CAP-24 |
| REQ-17 | Ship a diagnosable, upgradeable product with complete user and developer documentation | CAP-25 |

## Nonfunctional requirements

| ID | Required property | Proposed measurable acceptance |
|---|---|---|
| NFR-01 | Predictable behavior | Same normalized request against the same fixtures has equivalent CLI/MCP results; volatile fields explicitly excluded |
| NFR-02 | Bounded execution | Deadlines, output limits, concurrency and fan-out limits enforced; no unbounded default jobs |
| NFR-03 | Honest results | Separate process/operation outcome, target assessment, evidence coverage and uncertainty |
| NFR-04 | Authority preservation | No input, manifest, role string or plugin metadata can increase its effective grant |
| NFR-05 | Credential handling | No fixture secrets in any tested stdout, stderr, logs, artifacts or reports; limits of redaction documented |
| NFR-06 | Portability | Exact supported OS/architecture/host combinations pass published acceptance; unsupported combinations are explicit |
| NFR-07 | Compatibility | Versioned schemas and operations; unsupported major versions rejected before dispatch |
| NFR-08 | Recoverability | Interrupted effects remain unknown until reconciled; rollback verified for the release and any supported change |
| NFR-09 | Maintainability | Fourth different capability added without rewriting unrelated core or adapters |
| NFR-10 | Performance | Benchmark startup and wrapper overhead separately from tool/network latency; no speed claim without a repeatable baseline |
| NFR-11 | Operability | Doctor output identifies missing dependencies/configuration without credentials; diagnostic errors name a next step |
| NFR-12 | Privacy | No external product telemetry by default; explicit retention, local artifact ACLs and export controls |

Proposed engineering budgets are in [contracts](contracts.md). They are tunable design starting
points and do not claim observed performance. A phase-zero benchmark sets host-specific regression
thresholds before using speed as a release gate.

## Product boundaries

The product executes declared operations and preserves evidence. An agent may interpret results
and choose a next permitted check. The core does not need an LLM to run commands, query Grafana,
calculate a budget, compare snapshots, or render a deterministic report. AI-assisted analysis is
an optional consumer and must cite observations rather than changing them.

General process execution does not prove a program is read-only, safe, or trustworthy. Effect
classification depends on executable, args, environment, files, target and identity. Local execution
under one OS account cannot by itself isolate that account's credentials from other tools.

This plan includes remote execution, scheduling, write operations, UI and extension distribution.
Their later phases are a sequencing choice. It excludes automatic incident command, autonomous
policy promotion, unapproved self-modification, and unbounded agent execution. Additional product
ideas enter the capability contract and review process described in the architecture.

## Adoption and success measurement

Start with the current team's Windows and Linux workflows, while preserving a tested macOS route.
The exact initial support matrix is DEC-03. Internal use validates product assumptions before any
external distribution promise. Open-source/commercial distribution and licensing are DEC-15.

Measure a matched set of representative tasks before and after the pilot:

1. Time to a usable observation, including configuration and command correction.
2. Manual steps and repeated target/credential setup.
3. Failed invocations caused by quoting, wrong arguments or missing tools.
4. Agent requests that need human repair and unsupported target guesses.
5. Evidence completeness, correct failure reporting and ability to reproduce a conclusion.
6. Installation/upgrade success and ongoing maintenance effort.

Use at least five distinct representative workflows for the pilot; sample count and any paid model
budget are chosen by the owner before the campaign. Record negative results. A proposed target is
a 25 percent reduction in median manual steps or time on at least three workflows, with no loss
of correctness or authority. This is a hypothesis to evaluate, not a promised benefit or automatic
acceptance rule. Include maintenance cost in the toil calculation.

## First product demonstration

For one explicitly configured trading service, a human and an agent each retrieve the same
dashboard, execute a bounded query, run a local common command and one offline script, and produce
equivalent evidence. Exercise a failure as well as success. Show target identity, window, errors,
truncation and missing coverage. No successful command alone declares service recovery.

No actual service, Grafana origin, credential, host grant or release target is inferred from these
examples. The owner selects them through DEC-02 and DEC-04 before live acceptance.
