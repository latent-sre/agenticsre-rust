# Saved-query MCP local verification

WORKBENCH-029 extends the bounded Linux stdio adapter with three independent installed-query
grants. This record describes the current uncommitted `codex/offline-evidence` working tree based
on `4644ab3`; no published revision or live agent-host registration is claimed.

The real-pipe fixture copies the ordinary executable to a private location outside the checkout.
It discovers the two installed definitions without a connection or credential, then executes
PromQL under protocol `2026-07-28` and LogQL under `2025-11-25` against synthetic HTTPS. Exact
compiled expressions, definition digests, canonical core receipts and bounded target selection
are checked. A mismatched definition digest, unknown tool/target, explicit null pin and changed
configuration are refused without an HTTP request. A separate direct-core test confirms that the
saved-query entrypoint cannot dispatch an arbitrary Grafana expression.

The complete workspace Rust test command passed inside `tools/test-sandbox.sh` after the MCP
implementation and first installed-copy tests. Formatting, strict all-target/all-feature Clippy,
`git diff --check` and specification verification also passed. The final direct-core error-status
test and unique staged executable copy were added afterward; the two affected MCP integration
suites passed on those final product bytes. A read-only review found no validated P0/P1/P2 issue
in the working-tree diff; it reserved an immutable-revision verdict because this work is not yet
committed.

```bash
tools/test-sandbox.sh cargo test --workspace --all-features --locked --offline
tools/test-sandbox.sh cargo test -p workbench-cli --test mcp_queries --test mcp_authority --features test-fixtures --locked --offline
tools/test-sandbox.sh cargo clippy -p workbench-core -p workbench-mcp -p workbench-cli --all-targets --all-features --locked --offline -- -D warnings
cargo fmt --all -- --check
.venv/bin/python -B docs/sre-workbench/verify_specs.py
git diff --check
```

The initial copied-executable test had one transient Linux `ETXTBSY` launch failure during a
combined suite. The copy now uses a unique staged path and rename before launch; the affected
combined suite passed afterward. The ordinary query executor, TLS transport, bounded stdio loop,
and core result schemas were reused without behavioral changes.

Local fixture evidence does not establish live Grafana compatibility, agent-host registration,
native Windows behavior, a release artifact or a hosted credential-isolation boundary.
