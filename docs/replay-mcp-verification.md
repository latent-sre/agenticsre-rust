# MCP offline replay verification — 2026-10-19

WORKBENCH-034 extends the [MCP investigation contract](investigation-mcp.md) with the
independently granted `workbench_evidence_replay` tool. It uses the existing fixed analyzer and
`linux-offline-v1` worker. Generic hosted execution still refuses evidence access; granting open,
list, comparison or reports does not grant replay. Browser authority remains unchanged.

The [installed packet](evidence/mcp-offline-replay-2026-10-19/summary.json) records **12 passing
acceptance groups**, **29 schema-valid MCP receipts** and **12 CLI parity receipts**, using an
ordinary binary copied to a disposable installation directory. Both `2026-07-28` and
`2025-11-25` protocol revisions passed. Successful and partial replay data also passed the
canonical offline-data schema and the exact discovered output schema.

| Artifact | SHA-256 |
|---|---|
| Ordinary installed CLI, 50,826,272 bytes | `256c3ae501b0879e0a1892f7dc6ce42d5bf87fc94366ce4fbe4df286e65105a5` |
| Scoped source manifest, 295 files | `5dd9cbbe33626630eba8608015e8058c7e5968299f3c2ca0aeddf751bde1ffd9` |

The source manifest covers crates, canonical schemas, dependency manifests, the synthetic context
fixture, investigation contract and acceptance harnesses. Source and binary bytes stayed fixed
throughout the installed mission. This acceptance record, retained packet and roadmap were added
afterward; they do not change product bytes.

## Established behavior

- Replay alone requires an existing private startup store and its own `--allow evidence.replay`.
  The source file is removed before the standalone mission; no live configuration or credential
  is provided. Other operations remain hidden and direct calls are refused.
- Both protocols preserve canonical CLI results, original observation windows, algorithm identity,
  analysis time and provenance. Repeated replay produces identical projections after removing only
  invocation IDs and clocks. The worker reports the existing `linux-offline-v1` profile.
- Item limits disclose omitted details and return partial results with `isError=true`. Wrong
  manifest pins, missing/corrupt bundles and a replaced store root fail without repairing evidence.
- Unknown/null authority fields, invalid references and unsupported limits fail before dispatch.
  Input/output schemas are self-contained. Replay annotations describe a read-only, closed-world
  operation; discovery grants no authority.
- Real-pipe cancellation suppresses the queued replay result and leaves ping responsive. Core tests
  cover cancellation before dispatch, generic hosted refusal and cross-operation grant refusal.
- Successful, partial, denied and failed work preserves all retained store bytes and modes.

## Checks and review

The expanded real-pipe integration tests and new core grant test first failed because replay was
not admitted; all five MCP integration tests then passed. The final focused MCP/UI run passed
35 and 12 unit tests respectively. The first broad workspace run found one stale discovery count
assertion (19 rather than 20); only that test expectation was corrected. Its failed log is retained
separately from passing evidence. No product behavior changed in response to that test failure.

The final complete workspace run passed **600 tests, zero failures and 33 existing scoped ignores**.
Strict workspace all-target/all-feature Clippy, formatting, schema synchronization and specification
validation passed. No dependency, protocol, browser asset or analyzer implementation changed.

Independent static review found no validated P0/P1/P2 issue. Its reviewed diff SHA-256 was
`71612b8a7d10bf750d465d9d03f27aa514d152411d59fb5dd555d98e178772ac`, verified unchanged afterward.
The review is provisional for mutable local bytes, not formal merge approval. The review record
identifies its excluded live/platform/host scope. At that acceptance checkpoint, no commit or merge
had been requested. The owner subsequently authorized a commit and PR on 2026-10-19; the reviewed
product bytes and installed acceptance manifest remain unchanged.

## Reproduction and boundaries

```sh
tools/test-sandbox.sh cargo build -p workbench-cli --locked --offline
tools/test-sandbox.sh .venv/bin/python -B tools/verify-investigation-mcp.py \
  target/debug/save --output target/mcp-offline-replay/fresh-acceptance
```

Use a new output directory. Tests run under the existing private Bubblewrap namespaces without
host credentials or external networking. One outer sandbox invocation refused namespace creation;
the same sandbox command ran successfully with approved execution permission. No boundary was
removed. The existing analyzer, syscall filter, schemas and protocol versions are unchanged.

Acceptance is local Linux x86-64 fixture evidence. Actual agent-host registration, native Windows,
other browser workflows, live compatibility and release acceptance remain open. Changes are local
on `codex/mcp-offline-replay` at acceptance. The owner subsequently authorized committing and opening
a PR against `main`; merge, deployment and registration remain separate actions.
