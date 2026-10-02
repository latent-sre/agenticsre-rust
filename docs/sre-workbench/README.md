# SRE Workbench planning package

**Status:** Proposed product specification, revision 0.1, 2026-10-02. The owner requested a full
plan including every capability discussed. This package defines that scope; it does not claim
an implementation, live integration, approved release, or accepted architecture.

The owner selected [latent-sre/agenticsre-rust](https://github.com/latent-sre/agenticsre-rust)
as the product repository. This is the continuing planning copy; the
[import record](../plan-import.md) identifies the original Save Toolkit revision and adaptations.

SRE Workbench is the working name for a shared operations product. Humans use a CLI; agents use
the same operations through structured tool interfaces. It runs common commands, reads Grafana,
executes named scripts, and grows into repeatable investigation workflows. The proposed executable
name is `save`; availability, branding, and package names remain a phase-zero decision.

The intended first proof is one real service investigated by a human and an agent using equivalent
operations and evidence. The larger product includes every feature in the capability catalog,
with delivery sequenced by dependencies rather than deleting later features from scope.

## Read the package

| Document | What it establishes |
|---|---|
| [Product requirements](product.md) | Users, outcomes, requirements, scope, measurable success |
| [Architecture](architecture.md) | Components, boundaries, extension strategy, storage and execution |
| [Shared contracts](contracts.md) | Requests, results, events, status, limits, compatibility and schemas |
| [CLI and agent interfaces](interfaces.md) | Command grammar, MCP tools, discovery and worked examples |
| [Capability specifications](capabilities.md) | All 25 capabilities, inputs, behavior, failures and acceptance |
| [Security and authority](security.md) | Threat model, permissions, credentials, extensions and live effects |
| [Jobs remote execution and changes](advanced-execution.md) | Durable jobs, remote leases, schedules, fan-out and change recovery |
| [Delivery plan](delivery.md) | Phases, dependencies, work packages, ownership and completion gates |
| [Verification plan](verification.md) | Acceptance cases, traceability, host matrix and failure testing |
| [Operating and developer guide](operations.md) | Proposed installation, configuration, troubleshooting and contribution |
| [Decisions and risks](decisions-and-risks.md) | Recommendations, alternatives, unresolved choices and risks |
| [Sources and current baseline](sources.md) | Repository evidence, external sources, applicability and refresh triggers |

Machine-readable draft contracts live in [schemas](schemas/README.md); valid and invalid fixture
examples live in [examples](examples/README.md). They specify intended shapes, not a released API.

## How to interpret this specification

- **Required** identifies an owner-requested outcome or a necessary property of that outcome.
- **Proposed** identifies a concrete design choice that can be changed during review.
- **Open decision** identifies a choice with an owner, due phase, and consequence.
- `[verified]` means a bounded current observation; `[sourced]` identifies documentary support;
  `[unverified]` marks an untested runtime claim or design hypothesis. Future behavior throughout
  this package is proposed and unverified unless explicitly identified as current-source evidence.
- MUST, SHOULD, and MAY describe the candidate contract. They do not expand existing agent grants
  or authorize live actions. Acceptance of the plan and acceptance of a shipping revision differ.

Stable references use `REQ-`, `NFR-`, `CAP-`, `AC-`, `WP-`, `DEC-`, and `RISK-` identifiers.
Capability IDs and acceptance IDs are cross-referenced so features cannot disappear between the
vision and implementation plan. Requirements live in this package; execution status lives only in
[WORKBENCH-001](../roadmap.md#workbench-001--bootstrap-the-product).
The work-package tables are a proposed delivery decomposition, not a second active backlog.

## Agreed direction and proposed implementation

The owner wants a real SRE tool that humans and agents can use, including ordinary commands,
Grafana checks, other scripts, all investigation features in the catalog, and room for unknown
future capabilities. Rust is the proposed core implementation. This repository owns product
planning and future releases; Save Toolkit remains a consumer with host-specific guidance and grants.

The plan does not require rewriting the repository's Python validators, evals, atlas, or generators.
Native Grafana reads are proposed for the product; optional script adapters retain their own
interpreter requirements. Existing guard/hook behavior remains the compatibility baseline until
a separately accepted migration replaces it.

## Planning completeness and readiness

The package contains design contracts, capability specifications, dependency-ordered plans,
acceptance cases, example data, and documentation requirements for every delivery phase.
It is ready for a design review when the checks in the verification plan pass. It is not a runtime
readiness verdict. Real service selection, host acceptance, credential isolation, deployment
topology, and product release evidence remain explicit work.

The [decision register](decisions-and-risks.md) names decisions that block only the phase needing
them. Later UI, scheduling, remote-execution, and controlled-change decisions must not prevent a
bounded local prototype after the initial contracts and ownership are accepted.
