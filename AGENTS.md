# AgenticSRE Rust development

Build the proof of concept before expanding the product. The owner authorized plan improvements,
PoC implementation and use of Cally on 2026-10-02, with minimal human intervention. Make reversible
engineering decisions within that scope. Keep publishing, production access and live changes
separate from local development. Do not commit, push, register integrations or change lab services
without existing authority for that action.

Read [the PoC contract](docs/poc.md), [the dashboard task contract](docs/tasks.md),
[the error-budget contract](docs/error-budget.md) and
[the roadmap](docs/roadmap.md). The roadmap is the sole live
work record and is owned by the coordinating agent. Builders report evidence to that agent; do not
create competing progress boards. Preserve the imported specification and license provenance.
For WORKBENCH-003, also read [the native Grafana fixture contract](docs/grafana.md) before editing.
For WORKBENCH-004, read [the restricted Linux command contract](docs/read-profile.md); its Cally
test boundary is distinct from the local sandbox that refuses nested namespace creation.

## Development environment

- Repository: `latent-sre/agenticsre-rust`; local root `/home/hawkfire/rusty/agenticsre-rust`.
- Rust: pin the available 1.98.0 toolchain, edition 2024; commit dependencies to `Cargo.lock` when
  commit authority is given. Cargo workspace: `crates/workbench-core`, `crates/workbench-cli`,
  `crates/workbench-ui`. Read [the GUI contract](docs/ui.md) for browser interface changes.
- Run: `cargo run --locked -p workbench-cli -- --json exec --cwd . -- /usr/bin/printf '%s\n' 'hello workbench'`.
- Checks: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`,
  `cargo test --workspace --all-features --locked`; executable acceptance uses the sandbox below.
- Specification checks: Python 3.14 in `.venv`; `uv pip install --python .venv/bin/python
  --require-hashes -r requirements-docs.txt`, then `.venv/bin/python -B docs/sre-workbench/verify_specs.py`.
- Command-runner operations bind no ports and need no credentials. The GUI binds loopback only;
  GUI tests run in the declared private network sandbox. Test inputs use synthetic values only.
- Grafana acceptance binds ephemeral loopback HTTPS listeners only inside isolated test namespaces,
  with generated test certificates and synthetic authentication. No host listener or live target
  is required. See `tools/verify-grafana.py` and the Grafana contract for the public fixture mission.

## Mission and verification

The mission is to run a familiar installed command with literal arguments and an explicit working
directory, returning a bounded, honest receipt. The command above must return a single schema-valid
JSON result with stdout `hello workbench\n`, child exit 0, execution `succeeded`, assessment
`not_assessed` and record mode `never`. A timed-out child tree must stop within the declared cleanup
allowance; discarded output must be disclosed; denied/invalid requests must cause no child effects.

Threats: untrusted argv, paths, request JSON, child output and process lifetimes. Generic operator
execution has unclassified effects and runs with the invoking account's authority. It is not an
agent read profile or a credential sandbox. No implicit shell, arbitrary environment inheritance,
fake read-only label, target health inference or silent retry is acceptable.

Local executable verification is available under Bubblewrap user/network/PID namespaces, a
read-only source/toolchain view, private `/tmp` and `/dev`, and no home credentials. The orchestrator
owns that boundary and retained evidence under `/tmp/agenticsre-poc-*`. Cally can supplement this
through its existing bounded job environment after checking the supported invocation path. No
production requests or credentials belong in tests. Windows behavior requires native evidence;
Linux results cannot establish Windows support.

Use the existing schemas and specification verifier plus explicit command/exit/output evidence.
There is no additional machine state system. Review safety-sensitive process supervision
independently; bind final executable verification to frozen source bytes. Keep useful passed
evidence, and rerun only for material changes, failures or unresolved concerns.
