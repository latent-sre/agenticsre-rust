# Opt-in toil metadata

`toil` records workflow episodes in a separate, private local store. Collection is off until an
operator explicitly enables it. There is no background telemetry, employee field, command/query
content, or automatic capture of unrelated operations. Workflow names are bounded identifiers;
use a workflow label rather than a person or sensitive value. Linux core/CLI is the initial scope.
UI/MCP grants and native Windows are separate requirements.

Configure the store explicitly; every mutation requires `--record auto` or `--record always`:

```sh
save --json --record always --toil-state-dir /absolute/private/toil toil configure --enabled true --keep-days 30
save --json --record always --toil-state-dir /absolute/private/toil toil record --input episode.json
save --json --record always --toil-state-dir /absolute/private/toil toil capture --input capture.json
save --json --toil-state-dir /absolute/private/toil toil summary --input summary.json
save --json --record always --toil-state-dir /absolute/private/toil toil configure --enabled false --keep-days 30
```

The global evidence `--retention-days` does not configure toil; `--keep-days` maps to the typed
`toil.configure` input's `retention_days`. All seven operations also accept the standard structured
request envelope through `call`. They use a local target and operation version 1.

An [episode input](sre-workbench/examples/input-toil-record.json) names `workflow`, `cohort`
(`baseline` or `candidate`), UTC `observed_at`, `measurement` (`measured` or `estimated`) and
`outcome`. Optional `elapsed_ms`, `effort_ms`, `retries` and `manual_interventions` remain unknown
when absent; explicit null is invalid. Durations are integer milliseconds up to 365 days; counters
are at most one million. Observations cannot be future-dated or already expired. UTC timestamps
use `YYYY-MM-DDTHH:MM:SS[.fraction]Z`, up to nine fractional digits and years from 1970.

An episode represents one complete attempt sequence. Its duration/effort includes any retries;
retry counts are descriptive and never multiply effort. Failed and partial episodes remain in the
comparison. Operator-reported measurements are declarations, not independent observation.

[`toil.capture`](sre-workbench/examples/input-toil-capture.json) runs one installed version-1
`dashboard-hygiene` or `error-budget` task through the existing supervisor. Consent, input, lock
and quota checks precede execution. It retains elapsed time, outcome, task identity and run ID;
task inputs, paths, stdout/stderr and calculation/detail data are returned in the child receipt
but never persisted in the toil store. Capture does not infer human effort from runtime. It records
zero retries because the supervisor does not retry; manual interventions remain unknown.

Use `toil annotate --input annotation.json` with `id`, `effort_ms`, `measurement` and optional
`retries`/`manual_interventions` to supply human effort for an existing episode without adding a
second sample. The sample retains its original runtime provenance and records the annotation time.
Annotation is an explicit correction, not an immutable audit log. A captured task may finish even
if recording subsequently fails or its deadline expires. The parent then preserves the child
execution/effect receipt and explicitly reports recording failure. Oversized child data is omitted
with `child_payload_omitted: true`; child execution identity and recording metadata remain visible.

## Summaries and arithmetic

A [summary input](sre-workbench/examples/input-toil-summary.json) selects one workflow and an
explicit half-open `[from,to)` UTC window. The window must be positive, at most 3650 days and a
whole number of milliseconds. It supplies build, maintenance and incident costs in milliseconds,
including explicit zero where appropriate, and a measured/estimated label for those declared costs.

Each cohort reports sample and outcome counts, retained episodes per day over the requested
window, known/missing/measured/estimated duration counts, means, and retries/manual interventions.
Only known values contribute to descriptive means. No unknown field becomes zero. At least one
episode and known effort for every selected episode in both cohorts are required for savings:

- Gross per run = baseline mean human effort − candidate mean human effort.
- Observed gross = gross per run × candidate sample count.
- Observed net = observed gross − build − maintenance − incident cost.
- With positive gross, break-even runs = ceiling(total costs / gross per run), using exact integer
  rational arithmetic. Otherwise the threshold is unavailable, with a reason.
- Optional projection uses declared `runs_per_day` × `horizon_days`, subtracts the same costs once,
  and always carries an estimated label.

These are counterfactual comparisons, even when all inputs are measured. They establish neither
causation nor productivity benefit. Self-selection, unequal work, small samples (fewer than five
in either cohort), missing data, estimated inputs and retention effects appear in limitations.
Missing effort yields null savings, not an error or an apparent zero-cost success. Failed attempts
count too. Frequency describes retained observations rather than an external arrival rate.

## Consent, retention and storage

Disabling collection blocks record, capture and annotate before any task starts. It preserves
history for summary, delete and prune. `toil delete SAMPLE_ID` removes the named row; `toil prune`
removes rows older than the stored retention interval. Retention is based on observation time;
summaries exclude expired samples without modifying the store. There is no implicit eviction or
background deletion. Reducing retention can exclude rows immediately; run prune for physical
removal. These local deletions do not erase external backups or prior returned receipts.

The owner-only namespace uses no-follow filesystem access, a process lock and atomic publication,
with a maximum of 1024 samples and 4 MiB. Full, corrupt, unknown-version or unexpected layouts are
refused without silently discarding history. Keep toil and immutable evidence/runbook directories
separate. Same-account writers can change local metadata; this store is not a tamper-proof ledger.
Ordinary task execution keeps its existing authority and restrictions.

Schemas: [configure](sre-workbench/schemas/toil-configure-input.schema.json),
[record](sre-workbench/schemas/toil-record-input.schema.json),
[capture](sre-workbench/schemas/toil-capture-input.schema.json),
[annotate](sre-workbench/schemas/toil-annotate-input.schema.json),
[delete](sre-workbench/schemas/toil-delete-input.schema.json),
[summary](sre-workbench/schemas/toil-summary-input.schema.json),
[sample](sre-workbench/schemas/toil-sample.schema.json) and
[operation data](sre-workbench/schemas/toil-data.schema.json). Prune requires an empty input object.
See [the roadmap](roadmap.md) for acceptance status and remaining platform scope.
