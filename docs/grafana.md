# Native Grafana fixture adapter — implementation contract

This increment adds `grafana.dashboard.get` and `grafana.query` through the shared core and CLI.
Use the [pinned helper and exchange fixtures](adapter-notes.md) as the compatibility baseline.
Current status belongs in [the roadmap](roadmap.md). Initial acceptance uses a controlled local
HTTPS service and synthetic credentials; it does not establish live Grafana-version support.

## Operator interface

```text
save --config /absolute/operator.json grafana dashboard get --target fixture --uid board-1
save --config /absolute/operator.json grafana query --target fixture --datasource metrics-1 --kind prometheus --from 2026-10-02T00:00:00Z --to 2026-10-02T00:01:00Z --expr up
save --config /absolute/operator.json --json call --request request.json
```

Both interfaces use the existing operation input schemas and named connection target
`{"kind":"connection","id":"fixture"}`. Wire requests cannot supply config paths, credentials,
origins or transport overrides. Existing process/task operations retain local-workstation targets;
loosening shared request parsing must not make a remote-looking command execute locally.

No implicit config discovery, persistence or live credential setup. The explicit config path must
identify a bounded regular file: 64 KiB, depth 16, at most 16 named connections. Example:

```json
{
  "spec_version": "0.1",
  "record": "never",
  "connections": {
    "fixture": {
      "origin": "https://localhost:8443/grafana",
      "expected_org_id": 7,
      "api_profile": "grafana-legacy-v1",
      "credential_ref": "env:SAVE_GRAFANA_FIXTURE",
      "tls_ca_file": "/absolute/fixture-ca.pem"
    }
  }
}
```

Extend the existing config schema: storage settings may be omitted with `record=never` and remain
inactive when supplied; retain their validation. Correct the organization ceiling to the exact
signed-64-bit maximum `9223372036854775807`. Add an optional absolute CA-file path, with a 64 KiB
read bound, for explicit private trust roots. Keep certificate and hostname verification enabled.

The selected environment reference resolves one bounded, strict JSON provider value (16 KiB,
depth 8), supplied by the trusted launcher:

```json
{
  "origin": "https://localhost:8443/grafana",
  "expected_org_id": 7,
  "operations": ["grafana.dashboard.get", "grafana.query"],
  "auth": {"kind": "bearer", "token": "synthetic-token"}
}
```

Also support exclusive tagged Basic auth with username/password. Before HTTP, match the normalized
origin, organization and operation against this provider scope. This catches accidental config
retargeting; files and environment controlled by the same OS account do not establish protected
agent identity. Generic commands and Python tasks retain their cleared child environments.
Discovery performs no HTTP, runtime probe or secret-value dump. Missing/invalid references fail
with static errors; values never appear in errors, Debug output or traces.

## HTTPS execution and limits

