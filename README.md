# AgenticSRE Rust

A planned SRE operations product for humans and agents: common commands, Grafana observations,
named scripts, repeatable investigations, and extensible operational capabilities.

**Status:** Linux command-runner PoC, two fixed offline tasks and a native Grafana fixture adapter
built, independently reviewed/verified and tested on Cally.
The local browser interface is built and independently reviewed/verified for its agreed Linux PoC
scope; [its acceptance record](docs/ui-verification.md) retains exact source identities and limits.
The bounded stdio MCP adapter is also [independently verified](docs/mcp-verification.md) for explicit
launcher grants and controlled Linux fixtures.
The [offline MCP investigation extension](docs/investigation-mcp.md) adds explicit context,
saved-evidence, comparison and report grants using the same core as the browser and CLI.
The [Linux development bundle](docs/packaging-verification.md) includes offline context resolution
and is verified for versioned installation, upgrade and rollback in the tested Cally environment.
The current source also includes [verified installed saved queries](docs/saved-queries-verification.md)
for the Linux core/CLI fixture scope; the retained bundle predates these commands.
The source exposes those installed queries through [browser grants](docs/query-ui.md) and
[MCP grants](docs/query-mcp-verification.md). Explicit [MCP collection grants](docs/evidence-mcp.md)
retain the fixed service-query pack in an existing private store; offline investigation grants
remain separate.
The `save` CLI executes literal arguments
with bounded output, deadlines and cancellation, or inspects commands without running them.
It provides text/JSON receipts, structured requests, discovery and offline diagnostics. The
`dashboard-hygiene` task checks local Grafana models using its embedded checker and optional
system Python >=3.11. The `error-budget` task calculates time/request budgets and bounded-window
burn policy from supplied measurements. Native `grafana dashboard get` and `grafana query`
provide scoped HTTPS observations with bounded, redacted results. It is an operator-local
development tool. A restricted Linux Git/ripgrep profile is implemented with remaining acceptance
limits; Windows execution and live Grafana-version acceptance remain open. SRE Workbench and
`save` are working names.

```bash
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo build --locked -p workbench-cli
target/debug/save --json exec --cwd . -- /usr/bin/printf '%s\n' 'hello workbench'
target/debug/save task run dashboard-hygiene --input tests/fixtures/dashboard-hygiene/clean-input.json
target/debug/save task run error-budget --input tests/fixtures/error-budget/time-input.json
target/debug/save --json queries list
target/debug/save ui serve --allow task.run
target/debug/save ui serve --allow queries.list --allow queries.describe
```

