# Prometheus rule validation acceptance — 2026-10-11

**Verdict:** PASS for WORKBENCH-020's Linux amd64 core/CLI increment of CAP-13/AC-19.
The [adapter contract](alert-rules.md) covers pinned Prometheus 3.15.0 alerting and recording
rule files. Live evaluation, metric existence, routing, template execution, other native builds,
hosted grants and broader validator coverage remain on [the roadmap](roadmap.md).

Final frozen source: 1258-file manifest SHA-256
`cdfd0fca4635d87ee45d0176f7e8521a51f62598991804fd3fd3c1f5d2f753f2`.
Ordinary CLI binary SHA-256:
`9adb672ef60d2e674897a37248650886d1137dad91c02a05841b90126f3d8885`.
Official native validator SHA-256:
`c736d55d3ccd959fe48329965fb5cb671d45585a9483954766448035100ff75c`.
Verification used a locally uncommitted working-tree snapshot; acceptance documentation follows
the freeze without changing tested product bytes. The executable is an operator-supplied
dependency, not bundled in the product.

The retained packet includes the [installed verdict](evidence/alert-rules-2026-10-11/verification.json),
[source manifest](evidence/alert-rules-2026-10-11/source-manifest.json),
[checks](evidence/alert-rules-2026-10-11/checks.json),
[dependency provenance](evidence/alert-rules-2026-10-11/dependency.json),
[upstream fixture observations](evidence/alert-rules-2026-10-11/upstream-probe.json),
[workspace output](evidence/alert-rules-2026-10-11/workspace-final.log) and
[artifact hashes](evidence/alert-rules-2026-10-11/artifact-sha256.json).
Exact stdout/stderr for all 21 installed commands is under the packet's `commands/` directory.

## Demonstrated behavior

| Requirement | Evidence |
|---|---|
| Fixed executable and immutable input | Only the official binary digest is accepted. Execution uses a private unlinked read-only executable snapshot and a sealed model descriptor; originals remain unchanged. Unit checks cover source replacement, descriptor access modes, seals, cleanup and execution without the source pathname. |
| Declared validation scope | Valid alerting/recording rules report two checked rules. PromQL, template syntax, unknown fields and invalid recording-rule fields fail with native diagnostics and consumed input identity. Duplicate lint preserves native exit 3; the optional findings policy changes only the CLI exit. |
| Honest incomplete outcomes | Empty files are uncheckable. Multiple-document warnings yield partial/unknown for both native exits 0 and 3. Truncated diagnostics never claim complete validation. |
| Refusals before effects | Missing configuration, wrong binary, incompatible kind/validator and wire-supplied executable/arguments refuse. Binding mismatch precedes missing-executable access; input mismatch precedes child launch. Hosted callers remain refused. |
| Limits and preparation | Oversized inputs and FIFOs refuse. A one-millisecond preparation deadline returns `timed_out`, CLI exit 1, no consumed-input claim and no child supervisor. Shared supervisor tests cover process timeout, cancellation, output loss and result-consumer closure. |
| Installed public contract | A copied ordinary binary and copied validator run from an unrelated directory in the existing sandbox. JSON receipts and configuration data validate; human findings and structured-call identity agree. Input, executable and source guards match afterward; no executable snapshot names remain. |

## Checks and resolved issues

The full sandbox workspace passed **387 tests, zero failures**, with 22 ignored cases. These include
19 existing native cases and three new tests requiring the official validator fixture. An explicit
fixture-enabled run passed all five public Prometheus tests, including those three. That run
preceded the two final launcher test-fixture corrections; the installed mission used the final
binding identity. Strict workspace lint, formatting, ordinary build and whitespace checks passed.
The specification suite passed 60 schemas and 42 positive/28 negative fixtures.

Independent static review of the revised snapshot boundary reported no unresolved material
findings and no P0/P1 findings. The reviewer did not run executable checks; the coordinator ran the
integrated suite and installed mission. This evidence reuses the earlier accepted dashboard,
offline, report and toil slices while exercising workspace compatibility.

Initial native execution exposed an AppArmor refusal of executable memory files. An isolated
probe confirmed private unlinked ordinary files could execute under the unchanged profile.
The adapter now removes private names before copying, closes its writer before descriptor-based
launch and retains only the read-only executable snapshot. Model sealing remains unchanged.
No sandbox or host-profile relaxation was used. The
[diagnosis record](evidence/alert-rules-2026-10-11/initial-launch-diagnosis.json) also records why
the two launcher fixtures now prefer GNU `printf`: this host's uutils shim rejects descriptor
execution. The initial workspace failure is retained; the corrected full suite passed.

Initial installed acceptance exposed a verifier error: timeout was expected to exit 124, but the
[common CLI contract](poc.md) specifies 1. The actual result already reported `timed_out` without
child effects. Only that expectation changed. Initial evidence and its source manifest remain
under `initial-installed/`; all 21 final commands passed with the same product binary.

Execution uses ordinary account authority. Same-account process tampering, abrupt-crash cleanup
during initial temporary-file creation, live deployment correctness and distribution clearance
are outside this acceptance scope.
