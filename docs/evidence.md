# Evidence collection and comparison

This contract implements CAP-09/10, REQ-07 and AC-11/13/17 in roadmap order. Status belongs in
[the roadmap](roadmap.md). It requires real collection through the accepted context and saved-query
operations; importing arbitrary receipts does not substitute for collection.

## Collection

`evidence.collect` version 1 takes a named connection target and strict inputs:

```json
{
  "service": "fixture-web",
  "environment": "dev",
  "pack": {"id": "service-queries", "version": 1},
  "window": {"kind": "relative", "lookback_seconds": 300},
  "metrics": {"datasource": "metrics-1", "parameters": {"job": "fixture"}},
  "logs": {"datasource": "logs-1", "parameters": {"job": "fixture"}},
  "on_failure": "continue"
}
```

Optional team/deployment selectors use the context resolver's exact matching rules. An optional
`expected_pack_sha256` pins the inspected installed definition. Explicit nulls, duplicate/unknown
fields, arbitrary operations or steps, unsupported versions and invalid query inputs are refused
before store or HTTP effects. The installed `service-queries`/1 pack contains exactly the reviewed
Prometheus up-by-job and Loki logs-by-job version-1 queries, including their installed digests.
Discovery reads embedded metadata only.

Collection requires a unique active, fresh context record from the explicit startup source. Its
target must equal the selected connection, its environment must equal the input environment, and
its organization must equal the configured Grafana organization rendered as canonical decimal.
These bindings are required for collection even where the source's general policy made them
optional. Context is sourced metadata, not a grant. The existing provider must independently
permit `grafana.query` on that exact origin/org. Configuration and CA identities are pinned for
the entire pack; later changes fail instead of retargeting a child. CA provenance distinguishes
the adapter's embedded `builtin` roots from a `custom` bundle with its exact digest; it does not
claim to load the host operating system's trust store.

Resolve the supplied window once from the parent's start timestamp using the saved-query rules.
Both children receive the same absolute bounds and one parent monotonic deadline, with a 60-second
ceiling. Children have truthful individual start/end times; nested execution cannot reset the
budget. Sequential execution avoids an extra runtime or scheduler. `continue` records each outcome;
`stop` records later checks as omitted after a failure. Signals, deadline expiry and consumer hangup
stop further dispatch and cancel active network work. There are no hidden retries.

Store only finalized, already-redacted child receipt bytes, including their canonical trailing
newline. Never append raw parameters, compiled expressions, configuration or source-file contents.
The context projection retains only selected binding/source provenance needed by the bundle;
known credentials in private projection/window metadata pass through the same credential redactor.
Generated envelope identity and protocol controls retain their existing public meaning. Unrelated
sensitive telemetry is not automatically detected. Evidence is classified `restricted` initially.

The parent result contains bounded step summaries and artifact references, not full child payloads.
Every expected step remains visible, including its omission/failure and retained receipt if any.
Capture completeness is separate from telemetry coverage: complete retention of successful query
receipts does not establish sample coverage or service health. The unfinished outer parent receipt
is not written inside dispatch; its run ID correlates with the sealed collection manifest.
The terminal data adds a bundle reference and `delivery_cancellation_signal` outside that immutable
payload. A signal observed during finalization is retained in the sealed payload when checked
before sealing. A later signal never changes those sealed observations or capture completeness:
delivery reports it separately and still returns the corresponding signal exit code. If it arrives
during output delivery, that exit code may be the only delivery notice available.

## Recording and local store format 1

The first consumer settles DEC-08 on a local, versioned file store. Explicit startup
`--state-dir`, `--store-max-bytes` and `--retention-days` options select the trusted policy;
wire requests cannot select filesystem paths or change quotas. Defaults are one GiB and seven days.
The initial maximum is one GiB, 1,024 runs and eight MiB reserved per collection. A bounded derived
reservation honors tighter response limits. Scans count metadata, artifacts and staging bytes.
No younger run is silently evicted to admit another collection.

`evidence.collect` supports `auto` and `always`; both retain the selected sanitized evidence.
`never` returns `recording_required` before store/network effects. Existing operations keep their
accepted `never` behavior. Read-only evidence listing/show/comparison uses `never`. Explicit store
management operations describe their local writes rather than claiming observational effects.

Directories are owner-private and files are owner-readable/writable only. Store mutation requires
one nonblocking exclusive OS lock, held across a bounded collection; contention returns
`store_busy`. Operations use pinned directories, generated names and no-follow descriptor-relative
access. Refuse links, special files, wrong ownership, unsafe modes, unsupported versions and
unbounded layouts. Unknown formats are never upgraded or deleted implicitly.

Each generated run directory contains an intent, an atomic progress checkpoint and content-addressed
receipt files under `artifacts/<sha256>.json`. Artifact paths are genuinely run-relative. Format 1
allows no cross-run references or hardlinks; duplicate bytes across runs count against quota.
This trades disk deduplication for straightforward ownership, export and safe retention. A later
index/deduplicating format needs explicit migration/rollback evidence.

