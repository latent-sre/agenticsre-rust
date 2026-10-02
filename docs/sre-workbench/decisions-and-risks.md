# Decisions and risks

The owner accepted the product direction and all discussed capabilities, selected this repository,
and authorized plan improvements and a PoC on 2026-10-02. Local choices below are implementation
decisions under that authority. Broader product/live choices remain proposals. Decision owners are
responsibilities, not claims that a named person approved a release.

## Decision register

| ID | Choice and recommendation | Alternatives and tradeoff | Owner and due phase |
|---|---|---|---|
| DEC-01 | PoC selected: Rust 1.98.0, edition 2024, shared core plus CLI, locked dependencies | Reuses the installed compiler; no speed or security claim follows from language choice. Review dependency licenses and actual execution behavior | Technical lead, local choice resolved |
| DEC-02 | Repository selected: latent-sre/agenticsre-rust, owner 2026-10-02; independent product release | SRE Workbench and save remain working names; branding/package collision checks remain open before distribution | Product owner; repository choice resolved, naming does not block local builds |
| DEC-03 | Prove Linux x86-64 PoC first; retain Windows x86-64 for P1 and macOS arm64 after | Advertise only exercised OS/ABI combinations. Other execution returns unsupported until native supervisor tests pass; compilation is insufficient | Technical lead for PoC; platform owner for expanded support |
| DEC-04 | One real trading service, one Grafana origin/org and a protected read identity for live pilot | Local fixture acceptance precedes live selection and requires no credentials; choose provider and trusted host before live work | Human SRE and access owner, P2 live pilot |
| DEC-05 | Native Grafana fixture adapter after core; optional reviewed script adapters in parallel | Preserve helper semantics through independent/parity tests; no live API capability claimed from fixtures | Technical lead, selected direction for P2 implementation |
| DEC-06 | CLI and MCP share one core; local stdio first | CLI-only leaves no-terminal agents unserved; remote HTTP immediately adds identity/server operations | Host integrator, P2 |
| DEC-07 | Direct argv default; named shell tasks first | General shell source is flexible but broader and harder to constrain. Later human shell capability stays explicit | Security and operator owners, P1/P5 |
| DEC-08 | PoC supports record=never only; rejects other modes before dispatch | Invocation IDs are correlation, not durable records. Choose SQLite versus files with the first evidence consumer, retention and migration requirements | Technical lead for PoC; data/privacy owner before persistence |
| DEC-09 | Read existing service records with provenance | A new authoritative inventory duplicates ownership; actual record adapter/schema depends on selected team data | Service owner, P3 |
| DEC-10 | Native modules plus external process extensions | Native dynamic Rust plugins lack a stable ABI; WASM/container isolation may suit future untrusted code but adds runtime/limits work | Technical/security leads, P5 |
| DEC-11 | Durable jobs before remote execution and scheduling | Immediate distributed orchestration expands scope; decide runner transport, deployment, identity, leases and stores | Platform owner, P6 |
| DEC-12 | Evolve contracts only from concrete consumers | A generic workflow/programming language early increases attack and maintenance surface; bounded typed steps cover first use | Technical lead, each extension phase |
| DEC-13 | First live write is one named operation under separate authority | Keep all operations observational until a legitimate consumer, exact approval and recovery are demonstrated | Change owner, P7 |
| DEC-14 | Begin UI with local evidence viewer | Hosted multi-user UI requires identity, tenancy and service ownership; UI must reuse core permissions/results | Product/security owners, P8 |
| DEC-15 | Existing repository MIT license retained; distribution, support and positioning remain open | Imported copyright notice preserved; dependency licenses and external support/distribution need review | Product/release owner, before distribution |

Unresolved decisions block only work that actually depends on them. Naming, live credentials and
later platform acceptance do not block the authorized local PoC. Host/API versions and exact library pins are recorded
in implementation evidence when selected; documents do not assume today's latest will be correct
at build time.

## Risk register

| ID | Risk and consequence | Mitigation and evidence | Owner |
|---|---|---|---|
| RISK-01 | Wrapper adds complexity without daily benefit | Matched before/after pilot and maintenance accounting, AC-36 | Product owner |
| RISK-02 | Common command runner becomes an unintended bypass | Operation/args/config-aware grants, no automatic shell fallback, AC-02/06 | Security reviewer |
| RISK-03 | Same-account processes undermine credential isolation claims | Explicit local limitation; separately controlled runner where needed, AC-23 | Platform owner |
| RISK-04 | Grafana API/model/edition changes break semantics | Version profiles, migration research, target fixtures and live canary, AC-07/08/33 | Integration owner |
| RISK-05 | Windows quoting/process-tree behavior differs from POSIX | Real host tests, job/tree containment evidence, AC-03/05 | Platform tester |
| RISK-06 | Output or evidence exposes secrets/private telemetry | Minimal environment, bounded controlled outputs, classification/export review, AC-09/13 | Privacy/security owner |
| RISK-07 | Evidence comparison creates false conclusions | Units/coverage/identity validation, explicit incompatibility, AC-17 | SRE/domain reviewer |
| RISK-08 | Extension metadata creates authority or code execution | Reviewed immutable packages and host grants, AC-26 | Extension owner |
| RISK-09 | Crash after a write causes duplicate effects | Unknown outcome plus reconciliation/idempotency, AC-24/25 | Change owner |
| RISK-10 | Scope expands into a platform before a useful workflow exists | Complete staged scope, first product demonstration, phase gates | Product owner |
| RISK-11 | Rust expertise/dependency footprint increases maintenance | Small core, locked reviewed dependencies, documentation and ownership | Technical lead |
| RISK-12 | Agent host changes invalidate tool grants | Exact-host conformance and refresh triggers, AC-27 | Host integrator |
| RISK-13 | Local store fills, leaks or cannot downgrade | Quotas/ACLs/retention and reversible migration tests, AC-13/35 | Storage owner |
| RISK-14 | Remote scheduler replays or overlaps work | Frozen target sets, durable state, leases and injected-clock tests, AC-22/30/31 | Platform owner |
| RISK-15 | Documentation drifts from contracts | Schema examples, reference generation and release example checks, AC-35 | Documentation owner |
| RISK-16 | Product/backlog split loses decisions or duplicates work | Explicit repository handoff and single authoritative live queue | Product owner |

## Scope and authority decisions already established

The discussion requires human and agent use, common commands, Grafana, scripts, investigation
features, extensibility and future product growth. It requests full documentation and planning.
It does not select credentials, authorize production operations, install Rust, create a remote
service, publish a product, merge a branch or run a paid model campaign.

Save Toolkit's [EFFECT-001](https://github.com/latent-sre/save-toolkit/blob/7d900fb999abee3b3712e1da881dcc7e80b1f6e7/docs/fleet-roadmap.md#effect-001--effect-bound-execution-broker)
explicitly waits for a legitimate controlled-effect consumer. The future change capability is
designed here without reopening that runtime boundary. Any accepted decision replacing this
constraint must name the consumer, authority and actual execution identity.

## Review questions

The design review should settle the first consumer/service, initial platform support, credential
boundary, whether Rust's distribution benefits justify its maintenance, and who owns ongoing support.
It should inspect command execution and output/privacy failures as carefully as happy-path Grafana
reads. It should also identify which capability proves the extension contract without prematurely
requiring a marketplace or distributed control plane.
