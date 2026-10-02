# AgenticSRE Rust

A planned SRE operations product for humans and agents: common commands, Grafana observations,
named scripts, repeatable investigations, and extensible operational capabilities.

**Status:** Product planning and repository bootstrap. There is no Rust executable yet.
The owner selected this repository on 2026-10-02. SRE Workbench remains the working product name
and save the proposed CLI name; choosing this repository does not settle the remaining design decisions.

## Start here

- [Complete planning package](docs/sre-workbench/README.md): all 25 capabilities, architecture,
  contracts, security, interfaces, acceptance cases and delivery phases.
- [Product roadmap](docs/roadmap.md): the single live work queue for this product.
- [Delivery sequence](docs/sre-workbench/delivery.md): dependency-ordered work packages.
- [Decisions and risks](docs/sre-workbench/decisions-and-risks.md): resolved and outstanding choices.
- [Contributing](CONTRIBUTING.md): reproduce the planning checks before proposing changes.

The first intended implementation slice is a Rust command runner with literal arguments, explicit
working directory, text/JSON output, execution limits, cancellation and Windows/Linux tests.
Grafana, script adapters and MCP follow through the shared core. No live target or credential
configuration is included in this repository.

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
