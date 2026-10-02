# Product roadmap

**Status:** Live product work queue, established 2026-10-02. The owner selected
latent-sre/agenticsre-rust as the working repository. Scope and dependencies live in the
[planning package](sre-workbench/README.md); this file owns current execution status.

## WORKBENCH-001 — bootstrap the product

**Status:** Active planning and repository handoff; Rust implementation has not started.
**Owner:** Human product owner; implementation and verification owners selected per slice.
**Outcome:** Establish this repository as the product home, preserve the complete plan and its
provenance, and make the first command-runner slice concrete and reviewable.
**Next action:** Review the imported specification and initial decisions, then select WP-02 through
the first bounded part of WP-04: a working process.exec path with text/JSON results, timeouts,
bounded output, cancellation and platform tests.
**Evidence:** [Import record](plan-import.md), [capability specifications](sre-workbench/capabilities.md),
[delivery plan](sre-workbench/delivery.md), and the local specification checks.
**SRE task:** Run the same useful operational checks directly or through an agent, with explicit
targets, dependable results and extensible capabilities.

All 25 capabilities remain in the product scope. Their P0 through P8 sequence is described in
the delivery plan. Individual implementation items are added here when selected; planning an
integration does not imply it is installed, authorized, connected or operational.
