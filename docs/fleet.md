# Bounded multiple-target collection

WORKBENCH-024 has Linux x86-64 isolated-process acceptance under CAP-20/AC-30 and the
[multiple-target specification](sre-workbench/advanced-execution.md#multiple-target-runs).
The controller uses the accepted [remote runner](runner.md), with its independent target-side
policy and durable request identity. The first consumer is the installed error-budget calculator:
it processes supplied measurements and does not collect live telemetry. Additional observational
consumers retain their own binding and acceptance requirements.

## Explicit selection and authority

Attach a private `--fleet-inventory FILE` and `--fleet-state-dir DIRECTORY`. Inventory version 1
contains a bounded revision and at most 16 entries, each with a distinct inventory `id`, `runner_id`,
remote `target`, private absolute `runner_config` reference and expected authenticated `principal`.
There is no ambient inventory discovery, selector expansion or default credential forwarding.

Collection inputs contain exactly `request_id`, `targets`, `task`, `concurrency` and
`failure_threshold`. The stable request ID is known before admission and is the lookup handle after
lost output. `targets` selects distinct inventory IDs; all selected targets remain represented in
the result. An empty selection is an explicit refusal, distinct from unreadable or invalid inventory.
The task is version-1 `error-budget` with its existing typed numerical input. Concurrency is 1..4;
the absolute failure threshold is 1..16.

Resolve all selected profiles and prepare their TLS configuration from the exact consumed bytes.
Capture a credential-free identity snapshot: profile, CA and client-certificate digests plus endpoint,
server name, runner identity and server leaf pin. Use those prepared clients throughout the run.
A separate file check followed by reopening an ordinary runner call would not preserve authority.
Private key and certificate payloads never enter the parent store.

Before child submission, authenticate selected runners, match the expected principal and installed
reviewed task binding, and capture their policy digests. An unreachable selected runner remains a
visible preflight failure. Freeze the complete target set and its preflight observations before the
first submission. Child submit includes a server-enforced `expected` principal and policy digest;
these assertions only narrow the existing grants. Status and cancel retain the expected principal
while allowing policy changes that revoke execution without removing historical ownership.
An old runner that rejects this optional protocol field remains unavailable to this fleet profile.

## CLI and result meaning

```sh
save --json --fleet-inventory /absolute/inventory.json --fleet-state-dir /absolute/fleet \
  --record always fleet collect --input /absolute/collection.json --timeout-ms 30000
save --json --fleet-state-dir /absolute/fleet fleet status COLLECTION_REQUEST_ID
save --json --fleet-state-dir /absolute/fleet --record always fleet resume COLLECTION_REQUEST_ID
save --json --fleet-state-dir /absolute/fleet --record always fleet cancel COLLECTION_REQUEST_ID
```

All four commands accept `--timeout-ms` (1..300000) and `--max-output-bytes`. Structured operations
are `fleet.collect`, `fleet.status`, `fleet.resume` and `fleet.cancel`. Lookup operations take
`{"request_id":"..."}`. Write operations require `record=auto|always`; status accepts `never`.
Hosted adapters do not acquire these operator startup profiles or stores.

The parent includes inventory/selection identities, a separate generated run ID, creation and expiry,
concurrency, threshold, cancellation intent and stop reason. Every target reports its original
request identity, execution phase, outcome/reason, remote job/attempt, receipt digest and evidence
reference. Optional receipt detail may be omitted to meet the output budget; target identity and
outcome are never silently dropped.

A successful runner management RPC does not establish successful child execution. Classify each
actual child result as succeeded or failed; uncertain admission/execution remains unknown. Skipped
means no submission intent was ever dispatched. Only all-success can produce aggregate success.
Mixed success is partial; no success with unresolved children is unknown; entirely resolved outcomes
with no success are failed. The aggregate retains unknown effects whenever a child is unresolved.
The calculator's budget/burn verdict is not a health assessment of the fleet.

Collect and resume use the collection aggregate for their execution status. Status and cancel
report whether the management read or cancellation intent succeeded; their explicit `aggregate`
and `aggregate_effect_outcome` fields still describe the collection. Omitted detail produces a
partial receipt even when the retained collection itself is complete.

## Persistence, deadline and recovery

Use an owner-bound private store with bounded runs and evidence. Reserve capacity before the first
child admission. Persist the immutable request, complete selection, derived child request IDs,
authority snapshots, limits and original expiry before dispatch. Identical request IDs/bodies return
the existing parent; changed selection, task or limits conflict rather than replacing it.

A sole active controller holds the parent lease; short store locks leave status and cancellation
available. Persist each child's exact admission intent before sending it. Persist the bounded latest
observation and its digest before exposing aggregate progress; terminal child evidence is immutable.
The parent becomes immutable terminal only when every target is resolved. Unknown targets keep the
parent recoverable. Partial writes preserve their bytes and refuse unsafe continuation.

Admission order rotates across repeated collections of the same canonical target set, derived from
persisted admission ordinals. Poll admitted targets fairly rather than waiting for one child before
observing another. Failure thresholds count failed or unresolved observations and stop subsequent
admissions when observed; children already admitted under the concurrency limit remain accounted for.

The original controller deadline bounds preflight, new admission and observation. Child runtime is
bounded by the remaining controller budget and runner policy when the exact intent is formed.
Remote runtime starts at remote dispatch, so queueing or a partition can leave work unresolved after
the controller deadline. Stop new admission and use one bounded cleanup window, at most 2,500 ms,
for cancellation/reconciliation. Report unknown when termination cannot be established. The
controller never extends a remote job's runtime or infers completion from elapsed time.

After controller loss, resume queries or cancels the original remote request IDs. Previously
unintended targets become skipped; no target is silently resubmitted. Even `not_found` after an
uncertain send does not authorize a repeat. Reopen original profile references only if their consumed
identities match the stored snapshots; profile or principal drift leaves that target unresolved.
An authenticated principal/policy precondition refusal stops polling that target for the current
call and preserves its receipt. A later explicit recovery call may check the original identity
again. Current inventory changes cannot alter an admitted selection. Explicit cancellation records intent
for an active controller, or performs bounded reconciliation when it can own the inactive parent.

## Verification and limits

[Retained acceptance](fleet-verification.md) uses two real runner processes and disposable synthetic
identities inside `tools/test-sandbox.sh`. It covers mixed outcomes, unreachable targets, selection
refusals, immutable inventory, deduplication, threshold rotation, bounded output, controller SIGKILL,
profile/principal remapping, cancellation and real private disk exhaustion. Focused controller and
store regressions cover the no-submit recovery protocol, cancellation before admission and receipt
integrity. The first consumer remains a calculator over supplied measurements. Live observations,
live fleet deployment, persistent scheduling, native Windows and controlled changes retain their
own acceptance requirements.