Write and synchronize each private temporary file, publish it atomically without overwriting a
different object, then synchronize its directory. Checkpoint retained references after artifact
publication. Publish the sealed manifest last and return a durable bundle reference only after its
commit succeeds. A manifest never contains its own digest. An interrupted intent or unreferenced
artifact does not establish a completed observation.

On failure, report the exact confirmed retained references and omissions. Disk-full, permission,
quota and synchronization failures cannot yield complete capture or a successful bundle reference.
Explicit recovery reconciles abandoned intents/checkpoints into interrupted partial records;
it never repeats HTTP work. Readers distinguish sealed, interrupted and corrupt records. A
read-after-restart must revalidate object type, size, digest and supported receipt/manifest shape.

Cancellation finalization may use at most a declared 2.5-second cleanup allowance for already
observed bytes and incomplete metadata; it cannot dispatch another observation. No detached writer
continues after terminal completion. Bounded synchronous I/O cannot forcibly preempt an
uninterruptible kernel filesystem operation; do not claim otherwise.

List/show/protect/unprotect/delete/prune/recover act only on generated references in this configured
store. Expiration selects terminal, unprotected, sufficiently old runs. Active/interrupted or
unresolved state is never silently expired. Explicit deletion refuses protected records. Removal
validates owned contents and never recursively follows a caller-supplied path or link. Recovery,
retention and unknown-version downgrade checks are part of acceptance, not optional cleanup.
Listing, recovery and retention apply the same collection payload and receipt consistency checks
as individual reads. Bulk mutation preflights its candidates before writing. Management checks
signals between bounded filesystem steps; cancellation retains deletion journals and reports
confirmed deleted/recovered references under `completed` instead of admitting another mutation.

## Manifest and comparison

The strict versioned manifest retains bundle/run/producer identities, explicit service/environment
and optional team/deployment, effective connection identity, source/config/CA provenance, installed
pack and query digests, sanitized resolved window, every expected step, original observation times,
errors, coverage, redaction/truncation notices and artifact size/digest/classification. Private
values may carry an explicit redacted marker; missing identity cannot be reconstructed by hashing
its secret original. Hashes prove byte integrity, not truth or authorship.

`evidence.compare` reads two generated references and a named normalization policy with no network,
command or credential access. `exact-window-v1` requires identical windows. `window-offset-v1`
requires equal durations and compatible sampling; preserve original times and compare offsets.
Check service/environment, supported schema, effective origin/org/datasource, installed query
identity/digest, typed parameters, units, windows and source-data coverage before computing changes.
Unrelated config/source revision changes are disclosed instead of silently erased.

For recognized Prometheus frames, compare canonical label sets and aligned finite samples; expose
added/removed series and changed/missing samples. Do not interpolate or treat null/missing as zero.
For recognized Loki frames, compare bounded returned entries while preserving duplicate counts and
the named timestamp policy. These are returned-entry differences, not total log-volume or rate
claims. Unknown shapes, redacted identities, incompatible units/sampling and missing checks produce
explicit incompatible/missing outcomes. Partial input cannot produce a complete aggregate claim.
Retain both source references and algorithm version; differences do not establish causation.

### Comparison interface and projections

```sh
save --json --state-dir /absolute/private/store evidence compare BEFORE_RUN AFTER_RUN \
  --normalization exact-window-v1
```

The structured operation accepts exactly `before`, `after` and `normalization`. Each reference
contains `id` and optionally `expected_manifest_sha256`; the direct CLI exposes the pins as
`--before-manifest-sha256` and `--after-manifest-sha256`. Both references are verified under one
store lock and deadline. Comparison never initializes, repairs or changes a store, resolves
current context, loads Grafana configuration, reads credentials or replays a query. Hosted evidence
grants remain unavailable. It accepts `record=never` only.

Algorithm `returned-observations-v1` returns both immutable bundle references, their original
windows and observation times, a compatibility result, source limitations, changed provenance
field paths, and separate `metrics` and `logs` checks. Equality concerns only the named projections
below. Config, CA and context-source revisions can differ; those differences remain disclosed.
Service/environment and selected team/deployment, effective origin/organization, installed query
identity, typed parameters, datasource and effective request sampling must agree. Retained compiled
requests are checked against the installed query compiler. Redacted essential identity cannot be
reconstructed. A missing or unusable receipt stays a missing check.

Global identity/window incompatibility produces `state=incompatible`; incompatible or missing
individual checks produce `state=partial`. Interrupted bundles, incomplete capture, missing or
nonfinite samples, warning notices and omitted difference details also prevent a complete result.
Complete comparison means all retained supported observations were compared; the receipt still
reports `coverage.state=not_established`. Empty returned log sets can be compared; empty metric
observations remain partial. Execution exits 0 for complete comparison, 1 for partial/incompatible
comparison or store failure, and 2 for invalid/unsupported requests. Health stays `not_assessed`.

