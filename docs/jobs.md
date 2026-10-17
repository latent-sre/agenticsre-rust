# Durable local jobs

This increment implements the local portion of [CAP-18](sre-workbench/capabilities.md#jobs)
and the crash/reconnect experiment in AC-22. The Linux scope has
[retained acceptance evidence](jobs-verification.md); the live record is
[WORKBENCH-022](roadmap.md#workbench-022--durable-local-jobs).

## Execution and authority

Submission persists a bounded operation request and returns a durable handle. It does not start
a background service or establish that the requested operation succeeded. The operator explicitly
starts a local worker against the same private store. Submission, cancellation and worker writes
require explicit recording consent; status and event reads do not.

The first consumers are native process execution, installed tasks and separately authorized
external capabilities. The worker uses the existing operation admission and process supervisor.
An extension job binds its exact installed package and policy identities at submission and checks
them again before dispatch. Queue persistence cannot supply a missing grant. Hosted adapters,
remote runners and scheduling retain their separate authorization contracts.

Native jobs retain the canonical executable and working-directory paths and the installed
supervisor identity. They preserve `process.exec`'s ordinary path semantics; they do not freeze
the executable file's bytes or provide a protected installation. Task jobs bind the installed
adapter implementation, while model paths still refer to the bytes consumed when execution starts.
The task receipt identifies those consumed bytes. Extension jobs enforce their admitted package
and policy digests inside the actual selection that supplies the executable snapshot.

The local owner is the operating-system account, not the supplied correlation ID. Store permissions
protect against other accounts; they do not create a protected boundary against programs running
as the same account. Requests and results may contain sensitive operator-supplied values. An
explicit job store is a separate persistence decision from evidence collection and ordinary
foreground execution with `record=never`.

Job runtime is distinct from the short admission deadline. Its host ceiling is one hour; the
ordinary foreground request ceiling remains 300 seconds. The actual child receipt must disclose
the runtime used. Cleanup retains the existing bounded TERM/KILL and pipe-drain behavior.

## Operator interface

`--job-state-dir` selects an explicit absolute private store. The optional startup option
`--job-max-runtime-ms` lowers the host ceiling; it cannot exceed 3,600,000 ms. Neither setting is
accepted from operation inputs or inherited by a hosted adapter.

```sh
save --json --job-state-dir /absolute/private/jobs --record always \
  job submit --request /absolute/request.json --max-runtime-ms 600000 --retain-events 32
save --json --job-state-dir /absolute/private/jobs --record always job work JOB_ID
save --json --job-state-dir /absolute/private/jobs job status JOB_ID
save --json --job-state-dir /absolute/private/jobs job watch JOB_ID --after-sequence 0
save --json --job-state-dir /absolute/private/jobs --record always job cancel JOB_ID
```

The submitted file is an ordinary version-0.1 request with `record=never` and a foreground timeout
at most 300,000 ms. `max_runtime_ms` separately declares the job's execution allowance, defaulting
to 30,000 ms in the CLI. `retain_events` is 1–256, defaulting to 32. All three submit input fields
are required in a structured call. The nested request ID is the admission deduplication key;
reuse that ID when retrying a submission whose reply was lost.
The [native request example](../examples/jobs/process.json) prints a fixed literal string and
uses `/tmp` as its explicit working directory. Give a new intended execution a new request ID.

`job.watch` returns one bounded page of at most 64 events; it does not hold a streaming connection.
Continue from `cursor.next_sequence` while `has_more` is true, then poll that cursor to reconnect.
The cursor explicitly describes any unavailable interval. `job.status` separately returns the
committed child result. If the requested response budget cannot include the whole child receipt,
`result_omitted` is true and `result_sha256` retains its identity; retry the lookup with a larger
output budget through a structured request.

The dedicated `job work` command calls the operator worker entry. A generic `call` for `job.work`
is refused, and submission never spawns that worker implicitly. The worker handles exactly the
selected job and exits. Its result consumer may disconnect without cancelling durable work.
Signals to the worker and explicit `job.cancel` requests follow the cancellation lifecycle.

The public data separates the management operation's result from the child result: a successful
status lookup is not proof of successful child execution. Inspect `job.state`, `job.execution_status`
and the committed `result`. `stored_state` identifies the durable state before a read-only lookup
observes that an active worker's ownership is gone; reconciliation can then durably record the
recovery-required observation without dispatching again.

## Persistence and recovery invariants

The admitted request fingerprint is immutable. Repeating the same owner/request ID with the same
fingerprint returns the existing handle; a different fingerprint is a conflict. Returning a stored
result after lost delivery never executes the request again. Admission deduplication makes no
claim of exactly-once external effects.

The lifecycle distinguishes queued, running, cancelling, finalizing, terminal and recovery-required
states. Record dispatch intent durably before starting the operation. Keep cancellation intent
separate from observed completion; a request to cancel is not evidence that a child stopped.
Once committed, a terminal outcome is immutable, including when cancellation arrives later.

An interrupted dispatch requires reconciliation. A persisted PID, an expired timestamp or a
released worker lock cannot authorize killing or adopting another process, prove success, or
justify automatic replay. When the prior outcome cannot be established, retain an explicit
recovery-required state and unknown effects. Preserve partial or corrupt writes for diagnosis;
never silently repair them into a successful job.

Events have monotonically increasing sequence numbers per job. A reconnect supplies the last
observed sequence and receives later retained events. Bounded retention discloses a gap and current
state when older events are gone. Terminal-result lookup remains independent of event delivery.

The initial store admits at most 64 jobs and 128 MiB of files, including space for staged writes.
Each child receipt is at most 1 MiB; the complete management response remains below 2 MiB.
Private directories use mode 0700 and files use mode 0600. Directory-relative no-follow access,
single-link checks, strict layouts and bounded duplicate-key-aware JSON parsing refuse unsafe
or corrupt storage. Atomic publication synchronizes file contents and directory entries.

There is no implicit deletion, eviction or automatic repair. Keeping admission records preserves
deduplication after terminal completion. A full or recovery-required store refuses further work
and retains the evidence needed for an explicit operator decision. Automated retention, migrations
and target-specific reconciliation remain separate follow-up work.

## Required verification

Acceptance must exercise real children through the public CLI in the existing isolated sandbox:
duplicate admission without redispatch, native and extension jobs, reconnect after an event cursor,
worker contention, cancellation before dispatch and during execution/finalization, client output
loss, worker crash/restart, interrupted persistence, capacity limits and explicit retention gaps.
It must also show that changed extension authority is refused before dispatch, malformed store
entries are preserved, and ordinary foreground limits remain intact.

Independent lifecycle/storage review and installed acceptance against frozen source and executable
bytes are required before this increment is accepted. Linux evidence does not establish Windows
support, remote ownership, target reconciliation or fleet execution.
