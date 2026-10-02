# Error-budget task — implementation contract

This second fixed task follows the [dashboard task](tasks.md) and uses the pinned
[upstream adapter baseline](adapter-notes.md). Current work and acceptance status belong in
[the roadmap](roadmap.md). It performs arithmetic on supplied measurements, without collecting
telemetry, contacting a target or sending an alert.

## Interface and inputs

`save [--json] task run error-budget --input INPUT.json` invokes task version 1 through the same
core as a structured `task.run` request with
`{"id":"error-budget","version":1,"input":{"slo":99.9,"bad_minutes":40.32}}`.
The CLI input file contains just the inner input object. Discovery includes both installed tasks.
The existing timeout and output limits apply; `--fail-on-findings` is rejected for this calculator
before dispatch because its results contain calculations, not dashboard findings.

Inputs retain the upstream names:

| Field | Contract |
|---|---|
| `slo` | Required finite percentage, strictly between 0 and 100 |
| `window_days` | Positive finite status horizon; default 28 |
| `bad_minutes` | Nonnegative finite bad minutes; exclusive with request measurements |
| `bad_events`, `total_events` | Both or neither; finite, bad >= 0, total > 0 and bad <= total |
| `sli_long`, `sli_short` | Finite percentages from 0 through 100; short requires long |
| `long_window`, `short_window` | Bound pairs only: `1h`/`5m` (default), `6h`/`30m`, `3d`/`6h` |

Unknown keys, explicit null, booleans in numeric fields and invalid combinations are refused.
Preserve upstream fractional event inputs rather than silently rounding them. A request with only
the SLO computes its allowed bad fraction; it cannot establish budget consumption or burn severity.
Reject arithmetic overflow, underflow that makes a required divisor zero, and any nonfinite result.
Every emitted number must be valid finite JSON.
Normalized binary64 inputs must retain their values across the structured child-protocol roundtrip.

## Calculation semantics

Allowed bad fraction is `1 - slo/100`. The time budget is `window_days * 24 * 60` times that
fraction, in minutes. The request budget is `total_events` times that fraction, in events.
Consumption never changes units. Remaining budget is budget minus consumption; request status
also reports observed availability `(1 - bad_events/total_events) * 100`.

Use the upstream near-zero tolerance `max(abs(budget) * 1e-12, 1e-12)`: remaining values within it
are `exhausted` and display positive zero. Otherwise negative remaining is `over_budget`, and
positive remaining is `ok`. This task does not infer live service health from these states.

Burn rate is `(1 - sli/100) / allowed_bad_fraction`. Both measurements must reach the selected
threshold: 14.4 for `1h`/`5m` produces `page`, 6 for `6h`/`30m` produces `page`, and 1 for
`3d`/`6h` produces `ticket`. These are fixed 30-day example policy thresholds. `window_days`
changes budget status and projection; it does not rescale those alert thresholds.

Correct one upstream rounding edge: at SLO 99 and SLI 85.6 the rate is mathematically 14.4,
but binary64 arithmetic produces `14.39999999999999`. Decide threshold crossings from decimal
forms of normalized inputs, comparing `(100 - sli)` to `threshold * (100 - slo)`, so an exact
boundary is included without treating a genuinely lower rate as crossed. Reported arithmetic
otherwise retains binary64 behavior and its documented near-zero budget tolerance.

One measurement produces `not_evaluated` severity. Two below-threshold windows, a recovered short
window or an unconfirmed short spike produce `none` with a distinct reason. None of these is an
all-clear or evidence about previously consumed budget. With both windows present and positive
long burn, projection is `window_days / long_rate`, explicitly for the full budget under steady
eligible volume. It is not remaining-budget runway. Zero long burn has no finite exhaustion
projection; a missing short measurement retains the upstream unevaluated projection behavior.

## Structured results and source binding

Successful task data includes normalized `input` with the three defaults filled, `task`, `runtime`,
`supervisor`, `cancellation_signal`, `coverage` and `calculation`:

- `allowed_bad_fraction` is a finite number.
- `status` is null when no consumption is supplied. Otherwise it has `kind` (`time` or `request`),
  `unit` (`minutes` or `events`), `budget`, `consumed`, `remaining`, `consumed_percent`, `state`
  and `observed_availability_percent` (null for time status).
- `burn` is null without a long measurement. Otherwise it has `policy` (`fixed-30-day-example-v1`),
  `policy_horizon_days` (30), `long_window`, `short_window`, `threshold`, `long_rate`, `short_rate`
  (possibly null), `severity`, `reason`, `full_budget_projection_days` (possibly null) and
  `projection_state` (`estimated`, `zero_long_burn` or `not_evaluated`).

Burn reasons are `both_windows_at_threshold`, `short_window_missing`, `short_window_recovered`,
`long_window_unconfirmed` or `both_below_threshold`. Completed arithmetic has execution=succeeded,
effect=not_applicable, assessment=not_assessed and exit 0 even for exhausted budgets or a page
verdict. Operational failures, partial capture, deadlines and cancellation keep their existing
failure semantics and must not become completed calculations.

Extract a pure structured calculation seam; never parse the upstream terminal output. Retain
the original upstream bytes and license as reference. Source identity distinguishes
`upstream_revision`, `upstream_digest`, `adapter_digest` and `binding_digest`; adaptation must
not imply that the original command-line implementation was executed unchanged. Embed the adapter
and wrapper in the binary and reuse the fixed optional Python >=3.11 runtime, `-I -S -B`, minimal
environment, closed stdin and verified process supervisor. Inputs are literal JSON values, never
interpolated Python or shell source. No task/runtime overrides or registration API are introduced.

## Acceptance

Use independent known numerical fixtures as well as the upstream behavior cases: exact exhaustion
and positive zero, time/request units, request availability, all three bound policies, missing
and one-sided windows, horizon independence and full-budget projection. Reject nonfinite and
derived-overflow cases, mixed/missing units, bad ranges, unknown/null/bool fields and mismatched
window pairs. Check CLI/call/text parity, input immutability, metadata/output schemas, missing
runtime, source identity, installed execution, hostile Python environment, output/deadline bounds
and the existing command-runner/dashboard regressions. Cally supplements local evidence with
Python 3.11 and missing-runtime lanes. No live alerting or Windows acceptance follows.
