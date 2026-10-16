# Diagnostic MCP local verification

WORKBENCH-030 adds three independently granted tools to the bounded Linux stdio adapter:
`diagnostics.list`, `diagnostics.describe` and `diagnostics.run`. This record describes the
locally committed `codex/offline-evidence` work after saved-query commit `c0e10b0`. The diagnostic
executor, pack definition, schema and private test fixtures are reused.

The installed-copy mission in `tools/verify-diagnostics-mcp.py` ran the ordinary executable from
a private directory outside the checkout. The catalog returned the fixed pack without DNS or
HTTPS work. The run tool used a launch-pinned target from `--diagnostics-config` and
`--diagnostic-target`, returning all five successful DNS/TCP/TLS/HTTP/process observations under
modern `2026-07-28` and legacy `2025-11-25` MCP. The modern receipt validated against the published
result and diagnostic-data schemas and matched a direct structured CLI call after excluding only
volatile observation fields. Tool discovery exposed local input/output schemas and the exact
selected target ID.

Hidden tools, a different target, endpoint override, explicit null input and over-limit concurrency
were rejected before fixture effects. A changed trusted configuration was refused before another
DNS or HTTPS request. Startup refused missing diagnostic authority, unused configuration and use
of the Grafana `--target` selector in place of `--diagnostic-target`. Separate target selectors
allow a launcher to grant Grafana and diagnostics independently.

Verification on the current product code included 35 MCP unit tests, the installed mission, 15
affected existing MCP integration tests with one pre-existing native test ignored, ordinary CLI
build and strict all-target/all-feature Clippy for the CLI/MCP packages. Formatting, specification
validation and `git diff --check` passed. Independent read-only source review found no validated
P0/P1/P2 issue in the pre-commit diff; it did not rerun tests or grant an immutable-revision verdict.
The host Cargo registry was unavailable, so locked
dependencies were fetched to a disposable `/tmp` cache and tests ran inside the repository's
private Bubblewrap wrapper.

This fixture result does not establish live target compatibility, agent-host registration,
native Windows support, a protected separate OS identity or the existing positive system-resolver
case. The new authority path is committed locally and has not been pushed.
