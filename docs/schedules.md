# Persistent observational schedules

WORKBENCH-025 has Linux x86-64 isolated-process acceptance under CAP-21/AC-31 and the
[scheduling specification](sre-workbench/advanced-execution.md#scheduling).
The scheduler builds on the accepted [fleet controller](fleet.md). Its first consumer is the
installed error-budget calculator over supplied measurements. Scheduling that calculation does
not create a live telemetry source or establish target health.

## Ownership and occurrences

A private operator-owned store retains versioned definitions and occurrence history. A definition
binds a stable schedule ID, generation, enabled state, exact task inputs and target selection,
timezone/cadence, overlap/missed-run policy, execution deadline and retention bounds. Owner identity
comes from the store's OS ownership, not a request-supplied label. The scheduler runs only through
an explicitly invoked foreground entrypoint; creating a definition installs no service.

Each occurrence binds schedule ID, generation and due instant. An explicit manual run also needs
a stable caller-known request ID. Persist intent, the exact fleet request and the original fleet
store identity before dispatch. A lifetime local lease controls admission; expiration of wall time
never transfers ownership. After a lost reply or scheduler crash, query or reconcile that same
fleet identity. A missing fleet result does not authorize another collection: the first attempt
may already have caused remote work.

Freeze credential-free profile identities and the configured target selection at definition
creation/update. Fleet admission must compare them against the exact inventory/profile bytes it
consumes, including its duplicate-request path. Reopening files after a separate check would not
preserve this boundary. A changed credential, principal, target or store cannot silently widen an
existing schedule. Private keys and certificate payloads do not enter schedule history.

## Pause, overlap and recovery

Pause and update serialize with occurrence-intent publication. An intent committed before pause
may already dispatch; pause prevents subsequent admissions without silently cancelling that work.
Cancel-active is a separate operation. An update increments generation and records the disposition
of old queued work. Unresolved older-generation occurrences still block overlap.

Default overlap and missed-run policies skip with durable reasons. There is no catch-up burst.
Retained safety state includes unresolved occurrences, manual-request deduplication identities and
cadence watermarks. History retention cannot erase the information needed to prevent replay.
Quota exhaustion is an explicit refusal, including the independent fleet-store capacity limit.
Scheduler shutdown stops new admission; cancellation of already admitted work requires its own
request and receipt.

## CLI and bounded policy

Attach `--schedule-state-dir DIRECTORY`, `--fleet-state-dir DIRECTORY` and, for definition
creation/update, `--fleet-inventory FILE`. Definition JSON contains exactly `id` and `definition`;
an update also supplies `expected_revision`. The definition fields are `enabled`, `timezone`,
`cadence`, `collection`, `misfire_grace_ms` and `retention_limit`. Collection supplies the existing
fleet `targets`, `task`, `concurrency` and `failure_threshold`, plus its `timeout_ms` ceiling.
See the [interval example](sre-workbench/examples/input-schedule-create.json) and
[daily example](sre-workbench/examples/input-schedule-daily.json).

```sh
save --json --schedule-state-dir /absolute/schedules --fleet-state-dir /absolute/fleet \
  --fleet-inventory /absolute/inventory.json --record always \
  schedule create --input /absolute/definition.json
save --json --schedule-state-dir /absolute/schedules schedule preview budget.hourly --count 8
save --json --schedule-state-dir /absolute/schedules --record always \
  schedule run-now budget.hourly --expected-revision 1 --request-id manual-check-001
save --json --schedule-state-dir /absolute/schedules --fleet-state-dir /absolute/fleet \
  --record always schedule work --max-occurrences 1 --timeout-ms 30000
```

`run-now` queues an explicit manual occurrence; it is allowed while paused, respects non-overlap,
and preserves cadence. Repeating the same manual request returns the original occurrence even
after a definition edit; it cannot create a new-generation run under that old ID. The explicit
`work` command makes one bounded processing pass and does not install or run a daemon. Generic
structured `schedule.work` calls cannot enter that operator worker path.

A worker pass completing does not mean every child succeeded: terminal fleet outcomes remain in
`fleet_aggregate`. Fresh refusal that proves no admission resolves the occurrence as `failed`;
uncertain intent remains `unknown`. A new explicit request may follow a definite failure, while
repeating the original manual request never dispatches it again.

`status ID`, `list`, `preview ID --count N`, `pause ID --expected-revision N` and
`cancel-active ID --expected-revision N` complete the management commands. Reads accept
`record=never`; writes require `auto|always`. Every operation has a foreground timeout of
1..300000 ms and a bounded output budget. Full child receipts can be omitted with their digests
retained; omitted resolved occurrence rows are counted explicitly.

At most 16 schedules and 128 retained occurrences per schedule fit in the 64 MiB private ledger.
`retention_limit` is a hard ledger cap, 1..128, not a rolling expiry policy. There is no automatic
history deletion, and the independent fleet store still caps retained parents at 32. A worker pass
processes at most 16 occurrences. Fixed overlap/misfire policy is `skip`; a 1..60000 ms lateness
window admits ordinary polling delays. Intervals are 60..86400 seconds from an explicit UTC anchor;
daily cadence is a local `HH:MM`. Preview returns at most 32 rows.

## Clock and timezone rules

Intervals use a fixed UTC anchor; they do not drift with completion time. Daily schedules use an
explicit IANA timezone and local clock time. Preview exposes both local and UTC values. A local
time skipped by a DST transition becomes a recorded skip; a repeated time selects the earlier
instant once. Backward clock changes cannot rewind the persisted cursor. Forward jumps must
produce bounded skip accounting rather than enumerate an unbounded backlog.

The implementation dependencies are pinned to Chrono 0.4.45 and Chrono-TZ 0.10.4, with embedded
IANA rules **2025b**. This is the declared rule set, not a claim that it contains every later
political timezone change. The generation records the rule version; changing rules requires an
explicit definition update. No host timezone database or `TZ` environment value selects authority.
Chrono-TZ provides compile-time timezone implementations, and Chrono exposes ambiguous and missing
local-time results so the schedule policy can handle each case explicitly.
See the [Chrono-TZ documentation](https://docs.rs/chrono-tz/0.10.4/chrono_tz/) and
[Chrono local-time mapping](https://docs.rs/chrono/latest/chrono/offset/enum.LocalResult.html).

## Verification and remaining scope

[Retained acceptance](schedules-verification.md) combines injected-clock and store regressions with
76 installed commands against real disposable runners. It covers duplicate manual delivery,
concurrent workers, scheduler loss, active cancellation, graceful worker shutdown, credential/store
changes and real private disk exhaustion. Source and ordinary binary hashes bind that evidence. Live deployment, notifications, native Windows and
controlled changes retain separate authorization and acceptance requirements.
