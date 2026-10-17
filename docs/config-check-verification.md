# Declared configuration-check acceptance — 2026-10-11

**Verdict:** PASS for WORKBENCH-019's Linux core/CLI dashboard increment, covering CAP-13/AC-19
through the [declared configuration-check contract](config-check.md). The adapter validates the
installed dashboard checker’s ten textual rules. Additional alert/config adapters, full Grafana
schema conformance, live deployment correctness, hosted grants and native Windows remain outside
this accepted slice and stay on [the roadmap](roadmap.md).

Final frozen source: 1192-file manifest SHA-256
`9ecf0472349fd296a6a09c5141b1e8b618cc17b7e43c91e8ad6fc3a7c2bcdf92`.
Ordinary CLI binary SHA-256:
`db87eebf88fa473d9ac7ea12c82efc3ef0b5d437a5a528bbcbe22c57e04822cf`.
This is a locally uncommitted working-tree snapshot; acceptance documentation follows the freeze.

The packet retains the [installed verdict](evidence/config-check-2026-10-11/verification.json),
[source manifest](evidence/config-check-2026-10-11/source-manifest.json),
[check record](evidence/config-check-2026-10-11/checks.json),
[workspace output](evidence/config-check-2026-10-11/workspace.log),
[input hashes](evidence/config-check-2026-10-11/input-sha256.json) and
[artifact hashes](evidence/config-check-2026-10-11/artifact-sha256.json).
All 20 installed commands' exact stdout/stderr are retained under the packet's `commands/` directory.

## Demonstrated behavior

| Requirement | Evidence |
|---|---|
| Explicit installed identity | Results identify kind, validator ID/version, unchanged upstream checker digest/revision and actual embedded binding digest. Discovery publishes the configuration input schema and dependency boundary. |
| Pins bind consumed bytes | Matching pins succeed. Validator mismatch refuses before missing-file access or child dispatch. Input mismatch reports the actual read digest, fails before checking and retains zero checked panels. Wrapper code hashes and validates the same bounded buffer without reopening. |
| Supported and unsupported models | Raw legacy, legacy response and fixture-backed v1beta1 app envelopes pass. Unknown/mixed envelope declarations, wrong declared kinds, malformed JSON, duplicate keys, invalid UTF-8, zero panels and V2 shapes never claim complete validation. Bounded complete reads retain input identity on these failures. |
| Findings and exit semantics | Findings are typed and countable. Default successful execution exits zero; `--fail-on-findings` exits one while preserving successful execution and `not_assessed`. Human text includes the same findings and identities. |
| Partial and unavailable evidence | Oversized inputs retain null digest/byte count. Long finding labels produce partial validation. A 20-panel finding fixture exceeds the 1024-byte child capture limit and cannot claim completeness. FIFO input is refused without blocking. |
| No source mutation or workspace dependency | Installed binary runs from an unrelated directory beside a hostile same-named Python module. Fixed isolated binding is used; original input hashes stay unchanged. No credentials or network are present in the test namespace. |
| Schema and structured-call parity | All installed receipts and populated configuration data validate. Structured invocation matches the CLI's validator identity, input digest and findings. |
| Refusal and deadline behavior | Core/public tests cover hosted refusal, recording-mode refusal, unknown versions, array/null/override inputs, control-character paths, inconsistent child protocol and expired budgets without child/input-read claims. Existing task cancellation/output tests exercise the shared supervisor. |

## Checks and resolved issues

The full sandbox workspace passed **376 tests, zero failures**, with 19 existing native cases
ignored. This includes the six new public configuration workflows, core admission/protocol tests,
legacy dashboard task checks and toil capture regressions against the changed wrapper. Strict
workspace lint, formatting, ordinary build, whitespace and specification checks passed. The
specification suite covered 58 schemas and 41 positive/26 negative fixtures.

Independent static review found one P2 mismatch: runtime path admission allowed ASCII control
characters while the schemas forbade them. Runtime now rejects controls and DEL before model
access; unit and public regressions cover it. The reviewer confirmed closure, reviewed the final
timeout metadata changes and reported no unresolved material findings. The reviewer did not run
tests; the coordinator executed the integrated checks.

Initial installed acceptance exposed a verifier fixture gap: a single finding fit within the
requested capture budget, so the expected truncation did not occur. The fixture now contains
20 panels. The initial receipt and source manifest are retained. Only verifier bytes changed
between those runs; final installed acceptance passed all 20 commands with the same product binary.
A new discovery-reference test also corrected the previous `toil.prune` metadata to use its strict
empty-object schema rather than a nonexistent schema reference.

Execution uses the existing fixed Python binding and ordinary account authority. This evidence
does not establish a filesystem/account sandbox, support for arbitrary third-party validators,
plugin-specific validation, query correctness or live Grafana health.
