# Adapter baseline after the command-runner PoC

Source identity: Save Toolkit commit `7d900fb999abee3b3712e1da881dcc7e80b1f6e7`, the same revision
as the imported plan. A targeted source read verified Git blob hashes against that commit's tree.
The older local checkout is not the compatibility baseline. Preserve its MIT notice already
retained under `third-party/` when importing helper code. No helper has been run against live data.

| Helper | Path at the pinned commit | SHA-256 |
|---|---|---|
| Dashboard hygiene | `skills/grafana/scripts/dashboard_hygiene.py` | `8a109e921332b2da99885cda111ad6b6e7d22965291fd2bc4ddd0199876ec5b2` |
| Error budget | `skills/obs-alerting/scripts/error_budget.py` | `4e55475e6b10a4a2e195b1a6f162d688b093664c06bee69cb09435f1931cf5c2` |
| Grafana reads | `skills/grafana/scripts/grafana_read.py` | `1c1711d8bd1c70e49e5715813506a0a4389021041f457d9d6b1a95a39f8fc3f7` |

## First offline task: dashboard-hygiene version 1

The [checker](https://github.com/latent-sre/save-toolkit/blob/7d900fb999abee3b3712e1da881dcc7e80b1f6e7/skills/grafana/scripts/dashboard_hygiene.py)
already separates `unwrap`, `iter_panels` and `check`; the latter returns rule/location/detail
tuples. Wrap these functions in bounded structured JSON, with fixed installed entrypoint/digest and
interpreter identity. Do not extract findings from terminal text. Task identity/version is a product
contract; these upstream scripts have no independent semantic version.

Preserve Classic/V1 models, app-platform wrappers and legacy dashboard/meta envelopes; reject V2.
Check titles/descriptions, units, datasource references, targets, noValue, variables and tags.
Run PromQL heuristics only with an explicit Prometheus datasource type, mask quoted literals before
matching, and retain valid `increase(...[$__range])` totals. Report findings as a completed check
with limited model-hygiene coverage. No rendering or live correctness claim follows.

Import independent expected fixtures from `scripts/test_dashboard_hygiene.py`: clean and one-field
mutations, mixed datasource types/overrides, quotes/regex brackets, nested rows, wrappers, unsupported
V2 and unreadable models. Add product AC-19 rejection for zero recognized panels, missing/wrong
field types and oversized input. Keep the original input unchanged.

## Error-budget task

The [calculator](https://github.com/latent-sre/save-toolkit/blob/7d900fb999abee3b3712e1da881dcc7e80b1f6e7/skills/obs-alerting/scripts/error_budget.py)
needs a pure structured seam before wrapping. Allowed bad fraction is `1 - slo/100`; budget is
window minutes or eligible event count times that fraction. Remaining budget is budget minus
consumption. Preserve explicit units and near-zero tolerance `max(abs(budget)*1e-12, 1e-12)`.
Burn rate is `(1 - sli/100) / allowed_fraction`.

Both bound windows must reach their threshold: 1h/5m at 14.4 for page, 6h/30m at 6 for page,
3d/6h at 1 for ticket. One window cannot establish an alert. These are fixed 30-day example policy
thresholds; a changed status horizon does not rescale them. Projection uses the full budget under
steady eligible volume, not remaining-budget runway. Preserve those limitations in structured
output. Upstream `scripts/test_error_budget.py` supplies unit, exhaustion and multi-window fixtures.

## Native Grafana fixture adapter

The [reader](https://github.com/latent-sre/save-toolkit/blob/7d900fb999abee3b3712e1da881dcc7e80b1f6e7/skills/grafana/scripts/grafana_read.py)
has a structured `run(args, environ, transport)` seam and injected-transport tests. Capture expected
HTTP exchanges before porting: verified HTTPS origin, explicit org readback, datasource UID/type,
org header on every call, fixed legacy dashboard/query routes, no redirects or ambient proxies.

Retain bounds: 24-hour window, 16,000 expression characters, 1,000 requested points,
`intervalMs=max(1000,ceil(range_ms/1000))`, 500 Loki lines, 20-second request timeout and 2 MiB
response. Reject unresolved macros/control characters and HTTP-200 query errors. Empty frames
mean successful query with coverage not established. Add the product's whole-operation deadline
and explicit server compatibility profile; the helper supplies neither a version-negotiation
contract nor panel transformation/rendering equivalence.

Use a controlled local HTTP server and synthetic credentials first. Live access remains a
separate target/identity decision. CLI and later MCP must share the same core operation results.
Current execution order and status remain solely in [the roadmap](roadmap.md).

### Captured Grafana exchange expectations

The pinned `scripts/test_grafana_read.py` provides a concrete starting fixture sequence. With
synthetic origin `https://monitor.example/grafana`, org 7 and a synthetic token:

1. Every operation first sends `GET /api/org`; require integer `id` 7 before any selected read.
2. Dashboard reads send `GET /api/dashboards/uid/board-1`; require matching dashboard UID and
   object `meta` in the legacy envelope.
3. Queries send `GET /api/datasources/uid/metrics-1?ds_type=prometheus` (or `loki`); require
   matching UID/type and, when present, matching integer `orgId` before query dispatch.
4. Query execution sends `POST /api/ds/query` with string millisecond `from`/`to`, exactly one
   query `refId=A`, datasource UID/type, literal expression, maxDataPoints 1000 and the bounded
   interval. Prometheus uses range=true, instant=false and format=time_series; Loki uses
   queryType=range and maxLines=500.

Every exchange carries the selected org header and synthetic authorization. Tests must distinguish
HTTP failure, redirect, malformed/oversized/nonfinite JSON, mismatched target, query-level errors
and empty frames. Error bodies and transport exception strings are never authoritative output.
Preserve the reference tests for secrets in nested keys, values, numeric/boolean/null echoes,
URL userinfo, auth headers and common encodings. Their masking can change types; the Rust port
must account for that explicitly in typed result/control metadata rather than silently applying
a schema-breaking blanket transformation.

The [accepted fixture adapter](grafana-verification.md) uses a bounded explicit operator config,
scoped environment credential reference, fixed compatibility profile and a real local HTTPS
fixture boundary. It keeps RFC3339 time strings and named connection targets, with whole-operation
timeout/cancellation evidence across requests. The config schema and parser now use the exact
signed-64-bit organization bound `9223372036854775807`. Live profile support and credentials remain
separate from these fixtures.

Grafana's current documentation was checked on 2026-10-02: version 13 deprecates the legacy
`/api` routes in favor of `/apis`, while keeping the legacy routes operational for now. Some
replacement APIs differ in structure. This supports retaining an explicit legacy compatibility
profile and testing newer profiles separately; it does not establish compatibility with an
unexamined server. See [Grafana API structure](https://grafana.com/docs/grafana/latest/developer-resources/api-reference/http-api/apis/).
