# Guided runbooks

The first installed runbook, `evidence-review` version 1, guides an operator through already
collected before/after evidence. CAP-12 and AC-15/16 require real supported observations,
prerequisites, typed branches, human checkpoints and version-aware resume. Live collection remains
an explicit separate operation. [The roadmap](roadmap.md) is the sole progress record.

## Public interface

`runbook list` and `runbook show evidence-review --version 1` inspect embedded reviewed definitions
without opening either store. Execution and resume require explicit recording and two distinct
startup directories:

```sh
save --json --state-dir /absolute/evidence --runbook-state-dir /absolute/runbooks \
  --record always runbook run evidence-review --input review.json
```

The input file contains exactly:

```json
{
  "before": {"id": "run-1-ab-1"},
  "after": {"id": "run-1-ab-2"},
  "normalization": "exact-window-v1",
  "on_failure": "stop"
}
```

References may include `expected_manifest_sha256`. Normalization is explicitly `exact-window-v1`
or `window-offset-v1`; failure policy is `stop` or `continue`. Optional
`--expected-definition-sha256` pins the inspected definition. Invalid fields, nulls, paths,
unsupported versions and malformed policies fail before journal creation.

`runbook status RUN_ID` reads the durable journal summary. Resume supplies the exact original input,
current revision and pending checkpoint name:

```sh
save --json --state-dir /absolute/evidence --runbook-state-dir /absolute/runbooks \
  --record always runbook resume RUN_ID --expected-revision REVISION \
  --input review.json --acknowledge review-complete
```

A fresh observation attempt is a new `runbook run`, optionally linked using paired
`--fresh-from RUN_ID --fresh-from-revision REVISION`. It never resets or changes old history.
The same core supports structured `runbook.list`, `.show`, `.run`, `.resume` and `.status` operations.
No hosted grant is implied. Run/resume use `auto` or `always`; other operations use `never`.

## Definition and execution

The embedded definition binds its ID/version/digest, explanatory text, typed input, fixed capability
versions, expected evidence, prerequisites, conditions and checkpoints. Unknown operations, cycles,
recursive invocations and unsupported conditions fail validation. Text never becomes a command.

The workflow verifies the before and after bundle through `evidence.show`, then invokes
`evidence.compare`. Each child receives pinned manifest references and the remaining parent budget.
These are actual shared-core observations. Failure or denial cannot trigger an alternate shell
command. `continue` may execute independent eligible checks; prerequisites still block dependent
checks. `stop` records later checks as omitted.

Typed scalar equality branches select `review-complete`, `review-partial` or `review-incompatible`
from the comparison's `data.state`. Conditions inspect typed data, never terminal text or eval.
Only the matching checkpoint waits for explicit acknowledgement; skipped branches remain visible.
Acknowledgement records an operator decision, not health, source authenticity or a new grant.
A limited comparison remains partial after acknowledgement.

## Durable resume

Journal identity binds the installed definition, exact typed inputs, target, source bundle digests,
branch decisions, completed child receipts, revision and remaining active budget. Before dispatch,
write the in-flight step durably. After execution, atomically publish the finalized sanitized child
receipt and clear in-flight state. Full child results remain in private journal storage; public
receipts expose bounded summaries, original observation timestamps, the retained comparison and
checkpoint instructions. Operators can review the differences before acknowledging them.

Every resume verifies the definition/input/target and source integrity again without redispatching
completed observations. The expected revision is checked under the exclusive journal lock; a stale
resume cannot apply a second acknowledgement. An uncertain in-flight step after a crash is never
silently repeated: explicit fresh attempt is required. Status exposes `in_flight` and `resume_safety` so this
uncertainty remains visible. Failed persistence never claims a durable
successful checkpoint, and an unacknowledged checkpoint never advances.

All child execution uses one monotonic deadline. Human waiting time is excluded from active work;
remaining active milliseconds are captured immediately before each durable publication. A resumed
invocation can narrow, not expand, that saved budget. The final publication overhead after the
snapshot is not debited from the next invocation; this is not an exact cumulative wall-time ledger.
Each invocation still includes its own filesystem, validation and child work in its deadline. Signals and result-consumer closure stop further dispatch. Finalizing
already observed bytes may use one bounded 2.5-second allowance, without admitting another child.
Synchronous filesystem I/O cannot forcibly preempt a blocked kernel operation.

## Journal format and limits

Use a separate private directory selected by `--runbook-state-dir`. It contains a discriminated
format-1 header, nonblocking exclusive lock, generated run-state files and bounded temporary staging
files. Evidence store format and immutable manifests are unchanged. Accidentally selecting the
other store's directory fails before mutation.

Reuse descriptor-relative no-follow access, owner-private directories, regular single-link private
files, atomic replacement and file/directory synchronization. Unknown formats/layouts and unsafe
files are refused; there is no automatic migration or cleanup. Bounds are 64 runs, four MiB per
state, 128 MiB total including temporary publication bytes, and 16 staging files. No silent eviction
or deletion occurs. State is restricted evidence; existing telemetry redaction limitations apply.

## Acceptance

Use real private HTTPS collection fixtures followed by offline installed runbook execution. Prove
checkpoint selection and resume without replay, preserved timestamps, stop/continue prerequisites,
version/input/target/source drift refusal, stale/concurrent resumes, explicit fresh attempts,
interruption/in-flight refusal, bounded children and honest persistence failures. Validate canonical
receipts and strict public schemas, review the changed storage/execution boundaries independently,
and bind installed execution to frozen source/binary bytes. Linux fixture acceptance does not
establish native Windows support, live home-lab compatibility or hosted authorization.
