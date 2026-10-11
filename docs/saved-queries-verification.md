# Installed saved queries acceptance

WORKBENCH-012 passes its Linux core/CLI fixture contract. The ordinary binary discovers two
embedded versioned definitions and runs them through the existing scoped Grafana transport.
Direct and structured calls preserve equivalent receipts, definition identity and one resolved
parent window. Current product work belongs in [the roadmap](roadmap.md); the full behavior is in
[the query contract](saved-queries.md).

## Accepted identity

The frozen source is `target/saved-queries/review-r2`, based on HEAD
`2c4e4220654d85d5be22ac42a034beb51fcc5624` plus the preserved diagnostics/browser work and this
increment. Its [378-file manifest](evidence/saved-queries-2026-10-10/source-manifest.json) has SHA-256
`aea5c74c7d92a2890c0ba249589f7aa71070c9b28470c3acdcf19117e48910e8`.
Acceptance documentation added afterward does not change the product bytes.

The ordinary `target/debug/save` binary is 39,346,720 bytes, SHA-256
`fa1cc9eff529f6f867bd3ff8a15f4056ff7c590ef96c6bff1ed41e574fb1e104`.
A readonly DATA copy is retained at `target/saved-queries/save.data`. Its installed test copy ran
from an unrelated working directory inside the private test namespace. Both independently checked
source and binary identities matched before and after execution.

The installed version-1 definitions are:

| Definition | Exact definition SHA-256 |
|---|---|
| `prometheus.up-by-job` | `e42bc20e72b99554cc61b2c09d06b2dd3ad028a1455aa835517f2dd956c1363f` |
| `loki.logs-by-job` | `d4661dbe0efc261ccb5eb340cbcbceb90568fa2e0973b6518cfa4fd796eb6019` |

## Verification

| Check | Result |
|---|---|
| Locked, offline workspace tests in the existing sandbox | 199 passed; 0 failed; 19 existing native-environment tests explicitly ignored |
| Clippy with all targets/features and warnings denied | Exit 0 |
| Rust formatting and ordinary CLI build | Both exited 0 |
| Specifications, examples, documentation links and IDs | 31 schemas; 28 positive and 14 negative examples passed |
| New public CLI HTTPS suite | 14 passed, including default text output |
| Independent installed/schema mission | Exit 0; 24 canonical receipts, including 21 strict query-data receipts |
| Independent static correctness/security review | No remaining findings on the frozen final source |
| Source/binary preservation and fixture cleanup | All 378 source files and binary matched; fixture threads joined |

The [verification summary](evidence/saved-queries-2026-10-10/verification-summary.json) records exact
commands, exits and artifact hashes. The [independent execution index](evidence/saved-queries-2026-10-10/installed-verification-index.json)
retains the separate executor's evidence. Exact [receipts](evidence/saved-queries-2026-10-10/installed/installed-query-receipts.jsonl)
and sanitized [HTTP ledgers](evidence/saved-queries-2026-10-10/installed/installed-query-ledgers.json)
are preserved unchanged.

Tests cover both dialects and exact expressions/windows; literal quotes, backslashes and Unicode;
discovery without credential/configuration reads; definition pinning; strict invalid/null/duplicate
admission with zero HTTP effects; anchored relative and exact absolute windows; scoped permission,
org/datasource/CA refusals; HTTP-200 query errors; empty/partial observations; literal and escaped
credential echoes; parent deadlines; active SIGINT/SIGTERM; output limits; and hosted grant refusal.

One independent P2 finding was resolved: default query text bypassed the existing terminal-safe
Grafana renderer and coverage disclosure. A real HTTPS regression failed on an unescaped C1
control before the fix and passed afterward. Safe text now includes definition, parameters,
window, observation and coverage. Both [red](evidence/saved-queries-2026-10-10/text-red.log) and
[green](evidence/saved-queries-2026-10-10/text-green.log) evidence are retained. An initial synthetic
test reference omitted the required environment-name prefix; its failed workspace log and final
passing run are also retained. The final installed mission required no product or harness repair.

## Scope and reproduction

Build from current source and use [the launch examples](development.md#installed-saved-queries).
To reproduce the installed mission after a normal locked build:

```bash
tools/test-sandbox.sh .venv/bin/python -B tools/verify-queries.py target/debug/save
```

All new network execution used the existing private Bubblewrap boundary, synthetic HTTPS
certificates and synthetic provider values. Existing transport, browser and native-containment
evidence remains scoped to its own accepted source; no unchanged native Cally mission was replayed.
The 19 ignored local tests require their already documented native environment. The inherited
positive system-resolver diagnostic fixture gap remains open.

This acceptance adds no browser/MCP query grant, durable history, live Grafana compatibility,
non-Linux support, published release or host installation. The retained context development
archives predate this binary. Observations remain untrusted data and do not establish service health.
