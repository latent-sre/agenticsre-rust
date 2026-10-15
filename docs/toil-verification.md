# Opt-in toil acceptance — 2026-10-11

**Verdict:** PASS for WORKBENCH-018's Linux core/CLI fixture increment, implementing the
[toil contract](toil.md) and CAP-16/AC-21's opt-in metadata and arithmetic slice. This establishes
software behavior with synthetic samples, not observed productivity benefit, a human/agent pilot,
live adoption, hosted grants or native Windows support.

The frozen working-tree manifest contains 1129 files, SHA-256
`c743cfe3e39f26503b7323c5c2aa22c2efec9cac09868deb55c34dbbb5698635`.
The ordinary CLI binary SHA-256 is
`b78b80ada15a01ed2553387e0a66609505e6deee1b86592f9f2fc81be18b78eb`.
Source remains locally uncommitted; acceptance documentation was added after this snapshot.

The [installed verdict](evidence/toil-2026-10-11/verification.json),
[source manifest](evidence/toil-2026-10-11/source-manifest.json),
[checks](evidence/toil-2026-10-11/checks.json),
[workspace log](evidence/toil-2026-10-11/workspace.log) and
[artifact hashes](evidence/toil-2026-10-11/artifact-sha256.json) retain the evidence. Exact stdout
and stderr for all 23 installed commands are preserved under the same packet's `commands/` directory.

## Behavior demonstrated

| Requirement | Evidence and result |
|---|---|
| Explicit consent and opt-out | Disabled initial store and later opt-out refuse capture/record/annotation before child dispatch; store bytes remain unchanged. Summary, prune and delete remain available. Mutations with `record=never` are refused. |
| Real task capture with minimal metadata | Installed error-budget task succeeds; its canonical child receipt is returned. Stored sample records runtime, task/run identity and outcome, with unknown human effort. Input canary, parameter names, stdout/stderr and calculation data are absent from stored bytes. |
| Missing and estimated labor | Capture initially produces null savings and `missing_effort`; annotation updates the same sample without changing sample count, labels effort estimated and preserves runtime provenance. |
| Cost and frequency arithmetic | Baseline effort 1000/2000 ms and candidate 500/1000 ms produce means 1500/750 ms, 0.25 episodes/day in an eight-day window, gross 750 ms/run, observed gross 1500 ms, costs 1500 ms, net zero and break-even two runs. Projection at 300 runs yields estimated net 223500 ms. |
| Bounded/exact arithmetic and timestamps | Core tests cover large exact rational break-even, finite projections, negative benefit, failed episodes, half-open nanosecond selection and refusal of fractional-millisecond window lengths. |
| Retention and deletion | Shortening retention excludes the expired sample without modifying the store; explicit prune deletes exactly that row and delete removes exactly the selected captured row while disabled. |
| Private bounded store | Core tests cover lock contention, quota refusal before capture, unknown format/layout preservation, array-encoded sample and incomplete lock-only refusal, atomic failed publication and cancellation/deadline before publication. Secure filesystem primitives reuse the existing no-follow owner/mode checks. |
| Truthful post-task recording failure | Injected pre-publication failure retains the actual successful child receipt, reports recording failure, leaves the sample absent and preserves old store bytes. This is fault-injection evidence, not a new native disk-full exercise. |
| Contract parity | Installed results and populated toil data validate against the JSON schemas. Structured summary matches the CLI summary. Input arrays, raw-content fields, explicit nulls and unsupported task inputs are refused. |
| Execution boundary | Installed copy runs from an unrelated directory under the existing private-network/credential-free test sandbox. Source/binary hashes match before and after; no external service is needed. |

## Checks and review

The full workspace passed **361 tests, zero failures**, with 19 existing native tests ignored.
After the final task-object validation change and two added regressions, all **12 core toil tests**
passed. Five public toil tests passed; final installed acceptance passed all 23 commands. Strict
workspace lint, formatting, ordinary CLI build, whitespace and specification checks passed.
The specification suite covered 56 schemas and 40 positive/24 negative fixtures.

Independent static review found one P2 schema mismatch: populated interrupted receipts could add
`cancellation_signal` but toil data schemas prohibited it. The schemas now accept the bounded
signal field and a positive interrupted-data fixture verifies the shape. The reviewer confirmed
closure and reported no other unresolved material findings. Coordinator inspection also corrected
positional-array task preflight before acceptance. The reviewer ran no executable checks; the
coordinator executed the checks above against the integrated implementation.

Same-account metadata editing, selection bias, unknown effort, small sample sizes and estimated
costs remain explicit limitations. No causal savings, employee ranking, automatic telemetry,
background retention deletion or arbitrary command capture is claimed. The
[roadmap](roadmap.md) retains the broader capability queue and separate live/platform requirements.