`exact-window-v1` compares absolute nanosecond timestamps. `window-offset-v1` subtracts each original
window start, requires equal durations and retains the absolute source timestamps alongside every
sample/entry difference. No interpolation, padding, implicit time shift, zero filling, tolerance or
rate estimation occurs.

- **Prometheus:** Recognize wide/multi numeric time-series frames, including the legacy untyped
  form and dataplane type version `[0,1]`. A series is its numeric field name and sorted string
  labels. Compare aligned finite values exactly as represented in retained JSON; preserve null
  and SDK `NaN`/`Inf`/`NegInf` entities as unavailable observations. Numeric field units must be
  absent or dimensionless (`""`, `none`, `dimensionless`). Time-field intervals and
  `calculatedMinStep` must agree when present, and each matching series must retain its sampling
  interval. Declared intervals can exceed the requested interval. Duplicate identities/timestamps,
  shifted disjoint sample grids and inconsistent sampling are refused; gaps remain explicit.
- **Loki:** Recognize legacy `labels`/`Time`/`Line`/`tsNs`/`id` and dataplane
  `labels`/`timestamp`/`body`/`id` frames, with optional `labelTypes`. Versioned log profiles use
  `[0,0]`. Simple time/body frames are accepted when unambiguous. Compare multisets of exact
  timestamp, canonical labels, supported label classifications and body, preserving duplicate
  counts. Generated IDs are excluded. `label_sets` counts projected label sets, not indexed Loki
  streams. Returned-entry differences never imply total volume or rates.

Epoch millisecond values and SDK `data.nanos` retain integer nanosecond precision. Legacy string
`tsNs` must agree with the millisecond timestamp and any supplied nanos. Unsupported long/table,
histogram, exemplar or unknown semantic profiles are explicit incompatible checks. Display names,
colors, links, frame presentation, generated log IDs and transport execution metadata do not enter
observation identity. Warning/error notices and truncation remain limitations rather than being
normalized away.

Per side, projection bounds are 512 frames/series or log label sets, 128 fields per frame and
10,000 observations. Label sets allow 64 entries, 256-byte keys, 2,048-byte values and 16 KiB
aggregate; field names allow 256 bytes and log bodies 16 KiB. Oversized or ambiguous inputs are
refused rather than silently sampled. Checks poll the one deadline, signal and result consumer
between bounded decoding/comparison steps. As with store reads, synchronous kernel I/O cannot be
forcibly preempted.

`--max-items` (structured `limits.max_items`) defaults to 100 and allows 1–1,000 emitted difference
details across both checks. `--max-output-bytes` bounds their combined encoded arrays, with an
effective ceiling of one MiB and the envelope's 1,024-byte minimum. Whole details form a stable
prefix; full bounded-input difference counts and `omitted_details` remain visible. Fixed bounded
metadata is additional to that detail budget, while the canonical receipt remains under two MiB.
`--timeout-ms` defaults to 30,000. Truncating details never claims a complete aggregate.

The projections follow the [Grafana time-series contract](https://grafana.com/developers/dataplane/timeseries),
[SDK JSON serializer](https://github.com/grafana/grafana-plugin-sdk-go/blob/v0.296.2/data/frame_json.go),
[Prometheus matrix fixture](https://github.com/grafana/grafana-prometheus-datasource/blob/cb5651b00a6b7769b10067f1a6baec07278f4c52/pkg/promlib/converter/testdata/prom-matrix-frame.jsonc),
and Loki [legacy](https://github.com/grafana/grafana-loki-datasource/blob/6d076df3d695f5fda342fbbad1f52547b55a96e2/pkg/loki/test_non_dataplane/streams_simple.golden.jsonc)
and [dataplane fixtures](https://github.com/grafana/grafana-loki-datasource/blob/6d076df3d695f5fda342fbbad1f52547b55a96e2/pkg/loki/testdata_dataplane/streams_simple.golden.jsonc).
These pinned upstream examples establish format evidence, not the home lab's installed plugin
versions or live compatibility.

## Acceptance

Use private HTTPS fixtures and the ordinary installed binary. Verify real pack dispatch, exact
common windows, child timing/deadline/cancellation, direct/structured parity and refusal before
HTTP for every binding/digest/input mismatch. Validate canonical receipts and strict manifests.
Exercise credential canaries through private metadata, checkpoints and stored objects.

Storage checks cover traversal, symlink/nonregular/ownership/mode refusal, lock contention, quotas,
corrupt/oversized metadata, changed artifact bytes, failures during write/publish/checkpoint/seal,
interrupted restart without replay, protected retention and unsupported-version preservation.
Comparison checks cover changed/equal/added/removed/missing observations, mismatched identities,
units/windows/sampling, empty/null/partial data and zero external effects. Review affected authority,
filesystem and lifecycle code independently, then verify the frozen installed mission.

The owner's home lab is the selected live pilot environment. Its authoritative inventory/configuration
will provide real context bindings; neither fixtures nor a successful local store test establish
live Grafana compatibility, native Windows support, a hosted grant or release readiness.
