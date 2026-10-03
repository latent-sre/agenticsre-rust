# AgenticSRE Rust

A planned SRE operations product for humans and agents: common commands, Grafana observations,
named scripts, repeatable investigations, and extensible operational capabilities.

**Status:** Linux command-runner PoC, two fixed offline tasks and a native Grafana fixture adapter
built, independently reviewed/verified and tested on Cally.
The local browser interface is built and independently reviewed/verified for its agreed Linux PoC
scope; [its acceptance record](docs/ui-verification.md) retains exact source identities and limits.
The bounded stdio MCP adapter is also [independently verified](docs/mcp-verification.md) for explicit
launcher grants and controlled Linux fixtures.
The [Linux development bundle](docs/packaging-verification.md) includes offline context resolution
and is verified for versioned installation, upgrade and rollback in the tested Cally environment.
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
target/debug/save ui serve --allow task.run
```

Rust 1.98.0 is pinned in `rust-toolchain.toml`. See [development](docs/development.md) for sandboxed
tests and Cally's offline build, and [the PoC contract](docs/poc.md) for exact limits and scope.
The [PoC](docs/poc-verification.md), [dashboard](docs/dashboard-verification.md),
[error-budget](docs/error-budget-verification.md) and [Grafana acceptance](docs/grafana-verification.md)
records tie passed checks and
limitations to frozen source.
The [local browser interface](docs/development.md#local-browser-interface) runs checks and shows
results/history using the same core. It has no AI chat or persistent history; its compiled assets
are embedded in `save`, so running it requires no separate Node server.

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
