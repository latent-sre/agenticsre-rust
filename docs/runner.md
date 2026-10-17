# Authenticated remote runner

WORKBENCH-023 has [retained Linux fixture acceptance](runner-verification.md). This contract implements the isolated-process experiment for
CAP-19/AC-23 using the accepted [local job lifecycle](jobs.md) and the
[researched transport profile](runner-transport-notes.md). Live cross-host and native Windows
acceptance remain open requirements. No server is started by reading this configuration or by
using ordinary capability discovery.

## Controller and runner authority

The controller attaches one explicit private `--runner-config` profile. It selects a numeric
endpoint, expected runner ID, TLS server name, CA, server certificate digest and its own certificate
and private key. Those credentials authenticate the controller itself; no ambient environment
credential or local job grant is forwarded.

The operator explicitly starts `runner serve --server-config FILE --record always`. That process
uses only its server configuration. Other local operator startup options are refused. The server
maps verified client certificate digests to configured principal IDs, and maintains a private job
namespace for each principal. Namespace ownership binds the runner and principal to durable
metadata; the backing OS UID is reported separately and is not the remote caller identity.

Grants bind a principal, target snapshot, operation/version, implementation and runtime ceiling.
Server grants are loaded at startup; applying a changed server configuration requires a restart.
The first task grant is the installed error-budget calculator with an exact binding digest and
typed numerical inputs. Extension grants bind an exact package and policy digest plus capability
version; the actual execution selection checks those identities. Native arbitrary commands and
model-file paths are not admitted by this profile. Expanding the supported consumers requires its
own target-side authority and verification contract.

Configured target snapshots contain a name and environment. They are operator-supplied metadata,
not proof of a machine's identity or health. Receipts identify the authenticated runner and principal,
the frozen target, job/run/attempt, remote timestamps and lack of established clock offset.

## Public operations

```sh
save --json --runner-config /absolute/controller.json runner handshake RUNNER_ID
save --json --runner-config /absolute/controller.json --record always \
  runner submit RUNNER_ID --input /absolute/submission.json
save --json --runner-config /absolute/controller.json runner status RUNNER_ID REQUEST_ID
save --json --runner-config /absolute/controller.json \
  runner watch RUNNER_ID REQUEST_ID --after-sequence 0
save --json --runner-config /absolute/controller.json --record always \
  runner cancel RUNNER_ID REQUEST_ID
save --json --record always runner serve --server-config /absolute/server.json
```

The controller's standard request envelope uses target kind `runner` and the explicit runner ID.
The attached profile must match it before a connection is attempted. Submission inputs contain
exactly `request_id`, `target`, `operation`, `operation_version`, `inputs`, `max_runtime_ms` and
`retain_events`. The stable request ID is required before transport; it is the recovery handle,
not an identity credential. Submit and cancel require recording consent. Handshake, status and
watch accept `record=never`. Hosted adapters do not inherit a runner profile or grant.

The fleet increment adds an optional protocol-1 `expected` object containing a required principal
and optional policy digest. The server checks these assertions before durable lookup or admission;
they cannot grant access. Fleet submit sends both assertions, while recovery retains the principal
assertion so revoked execution grants do not prevent historical observation. Controllers prepare
and retain the exact selected TLS material for a collection. Runners that do not support this
optional field reject fleet calls safely; existing calls without it retain their behavior.

Handshake reports the protocol/platform, configured capability grants and limits. These grants
do not establish that a package is still installed; admission and dispatch check the actual binding.
Status and cancel
use the authenticated principal's request ID; watch adds a last-observed event sequence. Lookup of
an existing admission remains possible when its execution grant has been removed. Revocation can
prevent new dispatch while ownership still permits observation and cancellation.

The server emits one bounded readiness JSON object after binding, including its actual socket
address (useful when the configured port is zero), runner ID, protocol and server certificate digest.
This is a long-lived explicit service entry, separate from ordinary one-result operation calls.
SIGINT/SIGTERM stop admission and request cancellation of owned active work before bounded shutdown.

## Durable admission and recovery

Persist the request, authenticated namespace, target/grant/implementation snapshot, request
fingerprint, job/run IDs and admission expiry before acknowledging acceptance. The existing bounded
job store contains the identity binding, so there is no independent request-to-job mapping commit
that could leave accepted work unreachable.

An identical submission returns the original admission. A conflicting body for the same principal
and request ID is refused. A missing admission reply means `unknown`, not failed or unattempted;
the controller never automatically submits again. Query that same request ID. A not-found response
states the runner's observation at that instant and does not prove a delayed earlier request can
never still be admitted.

Admission and lookup return the frozen `binding`: authenticated scope, target and grant digests,
the original expiry and submission digest, and the exact task source or extension package, policy
and executable identity. `management_effect_outcome` describes the lifecycle operation's writes;
the retained child receipt separately describes execution and its effects.

Queued work has a fixed 60-second admission expiry. Dispatch rechecks current authority and the
frozen target and implementation; restart or reconnect never renews the expiry. Admitted execution
retains its bounded runtime, at most one hour. Connection loss does not extend it or grant new work.
There is no renewable distributed lease or automatic takeover claim in this profile.

Runner startup reconciles interrupted states without replaying dispatched work. Only queued work
that is still within its original expiry and currently authorized can start. An interrupted running
attempt remains recovery-required unless a verified published terminal receipt supports adoption.
Cancellation remains a request until the runner observes termination or a committed prior outcome.

## Configuration and limits

Configuration and referenced certificate/key files are private owner-controlled mode-0600 files,
read through bounded no-follow access. The server's state directories are private. Keys use PKCS#8
DER; certificates use DER. Profiles contain absolute file references and no inline credentials.
The server configuration contains at most 16 principals, 16 targets and 64 exact grants; duplicate
identities or grant tuples are refused.
Target snapshot names and environments are each limited to 128 UTF-8 bytes without control
characters. Configuration schemas express their structural limits; runtime additionally checks
numeric socket parsing, byte counts, filesystem ownership and cryptographic relationships.

The transport uses TLS 1.3 and ALPN `save-runner/1`, mandatory certificate validation plus leaf
pins, no early data/resumption, one bounded framed RPC per connection, and an absolute connection
deadline. At most eight connections and two workers are active. Messages remain bounded to 64 KiB
requests and 2 MiB responses; job stores retain their separate capacity/event/result bounds.
The controller uses its ordinary request deadline for each RPC, independently of the admitted
job's runtime. Interrupting a controller stops its transport wait; it does not cancel remote work.
After a submission may have reached the runner, interrupted transport retains unknown admission
and the original request ID for lookup. Remote cancellation uses the separate explicit operation.

## Acceptance required

Use real controller and runner processes, synthetic certificates and disposable stores inside
the existing isolated network/PID boundary. Exercise handshake and real task/extension execution,
missing/untrusted/expired/wrong-name certificates, wrong leaf pins, unknown principals, forbidden
targets/operations, protocol mismatch, conflicting duplicate requests, caller isolation, and
bounded malformed/slow messages. Prove lost-reply lookup without resubmission, changed-grant reads,
partition/runtime behavior, target-side cancellation, graceful stop, runner crash/restart, expired
queued work, partial persistence and bounded results. Bind installed acceptance to frozen source
and executable bytes after independent transport/authority/lifecycle review.
