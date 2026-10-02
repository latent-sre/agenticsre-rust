# Error-budget task acceptance — 2026-10-02

The [error-budget contract](error-budget.md) and shared object-shape fixes passed independent
static review and executable verification. Both Cally runtime lanes passed. This closes the
second fixed offline task; [the roadmap](roadmap.md) owns subsequent work.

## Candidate identity

- Disposable snapshot commit: `87f5162cf256e53e39dbf644af4dbc03ef6866d9`.
- Tree: `c87cee35b655f9ba4e15adaf54c7dd88c06ad983`.
- Archive SHA-256: `daf6c4278dc765f21a36974753fccfad381e24458941e2d798e55739b241eaee`.
- All 133 captured files matched before and after independent execution. The coordinator also
  matched all 30 current product source/resource/test/manifest files against this archive.
- Upstream calculator SHA-256: `4e55475e6b10a4a2e195b1a6f162d688b093664c06bee69cb09435f1931cf5c2`.
- Adapted calculator SHA-256: `e7938a81478ea47bb2568a2d24b303e8bc1767612ccdac9ffeb7da95df17a17f`.
- Installed binding SHA-256: `1f74493ffdb4d992e2bd8dcf1bb5eeeea5242e90480746cde5c35ec03ab81fe4`.

The original upstream command-line source is retained unchanged as reference. The executed pure
calculator and wrapper are separately identified adaptations. Product changes remain uncommitted;
the snapshot Git repository is disposable, at `/tmp/agenticsre-budget-final-s21w6gka`. Evidence
copies, this report and subsequent planning documentation were added after capture.

## Results

| Check | Observed result |
|---|---|
| Independent static review | APPROVE; no reportable findings; original process supervisor unchanged |
| Rust/Python suite | 46 Rust tests passed, including a test that executes all nine Python arithmetic cases |
| Public CLI contracts | 52 error-budget, 25 dashboard and 19 command-runner schema-valid results passed |
| Independent numerics | 48 arithmetic/input-identity cases, including each policy's exact and adjacent boundaries |
| Protocol and admission | 27 task/protocol fault cases; invalid output, failed children, capture loss, timeout and signals never became accepted calculations |
| Strict object forms | Actual baseline marker effects reproduced; candidate denied malformed envelope/target/limits and nested task inputs before dispatch |
| Installed binding | Offline installation worked without the checkout under hostile cwd/modules/environment; input and executable remained unchanged |
| Formatting/lint/specs | Passed; 18 schemas, 20 positive and nine negative specification fixtures |
| Cally | Python 3.11.2 passed all 46 tests and both tasks; the no-Python image explicitly refused both tasks |

The task preserves minutes versus events, near-zero exhaustion tolerance and positive zero. It
requires both bound windows for severity and labels fixed example thresholds and full-budget
projection. Completed exhausted/page calculations remain successful arithmetic with no health or
alerting claim. Source identity, normalized inputs, runtime and coverage are explicit.

## Reproduced corrections

Three regression classes were fixed before acceptance:

1. At SLO 99 and SLI 85.6, the upstream calculator displayed 14.40x but classified it below 14.4.
   Decimal comparison of normalized input values now includes that exact threshold and excludes
   its adjacent lower-rate value; reported arithmetic retains binary64 behavior.
2. Positional JSON arrays could stand in for object structs. Independent baseline runs actually
   created a touch marker through array-form envelope/target/limits and dispatched an array-form
   dashboard task input. Explicit shape guards now reject these before effects. Legitimate argv
   and findings arrays remain supported.
3. Default float parsing changed a finite input by one bit and could reject a valid child echo.
   The accurate roundtrip feature fixes it without weakening exact input comparison. The same
   independent Rust helper failed against baseline dependencies and passed against the candidate.

The [independent packet](evidence/error-budget-verification-2026-10-02.txt) records these baseline
reproductions and candidate checks. Focused author/coordinator evidence is also retained:
[threshold red](evidence/error-budget-threshold-red-2026-10-02.txt) /
[green](evidence/error-budget-threshold-green-2026-10-02.txt),
[object-shape red](evidence/error-budget-object-red-2026-10-02.txt) /
[green](evidence/error-budget-object-green-2026-10-02.txt), and
[real finite-input red](evidence/error-budget-finite-public-red-2026-10-02.txt) /
[green](evidence/error-budget-finite-public-green-2026-10-02.txt).

One test expectation was corrected: an 851-byte response fits the 1,024-byte minimum capture
limit and should succeed. Controlled interpreter injection independently verified capture-loss
failure handling; no product limit was relaxed to satisfy that test.

## Environments and retained evidence

Local verification used Rust 1.98.0, Python 3.12.3 for tasks and Python 3.14.7 for the harness,
with isolated network/process/filesystem namespaces, read-only sources/runtime and no credentials.
The shared UID already exceeded a 512-thread ceiling before execution; the coordinator approved
a disposable 1,024-thread limit while retaining one CPU, one Cargo job, two test threads and all
other controls. No host process was stopped or persistent host limit changed. Per-process memory
limits are distinguished from aggregate cgroup limits in the packet.

Cally used identical frozen source in pinned Rust 1.98.0 Debian 12 images, no network/host mounts,
2.5 CPUs, 4 GiB aggregate memory, 512 PIDs and a 15-minute limit. The
[Python log](evidence/error-budget-cally-python-2026-10-02.txt) and
[missing-Python log](evidence/error-budget-cally-missing-python-2026-10-02.txt) are unchanged copies.
Both builds produced SHA-256
`fb1871c0006aa51c296796f09b91b30c08e13c53677ee691a44cd1c3ac25ee2b`.
The independently installed binary was
`7f5b26966c4d86f543858faa6954f5fc18f7f65aebcacea13aef929dd7a94eec`.

Remote containers and independent disposable builds/installations/processes were removed. Detailed
verification commands, test-only harnesses and their hashes remain under
`/tmp/agenticsre-budget-verify-JgcJF2q7`; source/build archives remain under
`/tmp/agenticsre-poc-cally.REFnbXD5` and `/tmp/agenticsre-poc-cally.epw2FBaP`.

Native Windows/macOS, protected agent permissions, live telemetry/alerting and production readiness
remain outside this acceptance. The hosted GitHub workflow has been linted locally, not published
or run. No product commit, push, deployment or paid model campaign was performed.
