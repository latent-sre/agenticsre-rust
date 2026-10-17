# MCP evidence collection

WORKBENCH-032 exposes three independently granted tools through the existing bounded stdio server:

| Startup operation grant | MCP tool | Effects |
|---|---|---|
| `evidence.packs.list` | `workbench_evidence_packs_list` | Embedded metadata only |
| `evidence.packs.describe` | `workbench_evidence_packs_describe` | Embedded metadata only |
| `evidence.collect` | `workbench_evidence_collect` | Grafana observations and private local store writes |

The catalog needs no config, provider, target, context or store. Description takes exactly
`{"id":"service-queries","version":1}`. Both catalog grants and collection remain independent
of the saved-query and offline investigation grants. Default discovery remains empty.

```sh
save --config /absolute/grafana.json \
  --context-source /absolute/context.json --context-max-age-days 30 \
  --state-dir /absolute/existing-private-store \
  mcp serve --allow evidence.packs.list --allow evidence.packs.describe \
  --allow evidence.collect --target reviewed-target
```

Collection requires an already initialized private evidence store. Start-up refuses missing,
uninitialized or unsafe stores without creating them. The existing operator collection workflow
can initialize a store; the hosted adapter cannot. Start-up keeps the existing `--record never`
adapter option; explicitly granting `evidence.collect` authorizes its fixed `record=always` request.
No per-call recording override is accepted. The one-GiB/seven-day default store policy remains in
force; the existing reservations, quotas, locks, no-follow storage and retention semantics apply.
This grants no deletion, pruning, protection, recovery or other management operation.

`workbench_evidence_collect` accepts the strict [collection input](evidence.md) plus a required
`target_id` naming one launcher-selected alias and optional bounded `limits`. The operation runs
the installed `service-queries` version 1 pack only. Limits default to 60,000 ms and 2,097,152 output
bytes and cannot exceed those values. Concurrency and item-limit controls are not accepted.
Input cannot supply a path, endpoint, credential, expression, arbitrary step, policy or recording
override. An optional `expected_pack_sha256` pins the installed pack. Relative windows remain
unresolved until the core starts the actual collection, then both query children share one window
and parent deadline.

The opaque collection grant holds only its selected targets, bounded Grafana configuration/CA
identity, context-source byte identity, age policy and the existing store descriptor identity.
Every invocation checks the exact consumed config, CA and source bytes. The store is opened without
creation and its identity is checked on the actual descriptor before retention or HTTP dispatch.
Configuration/source drift or store replacement fails rather than rebinding the session. Credential
scope still independently permits `grafana.query`; context metadata never grants network authority.

Generic hosted execution still refuses evidence collection. The existing offline investigation
grant has no new write authority. Reading a new bundle through `workbench_evidence_open` requires
a separate explicit `--allow evidence.open`, which can also be used in a new offline server process.

Collection discovery advertises `readOnlyHint=false`, `destructiveHint=false`, and
`openWorldHint=true`: the local store mutation is explicit and the operation performs network
observations without granting deletion. Catalog tools retain read-only, closed-world annotations.
Both negotiated protocol forms return the canonical bounded receipt with sanitized step outcomes
and a durable bundle reference when sealing succeeds. Cancellation, partial capture, child errors,
quota exhaustion and integrity checks use the existing core behavior. Complete retention is not
complete telemetry coverage or a service-health verdict; retained and returned content stays
untrusted data. A subsequent offline open may be partial for empty or unsupported observations even
when collection successfully sealed both receipts.

Linux fixture acceptance is recorded in [verification](evidence-mcp-verification.md). Agent-host
registration, live target compatibility, native Windows, new packs and other recording policies
remain separate work.
