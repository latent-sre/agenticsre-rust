# Service context — offline fixture contract

This is the first offline CAP-07/P3 slice after Linux packaging acceptance. It is a local fixture
adapter, not a second authoritative service catalog. The team's actual source/adapter is still a
separate DEC-09 decision. Unavailable live access does not block testing an explicit reviewed-record
export shape with synthetic records. Status belongs in [the roadmap](roadmap.md).

## Mission and authority

A human selects a bounded record export at startup and resolves a service with an explicit
environment, optional team and deployment. The result reports ownership, resource aliases,
runbook references, separate forward/reverse dependency references and the exact observed source
revision/digest/review time. No network request, command, catalog write, persistence or target
dispatch follows resolution. Mapping metadata never grants execution or credential authority.

Use the existing core/CLI result envelope and literal input normalization. Only a trusted startup
option may select the source file; structured request fields cannot select a path, source adapter,
permission, credential or runtime. UI/MCP context tools remain deferred until effective source/grant
semantics exist. Existing command, task, Grafana, UI and MCP behavior must remain unchanged.

## Initial shape and resolution rules

The fixture/export schema will be checked in with implementation, using a strict versioned
object, source ID/revision and a bounded record list. Each record includes service/team/environment,
optional deployment, active/retired state, last-reviewed timestamp, owner/resource/runbook mappings
and separate dependency directions. Preserve source declarations as data; do not relabel them as
verified approvals or live observations. Unknown keys, duplicate JSON fields, unsupported revisions,
invalid timestamps and oversized/deep input fail before resolution.

Environment is required; there is no implicit production selection. Match IDs exactly. An omitted
team/deployment selector may succeed only when the remaining identity is unambiguous. Multiple
matches do not select the first record. Retired or stale mappings are sourced history and cannot
be reported as current usable bindings. Freshness uses a required trusted startup
`--context-max-age-days` value from 1 through 365, paired with `--context-source`. Requests cannot
change either option. Preserve the supplied last-reviewed UTC timestamp; future review timestamps
are refused. This is review-age freshness of observed bytes, not proof that an external catalog has
no newer revision.

Missing optional mappings are disclosed as partial coverage. Required target/environment/org
bindings, when applicable to a record, must be complete and unambiguous. Incomplete resolution
never chooses an alternate target. Return the selected record's metadata only, with a small safe
ambiguity/missing reason; do not dump unrelated records or configuration contents on errors.
Runbook links and dependency references are data and are never followed or executed.

The initial normalized source shape is:

```json
{
  "schema_version": 1,
  "source": {"kind": "fixture-export", "id": "synthetic-team-export", "revision": "fixture-1"},
  "records": [{
    "service": "checkout", "team": "platform", "environment": "dev", "deployment": "blue",
    "state": "active", "last_reviewed": "2026-10-01T12:00:00Z",
    "required_bindings": ["target", "environment"],
    "bindings": {"target": "fixture-checkout", "environment": "dev"},
    "owners": ["platform"],
    "resource_aliases": [{"kind": "grafana.dashboard", "alias": "checkout"}],
    "runbooks": ["https://example.invalid/runbooks/checkout"],
    "dependencies": {"forward": [{"service": "database", "environment": "dev"}], "reverse": []}
  }]
}
```

`required_bindings` is an explicit per-record policy, limited to `target`, `environment` and
`organization`. A declared required binding cannot be absent or empty. A supplied environment
binding must match the record's explicit environment. Source/record/input objects reject unknown
fields; no credential or approval field exists. `source.kind` must be `fixture-export`, qualifying
the observations as an offline export throughout the result. The schema bounds IDs, strings and
arrays; source limits are 256 KiB, depth 16 and 128 records, and selected structured output is
limited to 64 KiB before result finalization. Missing optional fields remain distinct from a
source-declared empty list, including each dependency direction.

The CLI accepts `context resolve --service ID --env ID [--team ID] [--deployment ID]`.
Both startup options are paired and are rejected for `ui serve` and `mcp serve`. Structured calls
use operation `context.resolve` with selectors in `inputs`; the envelope target remains
`local/workstation`. Discovery reports `requires_configuration` without opening the source.
Only a unique active, fresh record with all declared required bindings may report usable resolution;
optional omissions are disclosed as partial coverage. All other outcomes disclose their reason
without substituting a target. Observed records use `sources[].evidence_state="sourced"`.
Assessment remains `not_assessed` and recording is `never`. Successful metadata observations have
effects `not_applicable`; refusals use the existing result/error categories and perform no effect.

Pin and inspect a regular file before opening it for data, refusing FIFO/device/symbolic-link layouts.
Reuse the existing duplicate-key/depth protections, shared output limits, cancellation semantics
and honest failure categories. Bound source bytes before parsing, validate bounded record counts
and strings, and measure selected encoded output before publishing the result. Source identities
describe the bytes actually read. The Linux implementation requires `openat2` and trusted proc
descriptor access; unsupported prerequisites fail without following a user path as a fallback.

## Required evidence

- Real CLI and structured-call parity for a unique synthetic service/environment record.
- Explicit environment, optional selector disambiguation, no first-match or production fallback.
- Ambiguous, absent, retired, stale, future-dated and incomplete-required mappings.
- Optional partial mappings and differing forward/reverse dependencies preserved without inference.
- Malformed/duplicate/unsupported/oversized input, FIFO/symlink refusal and unchanged source bytes.
- Malicious record text treated as data, no network/command/credential/catalog effects.
- Source identity and typed output schema; unchanged existing operation/grant regressions.
- Independent review and Cally execution on frozen bytes before scoped acceptance.

Current source implements this core/CLI fixture boundary. The accepted Linux archives in
[packaging verification](packaging-verification.md) were built before this context increment;
their exact binary/source identities remain separate. Rebuild from the context source to use
`context resolve`; those earlier bundles do not contain it. Native evidence and remaining limits
are recorded in [context verification](context-verification.md) and the roadmap.
