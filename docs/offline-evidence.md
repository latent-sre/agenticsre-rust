# Offline evidence inspection and replay

CAP-14 and AC-18 require historical evidence to remain distinguishable from a new observation.
This increment opens the existing version-1 private store and runs the installed deterministic
`retained-observations-v1` analyzer. The [roadmap](roadmap.md) records current acceptance.

## Commands and selection

```sh
save --json --state-dir /absolute/private/store evidence open RUN_ID
save --json --state-dir /absolute/private/store evidence replay RUN_ID \
  --expected-manifest-sha256 sha256:MANIFEST_DIGEST --max-items 100
```

Both operations take a generated run ID and optional manifest digest. Structured `evidence.open`
and `evidence.replay` use the [reference input](sre-workbench/schemas/evidence-reference-input.schema.json).
Paths, alternate analyzers, commands, live targets and unknown fields are refused. The selected
store is an explicit startup option; requests cannot select another directory. Recording remains
`never`; the retained bundles and their protection/retention metadata are unchanged.

`open` verifies the envelope, artifact sizes/digests and collection/receipt bindings before
displaying a historical manifest and bounded original sanitized receipts. Receipts that cannot be
analyzed remain visible as limited inspection, with missing/incompatible reasons. Corruption,
unsupported store formats and unsafe files fail without presenting unverified data.

`replay` projects the supported retained Prometheus and Loki frames. Metric summaries report
series, samples, finite values, missing/nonfinite values and sampling gaps. Log summaries report
entries, label sets and unique/duplicate counts. Details retain exact nanosecond timestamps,
numeric JSON values, labels, log text and duplicate multiplicity. Missing measurements are never
converted to zero. Redacted or unsupported frames cannot produce a complete analysis.

## Time, provenance and bounds

The [offline result data](sre-workbench/schemas/evidence-offline-data.schema.json) contains the
original observation window and collection times, manifest reference, algorithm identity, and a
separate analysis timestamp/tool version. Identical supported source bytes produce identical
projections; request/run IDs, receipt timing and analysis time describe the new invocation.
Source labels remain `sourced` and `untrusted`. Integrity establishes retained-byte consistency,
not source authorship, present health, complete telemetry or causation.

Both commands accept `--max-items` (default 100, maximum 1000), `--max-output-bytes` (default and
effective ceiling 1 MiB for encoded detail arrays) and `--timeout-ms` (default 30000, maximum
300000). Detail limits apply across both checks as one prefix. Omitted detail counts remain
explicit while summaries cover all supported observations within parser bounds. Open counts
whole original receipts; replay counts projected samples or distinct log entries. No retained
receipt is silently shortened. The existing 2 MiB final receipt ceiling also applies.

Incomplete source capture, interruption, missing checks, unsupported observations and omitted
details produce an explicitly partial result (exit 1). Recovered artifacts outside the installed
metrics/logs pair are disclosed as uninspected. Read failures, cancellation, consumer closure and
deadline expiry cannot become successful analysis. Complete analysis has exit 0 and assessment
`not_assessed`; callers must inspect coverage and transport completion.

## Offline execution profile

On Linux x86-64, a dedicated scoped worker installs `linux-offline-v1` before opening the store.
The kernel seccomp filter refuses socket operations, network messaging, process creation,
`execve`/`execveat`, io_uring and alternate syscall ABIs. Failure to install it refuses the
operation before evidence access. The caller thread retains its existing permissions after the
worker joins. Other platforms fail closed until a native profile is implemented and verified.

The worker receives only the bundle selection, limits, store policy and cancellation control.
It has no path to credential lookup, live query dispatch, scripts or plugin callbacks. Embedded
commands and URLs are inert source data. This is a fixed-analyzer network/command restriction,
not a filesystem or credential sandbox for arbitrary extension code. Live rechecks require a
separate explicit collection. The [browser investigation extension](investigation-ui.md) grants
`evidence.open`, `evidence.replay`, `evidence.list`, `evidence.compare` and `report.create` independently against an
existing startup store; collection requires its separate recording grant. The
[MCP investigation extension](investigation-mcp.md) also independently grants `evidence.replay`
against an existing startup store, using this same fixed analyzer and offline worker.
Generic hosted execution retains its refusals without a dedicated investigation grant.

## Acceptance

Verify installed CLI/structured parity, schema-valid receipts, original receipt bytes and times,
deterministic projections, missing/nonfinite samples, duplicates, hostile log text, limited
inspection, item/byte limits, malformed references, manifest pins, corruption and unchanged store
bytes. Exercise actual kernel denial of socket and command syscalls while the caller remains
unrestricted. Run public workflows in the private network test namespace, including analysis after
fixture shutdown and removal of credentials/configuration. Existing comparison and collection
regressions must continue to pass.
