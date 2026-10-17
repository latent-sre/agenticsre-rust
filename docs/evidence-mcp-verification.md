# MCP evidence collection verification

The WORKBENCH-032 implementation adds explicit collection authority and three independent MCP
tools under the [collection adapter contract](evidence-mcp.md). All executable fixtures use the
repository Bubblewrap boundary with a private network namespace and synthetic HTTPS credentials.

Focused checks performed on 2026-10-17:

- `tools/test-sandbox.sh cargo test --locked --offline -p workbench-mcp --lib`: 35 passed.
- `tools/test-sandbox.sh cargo test --locked --offline -p workbench-cli --features test-fixtures --test evidence_mcp --test query_ui`:
  all three `evidence_mcp` tests passed. Browser-specific results belong to WORKBENCH-031.
- `tools/test-sandbox.sh cargo test --locked --offline -p workbench-core --lib evidence::grant`:
  one passed, checking missing-store startup refusal, operation scope, generic hosted refusal,
  offline-grant isolation and catalog executor scope.

The real-pipe CLI tests copy the binary to a separate installed location and prove six HTTPS requests
for the fixed two-query pack, a sealed retained bundle, and later verification through a separately
granted offline reader. They exercise catalog-only and empty grants, unknown authority fields, null
pins, invalid pack versions and limits, ungranted targets, and config/CA/source/store drift before
HTTP or retention. Empty query frames correctly produce a partial offline observation despite
complete capture; the fixture asserts that distinction.

The installed acceptance command is:

```sh
tools/test-sandbox.sh .venv/bin/python -B tools/verify-evidence-mcp.py \
  target/finish-20261017/save --output target/evidence-mcp-2026-10-17
```

The checker first proves that hosted startup cannot initialize a missing store, then initializes it
through an ordinary operator collection. Modern and legacy protocol calls inspect both catalog
tools, collect real metric/log frames, verify retained receipt and manifest digests, and read the
sealed bundle through the independent offline grant. It validates canonical and advertised schemas,
checks credential canaries, confirms strict rejection before HTTP/store effects, and rejects changed
config, CA, source and store identities. Its summary records the installed binary digest, every
retained response digest, private store file digests and a source manifest that must remain unchanged.

Installed mission: **PASS**, six acceptance groups across both protocols, using the ordinary binary
SHA-256 `7c6d4d22d386cc00c5ea714c630688cad19a5d5ac681608967afdcffcabab5c6`. The
[retained summary](evidence/evidence-mcp-2026-10-17/summary.json) links the response identities;
the packet includes all fourteen canonical responses, the operator bootstrap receipt, private
store file digests and the unchanged source manifest. Full discovery schema bytes remain in
`target/evidence-mcp-2026-10-17/discovery.json`; the checker validates their local references and
canonical output shape. Production source and installed binary remained unchanged during this run.

An independent source review of the new authority boundary found no actionable issue: exact
target/config/CA/source/store bindings, separate offline authority and fixed recording remained
intact. Existing
core quota, retention, partial-recovery and cancellation suites remain the regression evidence for
those unchanged mechanisms. Root integration records the final workspace checks and ordinary
installed binary identity. No fixture proves live Grafana compatibility, actual agent-host
registration, native Windows support or release readiness.
