# Saved queries in the local browser workbench

The browser independently grants `queries.list`, `queries.describe` and `query.run` through the
existing authenticated run, receipt and session-history endpoints. The two catalog operations
inspect installed definitions without configuration, credentials or network access. Executing a
query requires an explicit launcher configuration and at least one selected Grafana alias:

```sh
save ui serve --allow queries.list --allow queries.describe
save --config /absolute/reviewed-grafana.json ui serve \
  --allow queries.list --allow queries.describe --allow query.run --query-target fixture
```

`--query-target` is separate from diagnostic `--target`. A session publishes granted query aliases
as `query_targets: [{"id":"fixture","label":"fixture"}]`; diagnostic `targets` keep their existing
meaning. Endpoint URLs, configuration paths, credential references and trust files are not exposed
in session discovery. Startup validates selected aliases and pins configuration and CA identities
without retrieving credentials or sending HTTP. Unknown or duplicate grants/aliases, absent
execution prerequisites and configuration supplied to catalog-only sessions fail startup.

All three operation descriptors have version 1. The catalog profile is
`installed-query-catalog-v1`; execution uses `saved-query-v1`. Each is independently visible only
when granted. Catalog listing uses empty inputs. Description uses
`{"id":"prometheus.up-by-job","version":1}` or `loki.logs-by-job` version 1. Execution sends the
authoritative [query input](sre-workbench/schemas/query-run-input.schema.json) with a separate
granted `target_id`:

```json
{
  "submission_id": "abf7ee28-fb50-42ed-97bd-48f5b2296404",
  "operation": "query.run",
  "operation_version": 1,
  "target_id": "fixture",
  "inputs": {
    "id": "prometheus.up-by-job",
    "version": 1,
    "datasource": "metrics-1",
    "parameters": {"job": "fixture"},
    "window": {"kind": "relative", "lookback_seconds": 300}
  },
  "limits": {"timeout_ms": 30000, "max_output_bytes": 1048576}
}
```

The installed definitions supply PromQL/LogQL; the user supplies only the datasource UID, literal
job label and bounded window. Absolute windows use RFC3339 `from` and `to`; relative windows use
1–86400 seconds. The core enforces a positive span no longer than 24 hours and resolves relative
time once at the original receipt's start. An optional lowercase 64-digit
`expected_definition_sha256` pins the installed definition; an explicit null or malformed pin is
invalid and a mismatched pin yields a denial receipt before network work.

Requests reject unknown fields, expressions, endpoint/path/runtime overrides, root selectors,
concurrency and max-items controls, including explicit nulls. Catalog submissions cannot select a
target. Execution checks only the query target grant and revalidates pinned configuration/CA
identity through the core before observation. A diagnostic alias alone grants no query access.
No unrestricted `grafana.query` operation is added to the browser.

The adapter returns complete core receipts with definition provenance, resolved window, actual
observation outcome and any bounded partial/error details. Catalog data describes installed
definitions; a successful observation does not establish service health. The existing one-active-
run, cancellation, submission deduplication, bounded history and receipt download contracts apply.
Normalized request identity includes query parameters, target and limits; relative windows remain
relative in that identity so a retry retrieves its original handle without moving the window.

Verification uses the private Linux network sandbox and synthetic HTTPS credentials. It covers
independent offline catalog grants, both installed dialects, absolute/relative windows, strict
input/startup refusals, mismatched pins, configuration drift before HTTP, and history/retry behavior.
Live Grafana compatibility, native Windows and broader query definitions remain separate work.
