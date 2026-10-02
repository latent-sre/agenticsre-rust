# Jobs remote execution and changes

This specification expands CAP-18 through CAP-22 so later phases have concrete behavior to build
and test. Deployment topology, identity provider and persistence implementation remain DEC-11.
No remote service, scheduler or change broker exists as a result of this plan.

## Job model

A job submission contains the normalized operation request plus maximum runtime, retention and
reconnect settings. Admission has its own short request deadline; job runtime is a separate input
bounded by host policy. The foreground envelope's 300-second ceiling must not be reinterpreted as
permission for unlimited background work. Proposed initial maximum job runtime is one hour,
configurable only by the host up to a separately reviewed maximum.

The durable record contains job/run IDs, caller principal, operation/implementation versions,
request fingerprint, resolved target snapshot, creation/admission/dispatch times, execution location,
lease/attempt identifiers, event cursor, cancellation intent, terminal result and unresolved effects.
Correlation IDs are not credentials. The submission fingerprint excludes secrets and records the
selected credential reference/version without persisting the credential value.

| State | Allowed next states | Required evidence |
|---|---|---|
| accepted | queued, rejected, cancelled | Validated request and effective owner |
| queued | running, cancelled, rejected | Resources/authorization still valid at admission |
| running | finalizing, cancelling, recovery_required | Dispatch recorded and executor identity known |
| cancelling | finalizing, recovery_required | Cancellation request plus observed child/target state |
| finalizing | terminal, recovery_required | Validated result and artifact commit |
| recovery_required | finalizing, terminal | Reconciliation observation, never assumption from elapsed time |
| terminal | none | Immutable terminal outcome; a new attempt is a new run |

The accepted response returns a handle and status; it does not claim the requested operation
succeeded. Repeated submit with the same authenticated caller/request ID and identical fingerprint
returns the existing handle. A different fingerprint is a conflict. This deduplicates admission;
it does not guarantee exactly-once effects at an external target.

Event sequence is monotonic per run. A watch request supplies the last observed sequence; retained
events resume after it. If retention has removed events, return an explicit gap and current state.
The client can query a terminal result independently of the event stream.

## Remote dispatch protocol

The runner handshake advertises product/protocol version, installed capability versions, supported
limits and platform identity. Authentication binds the actual caller and runner; tool/role names
are not identities. TLS, credential audience and token validation follow the accepted transport
contract researched at implementation time. No local credential forwarding is a default.

A dispatch record binds job/run ID, attempt ID, request fingerprint, operation implementation digest,
target snapshot, deadline, lease epoch and reply cursor. The runner checks local policy and
resources before acknowledging admission. Acknowledgment lost in transit yields uncertain
admission; the controller queries that exact job/attempt instead of submitting another operation.

Leases limit admission of new work and ownership, but expiration does not prove an in-flight
external effect stopped. Before reclaiming a job, reconcile the old attempt. Where a target
supports fencing/version preconditions, use them; otherwise report that parallel takeover is
unsafe. Observational work may be retried only as an explicitly recorded new attempt under policy.
Live mutations require CAP-22 semantics.

A network partition stops new work that needs fresh authority, preserves local run evidence and
uses the configured cancellation deadline. The runner never grants itself more time because the
controller disappeared. Its reconnection report includes incomplete termination and uncertainty.

## Multiple target runs

Resolve the complete authorized target set before dispatch. Persist target IDs, inventory revision,
selection expression/digest, maximum target count, concurrency, global deadline and failure
threshold. Failed discovery is different from an empty set; neither should produce a misleading
all-healthy result.

Each target gets its own child run and outcome: succeeded, failed, unknown or skipped. Skipped
means never dispatched. An admitted request that times out is not skipped. Parent state is partial
when outcomes differ, failed when none completed successfully, and succeeded only when every
requested operation met its own completion contract. Any unresolved child effect remains visible
in the aggregate effect outcome even when other targets completed.

A changed selector or inventory does not silently alter an active run. A new target set requires a
new plan. On threshold/deadline/cancellation, stop admitting children, then safely finish or cancel
already admitted children and report them. Scheduling order must not permanently starve a target.

## Scheduling

A schedule definition contains stable ID/version, owner, operation request/template, configured
target set or selection policy, IANA timezone, cadence, overlap rule, missed-run rule, concurrency,
run deadline, retention and optional separately authorized notification destination.

The schedule store records revision, enabled/paused state, next due instant, last admission,
run IDs and generation. An occurrence has a stable schedule ID/generation/due-instant key.
Only one valid scheduler lease can admit it; persistence and runner deduplication protect against
repeated delivery. A scheduler cannot claim exactly-once target effects.

Proposed initial behavior is non-overlapping observation jobs, skip missed occurrences and record
the reason. For a nonexistent local time during a DST transition, skip with a reason. For an
ambiguous repeated time, run once at the earlier instant. Display both local time and UTC in the
preview. Later alternatives must be explicit schedule fields and have injected-clock tests.

Pausing prevents future admission but does not silently cancel already running work. Cancel-active
is a distinct request. A schedule edit increments generation and records how queued old-generation
runs are disposed. Expired credentials or denied targets produce a failed occurrence with a safe
diagnostic; no self-repair of credentials or automatic expansion of access occurs.

## Plans approvals and effects

The planned change record contains:

- Plan ID/version and canonical digest, creation/expiry time and owner.
- Exact target identities, resource versions and selection snapshot.
- Operation/version, implementation digest and normalized inputs.
- Preconditions and expected observable effects.
- Concurrency/lease requirements and target-native idempotency support, if any.
- Verification/readback steps and success criteria.
- Rollback or recovery steps, their scope and limitations.

Canonical plan serialization must be defined and tested before signing/hashing; do not assume
different languages serialize numbers, Unicode or key ordering identically. Approval references
resolve to a trusted record covering the exact digest, principal/executor, target scope, expiry,
single-use token and permitted recovery actions. Approval is rechecked at admission and dispatch.

The effect record moves through prepared, authorized, dispatch_recorded, response_observed,
verified and closed. Any interruption after dispatch_recorded may require recovery. Response
success without required readback is not verified success. Readback failure after a successful
write remains a verification gap and does not justify repeating the write.

Reconciliation compares target state and target-native operation IDs/versions with the intended
effect. Outcomes are confirmed_applied, confirmed_not_applied or still_unknown. Only proven
not-applied or target-supported idempotency permits a policy-authorized retry. Compensation is a
new effect with its own authority unless explicitly covered by the accepted plan.

## Failure matrix

| Failure point | Required result | Prohibited shortcut |
|---|---|---|
| Before admission | Denied/failed with not_attempted effect | Claiming a target failure that was never observed |
| Admission reply lost | Query existing occurrence/job ID | New blind submission |
| Child exits before valid adapter result | Failed or recovery_required | Treating exit zero as a valid result |
| API response lost after dispatch | Unknown effect | Assuming timeout means no write |
| Lease expires during target call | Reconcile and stop new admissions | Concurrent takeover without fencing evidence |
| Controller or runner restarts | Recover records and query target/attempt | Replaying all nonterminal rows |
| Result stored but delivery lost | Return stored result on authenticated lookup | Re-executing the operation |
| Rollback fails | Retain actual state and escalate recovery | Marking the original change undone |

AC-22 through AC-25, AC-30 and AC-31 exercise these states with crash points and partitions.
The release record must identify which transitions were observed on the selected target and which
remain unverified. Controlled changes remain unavailable until the complete authority and recovery
path is accepted.