Use a native async client with locked dependencies, a current-thread runtime and cancellable DNS.
The reviewed starting choice is reqwest 0.13.5 with defaults disabled, Rustls and asynchronous DNS,
plus Tokio. Confirm actual feature/API configuration in the locked source during implementation.
Explicitly disable ambient proxies, redirects, retries, compression, cookies and TLS key logging.
Default TLS trust uses the compiled Mozilla roots; an explicit CA uses only the supplied bundle.
Ambient `SSL_CERT_FILE`/`SSL_CERT_DIR` do not replace either choice. DNS uses the system resolver
configuration without an implicit public-resolver fallback. Missing resolver configuration fails
name resolution explicitly; numeric-IP targets do not require a resolver.
See the official [client controls](https://docs.rs/reqwest/0.13.5/reqwest/struct.ClientBuilder.html)
and [retry policy](https://docs.rs/reqwest/0.13.5/reqwest/retry/fn.never.html).

Give this operation a 60-second default and hard maximum, with each request bounded by 20 seconds
and the remaining operation time. Include config/credential admission where it can be bounded,
connection, TLS, every sequential request and response-body consumption in the deadline.
Poll the existing cancellation signal during I/O; drop client work and preserve exit 130/143.
Cancellation of the client does not prove cancellation of remote datasource work. Keep process
supervision unchanged and share one core implementation between human and structured interfaces.

Use only the explicitly selected `grafana-legacy-v1` profile. Do not probe/fall back to a different
API family. Profile fixture conformance does not advertise compatibility with an untested server.
Preserve the configured base-path prefix, verified org readback, datasource UID/type/org checks,
fixed routes, headers and query body described in the adapter notes.

Keep the 24-hour range, 16,000-character expression, requested 1,000-point and 500-Loki-line bounds.
Reject unresolved macros, control characters and invalid UIDs. Parse RFC3339 offsets, normalize
UTC and reject timestamps that would require sub-millisecond rounding. Cap each response at 2 MiB
by default and honor a tighter caller output limit. Bound response JSON to 64 containers of nesting,
separate from configuration/provider bounds. Disclose requested limits without claiming a backend
returned fewer points solely because a request parameter was set.

Timestamp admission enforces RFC3339 syntax, including a `T` or `t` date/time separator, before
conversion by the time library. Extra fractional digits may be zero; any nonzero precision beyond
milliseconds is rejected. A library's acceptance of other separators does not extend this contract.

Reject malformed, duplicate-key, nonfinite, deeply nested and oversized JSON, non-success HTTP
statuses, query errors under HTTP 200 and incomplete/unexpected query results. Empty frames are
successful observations with coverage `not_established`. Error bodies and transport exceptions
are never copied into results. There are no hidden retries.

## Results and redaction

Add strict operation-specific data schemas for adapter/profile identity, normalized request
metadata, returned dashboard/meta or query results, redaction summary and cancellation signal.
Child status is null, assessment is `not_assessed`, and attempted observations use effects
`not_applicable`. Results retain source/vantage/time and make no rendering or service-health claim.
Shared envelope admission errors precede adapter dispatch and retain the generic result/error
contract. The operation data schemas apply once the adapter is dispatched, including its own
configuration and transport failures.

Validate target and response identities before masking. Redact private credential echoes and
known encodings from remote data, strings/keys, numeric/boolean/null values, URL userinfo and
authentication-header fields. Use explicit redacted-marker unions for typed observational
metadata. Reject masked-key collisions instead of silently overwriting values.

Keep public request/envelope identity and generated control fields typed and correlated; a
synthetic token equal to a JSON keyword must not break control schemas. Tests trace private
credential flow into data, rather than asserting impossible literal-byte absence throughout
generated controls. Document the precise boundary and any resulting adapter difference from the
upstream whole-result masker. Recheck the final encoded 2 MiB receipt limit after redaction;
incomplete/oversized structured output cannot remain successful and complete.

The accepted masking evidence covers literal credentials and canonical percent/form/base64
variants. It does not establish arbitrary decoding equivalence, including lowercase or mixed-case
percent escapes, or detection of unrelated sensitive telemetry. See the
[acceptance record](grafana-verification.md) for the tested scope and corrections.

## Acceptance

Use a real loopback HTTPS server inside the isolated test network, a synthetic CA with
hostname-valid certificates and a request ledger. Prove exact dashboard/Prometheus/Loki exchanges,
prefix and CLI/call parity, and that org/datasource mismatches prevent later requests. Exercise
unknown CA, wrong hostname, redirects, proxy canaries, HTTP/query errors, oversized/invalid bodies
and empty frames. Check credential variants, key collisions, primitive echoes and stable controls
against result schemas.

Use slow headers/bodies and sequential requests to prove whole-operation deadlines, SIGINT/SIGTERM,
no retry and no further dispatch after cancellation. Invalid config/profile/reference, changed
credential scope, dates/macros and per-operation targets must fail before HTTP. Retain command/task
regressions, installed execution and Cally's bounded offline fixture lane. Native Windows, live
credentials/targets and protected agent permissions remain later acceptance boundaries.
