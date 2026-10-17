# External extension execution

This is WORKBENCH-021's [accepted Linux x86-64 execution contract](extension-runtime-verification.md). The
[package format](extensions.md) remains inert. Execution additionally requires an explicit,
private operator policy authorizing the exact installed package digest and capability name.
The initial profile is `operator-reviewed-v1`, for code the operator has reviewed and chosen to
run with their own account. A subprocess is not an untrusted-code sandbox.

## Operator policy and invocation

```json
{
  "policy_version": 1,
  "execution_profile": "operator-reviewed-v1",
  "packages": [{
    "package_sha256": "sha256:REVIEWED_PACKAGE_DIGEST",
    "capabilities": ["ext.example.capacity.project"]
  }]
}
```

The policy is an absolute, owner-only 0600 regular file: no symlinks, hardlinks, unknown fields,
duplicate keys or alternate profiles. It is bounded to 64 KiB, 32 package pins and eight capability
names per pin. Package metadata and wire inputs cannot create or change this policy. Installers
do not enable code. The host reads policy and verified package bytes for each invocation; it does
not retain a stale authorization cache or choose a replacement version implicitly.

```sh
save --json --extension-state-dir /absolute/private/extensions \
  --extension-policy /absolute/private/extension-policy.json \
  extension run ext.example.capacity.project --input capacity.json
```

`extension run` accepts `--operation-version`, `--timeout-ms` and `--max-output-bytes`.
Foreground requests retain the 300-second ceiling. The private [durable job worker](jobs.md)
may supply a remaining execution allowance up to one hour; protocol peers must validate that
bound independently. This does not permit a foreground caller to request a longer timeout.
It maps to the ordinary request envelope with the capability's `ext.*` operation ID, flat typed
inputs, local workstation target and `record=never`. Structured `call` uses the same operation
directly. UI/MCP cannot inherit the store/policy or execute these external capabilities.

With an explicit store, operator-local capability discovery includes verified external metadata.
Effective availability requires a matching policy grant; conflicting authorized versions refuse
ambiguous selection. Package inspection stays independent of the store and reports inert metadata.
Inventory distinguishes effective policy authorization from installation, and reports the policy
digest. Declared provenance remains unverified even when execution is authorized.

## Wire protocol

Messages are UTF-8 JSON objects terminated by LF, at most 64 KiB including LF. Every message has
`protocol: 1`, exact `request_id` and `run_id`, a nonnegative integer `sequence` and a `kind`.
Unknown keys, duplicate keys, positional arrays and unsupported versions fail closed.

1. Host sends `hello`, sequence 0, containing only those five common fields.
2. Peer returns the correlated `hello`, sequence 0. Work is not sent before this succeeds.
3. Host sends `request`, sequence 1, adding `operation`, `operation_version`, `inputs` and
   `limits` containing the remaining timeout and effective output budget. Host then closes stdin.
4. Peer emits zero or more `progress` frames with `message`, then exactly one `result`.
   Peer sequence numbers begin at 1 and increase by one across all work frames.

Progress messages are bounded to 1024 characters. A result contains `status`, `data` and `error`:
success requires the declared flat output schema and null error; failure requires null data and
an error object with bounded `code` and `message`. The host validates output rather than trusting
the peer's claimed success. Work frames arriving before the host finishes writing its request
are invalid, as are frames after the terminal result, incomplete trailing bytes or silent exits.

At most 64 peer frames and 256 KiB total stdout are admitted. Stdout is protocol data, not an
extra user-visible transcript. Stderr is separately bounded to 64 KiB and marked untrusted.
There are no callbacks, host tool requests, arbitrary environment/argv overrides, network grants
or artifact references in this initial protocol. Larger artifacts need a later validated-reference
increment; they cannot bypass the frame bounds.

## Execution and receipt integrity

The host verifies the installed package, validates inputs against its supported schema subset,
decodes the pinned executable and copies it into a private unlinked read-only snapshot. All
writable handles close before descriptor-based launch. The original package path is never
reopened for execution. A source/store pathname replacement cannot substitute unchecked bytes.

Execution uses empty argv, fixed `save-extension` argv0, working directory `/`, the shared minimal
environment and closed ambient descriptors. This removes inherited credentials and caller command
selection; it does not restrict the account's filesystem or network authority. Discovery classifies
effects as unclassified, and receipts retain unknown effects after launch even when a peer declares
an observation-only capability. Same-account/root tampering is outside this boundary.

The existing process supervisor owns nonblocking stdin/stdout/stderr, process-group observation,
deadlines, cancellation, result-consumer closure and TERM/KILL cleanup. A protocol fault starts the
same cleanup path. Peer success additionally requires native exit 0, clean protocol EOF and
confirmed cleanup. Truncation or incomplete cleanup cannot produce complete success. No retry or
automatic replay occurs; repeating a request ID does not create durable idempotency.

Receipts retain package/executable/policy identities, correlated protocol state, untrusted progress
and terminal data, diagnostics and the original supervisor outcome. The effective output limit is
the serialized detail budget for `{progress, terminal_data, terminal_error_message, stderr}`.
`detail_bytes` and `detail_budget_bytes` expose that accounting. Required identity, protocol and
supervisor metadata are outside the detail budget; the complete receipt still obeys the common
2 MiB encoded limit. Progress may be omitted, then terminal data, then diagnostic/error text;
explicit omission flags and partial outcomes preserve what was lost. Error code/status remain.

## Capacity-projection experiment

The independent `capacity-projector` executable has no dependency on the workbench core. It
implements `ext.example.capacity.project` using the same protocol and request/result boundaries.
Inputs are supplied `capacity_bytes`, `used_bytes` and `growth_bytes_per_day`, each an integer
between 0 and 9,007,199,254,740,991. No live measurement or service-health inference is performed.

`headroom_bytes` is capacity minus usage, floored at zero. Positive growth yields whole days of
headroom, rounded down. Less than one day is zero days without claiming exhaustion. Already-full
capacity has zero remaining days. With spare capacity and zero growth, `has_finite_horizon=false`
distinguishes an absent projected horizon from the zero numeric placeholder. Every result states
the constant-growth assumption. Inputs beyond safe bounds or with unknown fields refuse.

Build and package the example with the [development workflow](development.md). Installing it alone
must still refuse execution. The separate adversarial test peer deliberately emits malformed
messages and stalls; it is not the production executable and is never enabled outside disposable
verification. The complete experiment and remaining scope are tracked in [the roadmap](roadmap.md).
