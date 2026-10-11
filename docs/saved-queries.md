# Installed saved queries — first increment contract

This increment implements CAP-11 / REQ-08 and the query portions of AC-08 / AC-16 through the
shared core and CLI. It builds on the accepted [Grafana exchange profile](grafana.md).
Implementation and acceptance status belong only in [the roadmap](roadmap.md). This contract
does not establish a working `query.run` operation or a browser/MCP grant.

## Mission and installed definitions

From the ordinary installed binary, discover two versioned queries without network work, select
an explicit configured target and datasource, and run each against private HTTPS fixtures.
The fixture observes the exact compiled expression and resolved time window. Direct CLI and
structured calls preserve equivalent observations, definition identity and original receipts.

Ship an embedded, reviewed catalog with these initial definitions:

| ID / version | Dialect and fixed expression structure | Typed parameters | Purpose / expected shape / units |
|---|---|---|---|
| `prometheus.up-by-job` / 1 | PromQL: metric `up`, one equality matcher on label `job` | Required literal `job` string | Observe the datasource's scrape samples for a job; time-series frames; dimensionless samples |
| `loki.logs-by-job` / 1 | LogQL: one equality stream selector on label `job` | Required literal `job` string | Observe the selected job's returned log entries; log frames; entries |

Each definition includes ID/version, dialect, parameter schema, expression structure, purpose,
units, expected shape, requested work limits and blind spots. Digest the exact installed definition
bytes; advertise that digest in discovery and retain it in every dispatched result. Catalog edits
require a reviewed definition/version change. Lookup validates installed identity and any caller's
expected digest before network work. V1 loads no mutable external template or caller-supplied
expression, label name, operator, pipeline, function, metric name or catalog path.

The purpose/units describe the requested observation. They do not verify the backend's metric
meaning or prove service health. Scrape coverage, absent series, sampling/staleness, label reuse,
log retention and incomplete returned data remain explicit blind spots. Empty results are not zero.

## Human and structured interfaces

Use one `queries` command family: `list`, `describe ID --version 1` and `run ID --version 1`.
Run requires an explicit connection target, datasource UID, parameters JSON and time-window JSON;
an optional expected definition SHA pins a previously inspected definition. Reuse the accepted
global `--config` provider and `--json` output. Neither discovery command reads credentials or
probes availability. The machine operation is `query.run`, operation version 1.

The intended structured input is:

```json
{
  "id": "prometheus.up-by-job",
  "version": 1,
  "datasource": "metrics-1",
  "parameters": {"job": "fixture"},
  "window": {"kind": "relative", "lookback_seconds": 300}
}
```

The existing request envelope supplies `{"kind":"connection","id":"fixture"}`, limits,
request identity and caller context. Optional `expected_definition_sha256` must be a non-null
64-character lowercase hex digest. An absolute window instead supplies `kind: absolute`, `from`
and `to` using the accepted exact-millisecond RFC3339 profile. Relative windows require integer
lookback seconds from 1 through 86400. Neither window form has an implicit production target,
datasource, job, current environment or time range.

Strict schemas and bounded parsing reject duplicate/unknown fields, wrong types, explicit nulls,
unsupported ID/version/dialect, missing/excess parameters and invalid windows. Parameters are
literal strings of 1–128 UTF-8 bytes; control characters are unsupported in this initial profile.
Retain the accepted legacy expression-macro guard; macro-looking values that it rejects remain
explicitly unsupported rather than bypassing that guard. All such refusals precede HTTP effects.

## Compilation, authority and one time window

Compile only the installed fixed expression structure. Encode the parameter as one dialect-safe
double-quoted label value; it cannot terminate the literal or add another matcher or expression.
PromQL documents Go-style string escaping, and Loki applies the same label-selector rules.
Use those rules for the supported string subset, with explicit quote/backslash and Unicode
regressions. [Prometheus language reference](https://prometheus.io/docs/prometheus/latest/querying/basics/#string-literals),
[Loki selector reference](https://grafana.com/docs/loki/latest/query/log_queries/#log-stream-selector).

Resolve a relative window exactly once at the parent operation's start, at millisecond precision,
then carry its same absolute bounds through org/datasource checks and the selected query exchange.
Do not reinterpret `now` in nested work or create another deadline. Absolute windows reuse the
accepted normalization and precision rules. The entire operation, including lookup, configuration,
binding checks and response handling, shares one monotonic deadline and cancellation control.
The parent receipt's start timestamp supplies the relative anchor, truncated to milliseconds;
the query does not sample the wall clock again after admission or an exchange. Checked arithmetic
rejects a window outside the accepted timestamp range before HTTP work.

Use the existing `grafana-legacy-v1` HTTP executor and connection policy. The selected target must
already permit `grafana.query`; a named installed query is a constrained use of that existing read
permission, not an extra destination or credential grant. Preserve exact configured origin/org/
datasource binding, CA verification, credential/provider identity, no redirects/proxies/retries,
and accepted redaction. Never call an alternate transport after refusal. Generic hosted and MCP
callers retain refusal until a separately reviewed explicit query grant is implemented.

Retain the accepted 24-hour maximum range, 16000-character expression ceiling, requested
1000-point / 500-Loki-line allowances, bounded response/result bytes and tighter caller limits.
Requested point/line limits are not proof of backend work performed or samples returned.

## Results and acceptance

Keep the canonical envelope, `not_assessed`, `record_mode: never`, actual timestamps, cancellation
and failure semantics. A strict `query.run` data schema includes installed definition ID/version/
digest, normalized parameters, actual resolved window, compiled request metadata, profile/binding,
returned query data, redaction and coverage. Reuse the accepted Grafana result vocabulary without
manufacturing another successful observation. Record omissions/incompatible shapes explicitly.
Fixed installed definition identity and generated protocol controls retain their public identity.
Private parameters, compiled expression and observational window metadata pass through the same
credential redactor as the accepted Grafana metadata; their schema admits its explicit redacted
marker. Do not attach an unredacted copy after that exchange returns. Admission failures do not
echo supplied private values. Arbitrary sensitive telemetry remains outside automatic detection.
Preserve the canonical cancellation signal and signal-specific CLI exit, and enforce the complete
encoded receipt ceiling after adding definition metadata, final timestamps and sources. If the
response must be omitted, retain definition identity and disclose the failed/partial coverage.

Required independent evidence includes:

- Installed discovery with zero HTTP effects and real PromQL/LogQL fixture exchanges, exact
  expression/window ledgers, strict schemas and direct/structured parity.
- Quotes, backslashes, Unicode and delimiter/operator-looking parameters remaining single literal
  values; macro/control/unknown/invalid inputs and definition-digest mismatch causing no HTTP work.
- One relative resolution and exact absolute normalization, range/work/output bounds, wrong org/
  datasource/CA, HTTP-200 query errors and empty/partial results.
- Credential canaries across inputs/compiled metadata/results/errors, actual parent deadline,
  cancellation, no retry or later dispatch, and joined fixture/worker cleanup.
- Affected immutable correctness/security review and real installed execution. Reuse unchanged
  Grafana transport and browser evidence; UI/MCP controls, durable storage, live compatibility and
  native platform support retain their separate requirements.

The new operation belongs before evidence collection/comparison so later bundles can preserve
an installed query identity, typed parameters and one resolved window. It does not close the
remaining CAP-11 live/platform or dependent browser requirements by itself.
