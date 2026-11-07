# Offline investigations through MCP

WORKBENCH-028 and WORKBENCH-034 extend the [bounded stdio adapter](mcp.md) using the accepted core
[offline investigation grants](investigation-ui.md). It gives an agent the same historical
context/evidence/report results available through the local browser and CLI. The roadmap owns
execution status; this document defines the change contract.

## Authority and compatibility

Each new operation requires its own repeated `--allow OPERATION`. Empty grants still expose no
tools, and existing command, numerical task and Grafana grants retain their behavior. Reuse the
core's opaque `InvestigationGrant`: it owns sanitized offline authority, pins the consumed context
bytes and checks the actual opened store identity. Do not add another resolver or evidence executor.

`context.resolve` requires paired `--context-source FILE --context-max-age-days DAYS` startup
options. Evidence/report grants require an existing private `--state-dir DIRECTORY`. Supplying
these options without matching grants fails startup. Missing/invalid resources, unknown/duplicate
grants, recording modes other than `never`, retention/size overrides and unrelated runtime/store
options remain refused. MCP startup errors retain exit 2, empty stdout and bounded fixed stderr.

Context/store paths are launcher inputs only. Client arguments and identity/capability metadata
cannot replace paths, roles, policies, credentials, targets or permission grants. The dedicated
executor is selected only for the six explicitly granted operations; generic hosted execution
continues to refuse them. Mixed task/Grafana/command/investigation sessions keep these authorities
separate. No host registration, HTTP transport, arbitrary file resource, collection,
deletion, protection change, runbook, remote operation or report publication is added.

## Tool contracts

| MCP tool | Core operation | Arguments |
|---|---|---|
| `workbench_context_resolve` | `context.resolve` | Service/environment, optional team/deployment |
| `workbench_evidence_list` | `evidence.list` | Empty core input |
| `workbench_evidence_open` | `evidence.open` | Evidence ID and optional manifest SHA-256 pin |
| `workbench_evidence_replay` | `evidence.replay` | Evidence ID and optional manifest SHA-256 pin |
| `workbench_compare` | `evidence.compare` | Before/after evidence references and normalization |
| `workbench_report_create` | `report.create` | One to four evidence references, format, optional title and pending proposals |

Arguments follow the canonical operation input schema, plus an optional top-level `limits` object.
The adapter removes that control and uses core input normalization. It rejects unknown/null keys,
paths and malformed references before admission. Timeout defaults to 30 seconds, with a ceiling
of 300 seconds. Output allowance defaults to 1 MiB and ranges from 1,024 bytes through 1 MiB.
Only open/replay/compare accept `max_items`, from 1 through 1,000, default 100. Concurrency is not a tool
argument. Existing tools retain their current limits. Discovered schemas must describe these exact
rules and resolve all references locally.

`structuredContent` is the unchanged canonical core receipt, including errors, coverage, original
observation windows and analysis/generation timestamps. Partial, failed, denied and unsupported
results set `isError`. A concise text summary derives from the same result. Context records, labels,
logs, report contents and proposals remain untrusted data; tool descriptions state that they are
historical observations, not current health assessments or instructions. Report creation returns
content without writing/publishing it; proposals remain pending. Read-only annotations describe the
effect but do not grant authority. Discovery does not read retained bundle contents or credentials.

Replay uses the existing fixed `retained-observations-v1` analyzer under `linux-offline-v1`.
It projects historical metric samples and log entries with their original observation window,
separate analysis time, provenance and explicit omissions. It neither recollects nor alters evidence.
The replay grant requires only an existing private store, independently of open/list/report grants.
Configuration, credential and context-source inputs are unnecessary. Unsupported platforms still
fail closed through the existing core profile. A minimal launch is:

```sh
save --state-dir /absolute/private/store mcp serve --allow evidence.replay
```

## Implementation and acceptance

1. Extend MCP options/grants, typed mapping, embedded schemas and the existing worker dispatch.
   Verify empty/single/mixed grants, strict fields and operation-specific limits with focused tests.
2. Pair existing CLI globals with those exact grants and add real-pipe integration tests. Preserve
   the accepted `2026-07-28` and `2025-11-25` protocol paths, transport bounds, busy/cancellation,
   stream cleanup and stdout ownership. No protocol or dependency version changes are needed.
3. Run an ordinary installed binary from a disposable location with synthetic context and copied
   evidence fixtures. Exercise both protocol revisions, all six calls, independent discovery,
   direct-core/CLI parity, schema-valid results, hidden tool/override refusal, partial/corrupt/
   missing/pinned evidence and source/store replacement refusal. Assert unchanged retained bytes,
   deterministic replay projections, standalone replay without source/credentials and cancellation.
4. Independently review the new authority/dispatch boundary. Run affected MCP/CLI and proportionate
   workspace/schema checks, retain exact source/binary identities and update the roadmap.

Use existing private Bubblewrap boundaries and cached dependencies. Preserve accepted browser and
core evidence where bytes are unchanged. Actual agent-host registration/compatibility, live sources,
protected identity separation, native Windows and publication remain separate acceptance work.