Rust 1.99.1 is pinned in `rust-toolchain.toml`. See [development](docs/development.md) for sandboxed
tests and Cally's offline build, and [the PoC contract](docs/poc.md) for exact limits and scope.
The [PoC](docs/poc-verification.md), [dashboard](docs/dashboard-verification.md),
[error-budget](docs/error-budget-verification.md) and [Grafana acceptance](docs/grafana-verification.md)
records tie passed checks and
limitations to frozen source.
The [local browser interface](docs/development.md#local-browser-interface) runs checks and shows
results/history using the same core. It has no AI chat or persistent history; its compiled assets
are embedded in `save`, so running it requires no separate Node server.
The [offline investigation extension](docs/investigation-ui.md) adds independently granted service
context, saved evidence listing/inspection/comparison and report previews. It uses explicit existing
source/store bindings and the same receipts/history; see the [launch example](docs/development.md#local-browser-interface).

## Start here

- [Complete planning package](docs/sre-workbench/README.md): all 25 capabilities, architecture,
  contracts, security, interfaces, acceptance cases and delivery phases.
- [Product roadmap](docs/roadmap.md): the single live work queue for this product.
- [Delivery sequence](docs/sre-workbench/delivery.md): dependency-ordered work packages.
- [Decisions and risks](docs/sre-workbench/decisions-and-risks.md): resolved and outstanding choices.
- [Contributing](CONTRIBUTING.md): reproduce the planning checks before proposing changes.

[Native Grafana observations](docs/grafana.md) use explicit operator configuration and synthetic
fixtures for current acceptance. [Restricted commands](docs/read-profile.md) and the
[GUI](docs/ui.md) and [stdio MCP adapter](docs/mcp.md) use the shared core.
The [installed saved queries](docs/saved-queries.md) add versioned PromQL/LogQL definitions,
literal job parameters, optional definition-digest pinning and one resolved time window to the
core/CLI Grafana workflow.
The [evidence collection increment](docs/evidence.md) builds on those queries with fresh context
bindings, sanitized receipts and a private versioned local store, with
[Linux fixture acceptance](docs/evidence-collection-verification.md). See
[collection commands](docs/development.md#evidence-collection-and-local-store).
Offline `evidence compare` checks two verified bundles and reports typed metric/log differences
with explicit normalization, incompatibilities and missing observations; see the
[comparison contract](docs/evidence.md#comparison-interface-and-projections) and
[Linux fixture acceptance](docs/evidence-comparison-verification.md).
[Guided runbooks](docs/runbooks.md) add an installed evidence-review workflow with typed
checkpoints, durable revision-bound resume and explicit fresh attempts, with
[Linux fixture acceptance](docs/runbooks-verification.md).

[Offline inspection and replay](docs/offline-evidence.md) add `evidence open` and `evidence replay`
for verified historical receipts and deterministic metric/log projections, with a Linux x86-64
kernel profile disabling networking and command execution. See the
[Linux fixture acceptance](docs/offline-evidence-verification.md).

[Evidence reports](docs/reports.md) render Markdown/JSON handoffs from verified historical bundles,
with explicit disagreements, missing checks and pending evidence-bound knowledge proposals.
See the [Linux fixture acceptance](docs/reports-verification.md).

[Opt-in toil metadata](docs/toil.md) supports private workflow samples, actual task capture,
explicit human-effort annotations and cost-aware baseline/candidate summaries. See the
[Linux fixture acceptance](docs/toil-verification.md).

[Declared configuration checks](docs/config-check.md) provide installed validator identity, actual
input digests, supported rules and explicit unsupported scopes. See the
[Linux dashboard acceptance](docs/config-check-verification.md).
The [Prometheus rule adapter](docs/alert-rules.md) uses a pinned native validator and handles
lint, empty inputs and incomplete YAML coverage. See the
[Linux amd64 acceptance](docs/alert-rules-verification.md).
[Extension package management](docs/extensions.md) supports inspection, pinned installation and
verified inventories with execution disabled. See the [Linux acceptance](docs/extensions-verification.md).
The [external protocol and capacity-projection experiment](docs/extension-runtime-verification.md)
are accepted for Linux x86-64 with separate exact-package operator policy.
[Durable local jobs](docs/jobs.md) add explicit submission and workers, reconnectable events,
cancellation and crash recovery without automatic replay. Native commands, fixed tasks and pinned
extensions have [retained Linux acceptance](docs/jobs-verification.md).
The [authenticated remote runner experiment](docs/runner.md) adds explicit certificate profiles,
principal-owned jobs and target-side grants, with [retained Linux fixture acceptance](docs/runner-verification.md).
[Bounded multiple-target collection](docs/fleet.md) freezes explicit selection and child identities,
retains partial/unknown outcomes, and resumes without resubmission. Its
[Linux fixture acceptance](docs/fleet-verification.md) includes real runner processes and crash recovery.
[Persistent observational schedules](docs/schedules.md) add owned generations, pinned timezone rules,
explicit workers and manual-request deduplication, with [retained acceptance](docs/schedules-verification.md).
See [development installation and rollback](docs/development.md#development-artifact-and-rollback).
Offline [service-context fixtures](docs/context.md) are implemented through the core/CLI and
[independently verified](docs/context-verification.md). The current development archive includes
this CLI feature; its installed mission and retained rollback baseline have separate verified identities.
All 25 capabilities remain in scope; [adapter notes](docs/adapter-notes.md) preserve the verified upstream baseline. No live
target or credential configuration is included in this repository.

## Validate the specifications

With Python 3.14 selected, create a local virtual environment and install the pinned documentation
dependencies from requirements-docs.txt. On Windows:

~~~powershell
python -m venv .venv
.venv/Scripts/python.exe -m pip install --require-hashes -r requirements-docs.txt
.venv/Scripts/python.exe -B docs/sre-workbench/verify_specs.py
~~~

On Linux/macOS, use .venv/bin/python. These checks validate schemas, example data, links and ID
coverage; they do not run product operations or establish live readiness. Rust is not required
to review or validate the planning package.

## Planning provenance and license

Imported from [Save Toolkit PR 308](https://github.com/latent-sre/save-toolkit/pull/308) at
[commit 7d900fb9](https://github.com/latent-sre/save-toolkit/commit/7d900fb999abee3b3712e1da881dcc7e80b1f6e7).
The source PR was open when imported; its specification remains proposed rather than an accepted
runtime design. See the [import record](docs/plan-import.md) for the changes made during transfer.

This repository retains its existing [MIT license](LICENSE). The imported material's original
copyright and MIT notice are retained in [third-party/save-toolkit-LICENSE.txt](third-party/save-toolkit-LICENSE.txt).

Experimental approved rule-file changes use separate authenticated requester/approver identities,
exact plan digests and verified file readback. See [the change contract](docs/changes.md) for the five
`change` commands, explicit startup grants, expiry and recovery behavior. Current acceptance and
remaining limits are recorded in [the roadmap](docs/roadmap.md#workbench-026--approved-local-configuration-changes).
