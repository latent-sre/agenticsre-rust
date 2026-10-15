# Approved local Prometheus rule-file changes

WORKBENCH-026 implements the first CAP-22 consumer: replace one explicitly granted, owned local
Prometheus rule file after separate authenticated approval of its exact plan. Acceptance uses
disposable files in the existing Linux sandbox. Current implementation and verification status
belong in [the roadmap](roadmap.md#workbench-026--approved-local-configuration-changes).
[Retained acceptance](changes-verification.md) records the tested identities and failure paths.

The existing runner transports synchronous `change.plan`, `change.status`, `change.approve`,
`change.apply` and `change.reconcile` requests over pinned mTLS. Requester and approver principals
have separate certificate fingerprints and explicit target grants. A request field, CLI flag or
certificate count does not establish human approval custody. This consumer changes file bytes;
it neither reloads a service nor establishes deployed configuration or current service health.

## Explicit startup authority

The runner server configuration accepts an optional `promtool_path` for the exact reviewed binary
and an optional `changes` object:

```json
{
  "state_dir": "/private/change-ledger",
  "targets": [{
    "id": "rules",
    "path": "/private/rules/alerts.yml",
    "validator_binding_sha256": "sha256:<exact installed binding>",
    "requesters": ["requester"],
    "approvers": ["approver"]
  }]
}
```

These principals must already exist in the runner's authenticated principal map; requester and
approver sets are disjoint for each target. The target is an existing owned, single-linked regular
file with mode 0600 in an owned directory with mode 0700. Symlink path components, substituted
parents, hardlinks and FIFOs are refused. The ledger is separately private and bound to its actual
filesystem identity. Linux x86-64 is the first implemented execution platform.

The controller uses its existing `--runner-config` mTLS profile. No change-specific credential or
role can be supplied through the request. Hosted browser/MCP execution has no change grant.

## Public operations

All five CLI commands use the same form:

```text
save --runner-config PROFILE.json --record always --json change plan --runner RUNNER --input INPUT.json
save --runner-config PROFILE.json --json change status --runner RUNNER --input INPUT.json
save --runner-config PROFILE.json --record always --json change approve --runner RUNNER --input INPUT.json
save --runner-config PROFILE.json --record always --json change apply --runner RUNNER --input INPUT.json
save --runner-config PROFILE.json --record always --json change reconcile --runner RUNNER --input INPUT.json
```

| Operation | Input | Authority and behavior |
|---|---|---|
| `change.plan` | `request_id`, `target`, `proposed_base64`, `expires_in_seconds` | Granted requester validates and retains exact original/proposed bytes without replacing the target |
| `change.status` | `requester`, `request_id` | Authorized requester or target approver reads retained state |
| `change.approve` | `requester`, `request_id`, `plan_sha256` | Distinct granted approver binds approval to the exact plan digest |
| `change.apply` | `request_id`, `plan_sha256`, `approval_id` | Original requester consumes approval once and attempts the exact replacement |
| `change.reconcile` | `requester`, `request_id` | Authorized observation of the original attempt; never repeats replacement |

Wire methods have the `change.` prefix and retain runner protocol 1 and its 64 KiB request frame.
Original and proposed byte sequences are each bounded to 32 KiB. Base64 must use canonical RFC 4648
standard encoding. Expiry is 1–900 seconds. Unknown keys, nulls, unsupported methods and wire role
claims are refused. New plans reject unchanged bytes. Plan, approval, apply and reconcile require
explicit `record=always` because they persist the ledger; status uses `record=never`.

## Exact plan and expiry

The canonical JSON plan includes exact original/proposed base64 and SHA-256, original file and
parent identity, target alias/path, requester certificate, actual server certificate, runner policy,
ledger identity, compiled change implementation identity, validator binding, complete validation
receipt, creation/expiry and readback/recovery rules. The plan digest hashes its canonical serialized
JSON. Approval must select that digest; a new plan needs new approval.

Validation reuses [the pinned Prometheus validator](alert-rules.md), including expected input and
binding digests. Planning and dispatch both require complete valid output, a positive checked-rule
count and no findings. Validator success does not authorize replacement by itself.

In-process monotonic anchors supplement wall-clock expiry. Backward clock observations invalidate
write eligibility. Restart discards monotonic anchors: unconsumed historical plans remain readable
but cannot be approved or applied. A new plan is required. Consumed attempts retain their identities
and can be reconciled after restart without another replacement.

## Replacement and recovery

A stable target-specific lifetime lease serializes cooperating operations. The implementation pins
and rechecks parent and original file identities. It creates replacement bytes exclusively in the
same directory, fsyncs them, records the prepared inode with consumed approval and dispatch intent
in the durable ledger, and only then atomically renames. Readback checks inode, exact digest and
metadata before a verified result. Directory changes caused by the lease/preparation are treated
separately from file version changes.

Repeated delivery of the same apply returns the retained attempt. A lost response or uncertain
publication is investigated through status/reconcile using the original requester and request ID.
Matching content on an unrelated inode cannot establish that the approved replacement occurred.
Private directories and leases coordinate this tool's writers; atomic rename is not compare-and-swap
against arbitrary malicious writers using the same account.

The ledger retains at most 32 plans and 64 MiB with no automatic pruning. Corruption or partial
publication requires recovery; expiry does not grant takeover. Original bytes remain in the bound
plan. Recovery that writes those bytes requires a separately validated plan and distinct approval,
and does not claim to reverse other external effects.

## Result meaning

A retained `verified` state records an identity-checked replacement, mapped to outer
`effect_outcome=succeeded`. `dispatching` or `unknown` must not become successful execution.
`not_applied` records observation of the original inode; consumed approval is still never replayed.
Historical status reports retained evidence rather than a new current-state observation. Reconcile
performs fresh readback. Authentication and response validation bind the returned request, plan and
principal identities before the controller accepts them.

The exact plan can be omitted as a whole when the caller's output detail limit is too small; the
receipt discloses partial coverage and retains its digest. Obtain a complete status response before
reviewing or approving content. Text and JSON are generated from the same receipt.

Required acceptance includes distinct principals, exact approval, stale bytes/identity, expiry and
restart, concurrent apply, symlink/hardlink/path substitution, lost reply/publication, readback
mismatch, partial disk failure and separately approved recovery. Installed acceptance is bound to
frozen source and binary digests. Live deployment, service reload, cross-host conformance, native
Windows and human approval custody remain separate acceptance boundaries.
