# Evidence report acceptance

WORKBENCH-017 passes for the Linux x86-64 core/CLI fixture scope of
[report creation](reports.md). CAP-15/AC-20 is exercised against the existing version-1 service-query
bundles. Other evidence kinds, hosted adapters and separately authorized publication remain open
in the [roadmap](roadmap.md).

## Installed observations

The [installed verdict](evidence/reports-2026-10-11/verification.json) records ten commands against
an ordinary CLI copied into a disposable installation, invoked from an unrelated directory.
Three real collections produced 18 authenticated fixture requests. The server and all fixture
threads stopped before reports ran; connection/context configuration was removed and no credential
provider was supplied to report invocations. All ten exact result receipts and all populated report
data passed their schemas.

Markdown and JSON reports retained the same verified facts, original windows/timestamps,
digest-bound references, redaction/provenance notices and explicit comparable disagreements.
The [example handoff](evidence/reports-2026-10-11/handoff.md) is generated from synthetic evidence.
It is historical source data, not a service-health claim. JSON rendering parsed back to the exact
structured report; direct and structured calls agreed after excluding their new generation times.

Real collected redacted evidence produced a partial report. A 1024-byte document budget omitted
the complete rendering with explicit partial state, rather than truncating a document. Adversarial
proposal text remained inert in Markdown and retained `operator_proposed`/`pending` labels plus
exact source digests. Modified artifact bytes caused refusal with no report. Original
[store hashes](evidence/reports-2026-10-11/store-sha256.json) matched after every read-only sequence
and after restoring the intentional tamper control.

## Regression and review evidence

The [workspace log](evidence/reports-2026-10-11/workspace.log) records 345 passing tests, zero
failures and 19 existing ignored native/read-profile tests. After reserving 1 KiB for final report
metadata and adding the four-bundle/failed-check case, focused verification passed:

- Six public report tests covering Markdown/JSON and structured parity, pending proposals and
  markup isolation, redactions, rendering limits, strict selection, tampering, all six pairwise
  comparisons between four bundles, failed receipts remaining missing and unchanged evidence.
- Six report core tests covering strict bounded inputs, adversarial Unicode/control/Markdown
  text, JSON roundtrip, distinct agreement/disagreement/insufficient/incompatible meanings,
  whole-document omission, final encoded bounds and human output.
- All-target/all-feature strict Clippy, ordinary CLI build, formatting, specification validation
  and whitespace checks. Existing full offline/collection/comparison regressions passed in the
  workspace run; their fixed analyzer/store behavior is reused.

The report worker installs the existing offline syscall profile before store access. It holds
verified bundles in memory for projection and pairwise comparisons; no second unpinned lookup
is used. Deadline/cancellation checks surround both analysis and serialization/rendering. Hosted
requests remain unsupported. Independent static review found no unresolved material findings and
zero independent P0/P1 findings. The static reviewer did not execute tests; runtime evidence above
comes from the coordinator's isolated executions.

## Identity and limits

The [1077-file source manifest](evidence/reports-2026-10-11/source-manifest.json) has SHA-256
`1a15e2b0a46435a82c8d693a111a863044a39f85d8b01d50bac3465a94461d2b`.
The ordinary binary SHA-256 is
`d00d0bdd44e16223bb54fd7080de03898dd2b0b5deab15faf87542ac01291196`.
Source/binary guards matched before and after installed execution; subsequent acceptance/status
documentation does not change the tested product, tests, schemas or installed verifier.

Reports are returned under `record=never`; no report file, knowledge edit, message or external
publication is created automatically. Operator proposals remain unverified proposed text.
The fixed analyzer profile is not an arbitrary-code, credential or filesystem sandbox. This
increment does not establish current health, model interpretation, native Windows behavior or
live source compatibility.
