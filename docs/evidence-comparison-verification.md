# Typed evidence comparison acceptance

WORKBENCH-014 is accepted for the Linux core/CLI fixture scope. `evidence.compare` reads two
verified retained bundles, checks compatibility, and compares typed Prometheus samples and Loki
entry multisets under an explicit normalization. It performs no live query or credential lookup.
The [contract](evidence.md#comparison-interface-and-projections) defines projections and limits;
the [roadmap](roadmap.md#workbench-014--typed-evidence-comparison) owns remaining work.

## Accepted evidence

The [verification summary](evidence/evidence-comparison-2026-10-10/verification-summary.json)
and [independent installed verdict](evidence/evidence-comparison-2026-10-10/independent-execution-r2.json)
record:

- 291 workspace tests passed, zero failed; 19 existing native tests remain ignored. The final
  source-metadata correction additionally passed all nine public comparison tests. Unchanged
  workspace evidence is reused; [build provenance](evidence/evidence-comparison-2026-10-10/build-provenance-r2.json)
  identifies that two-file delta and fresh format/lint/build checks.
- 38 schemas, 32 positive and 17 negative specification fixtures passed before acceptance docs.
- Independent static review found no unresolved issues in the accepted snapshot.
- An ordinary installed binary, copied to a private prefix and invoked from an unrelated working
  directory, passed ten commands: three real fixture collections and seven offline comparison cases.
  Ten terminal receipts, six retained query receipts and three manifests passed their frozen schemas.
- Collection produced 18 HTTPS requests. The fixture was closed and its threads joined before
  comparison; configuration/context files were removed and the provider was absent. All 17 store
  files remained byte-identical. Source/binary identities matched before and after execution.

The installed cases prove hand-counted metric and duplicate-log differences, exact nanosecond
identity, equality, exact-window incompatibility, relative-window alignment, bounded detail
omissions, manifest pins and path refusal. Public/core tests additionally exercise partial/missing
checks, units, sampling, redaction, nonfinite entities, unsupported shapes, direct/structured parity,
service/datasource/parameter incompatibility and the shared detail budget. Accepted store recovery,
retention and native evidence remain scoped to their earlier increments.

## Identity and resolved findings

Final frozen source: `target/evidence-comparison/review-r2`, 765 files; manifest SHA-256
`e80369b5af3eb33c479c37b7270f020693f88bbe081964f6c6c595c0e85295f7`.
Ordinary binary: 41,712,592 bytes; SHA-256
`319eabdd1a0f85d1c480362848d900c7b06abebfefaafbd4722b4c7ea8a14e70`.
The source manifest identifies an uncommitted working-tree snapshot; base HEAD alone does not
identify these changes. Acceptance documentation added afterward does not alter product bytes.

The [review record](evidence/evidence-comparison-2026-10-10/review-summary.json) closes three findings:

1. Matching sets of sampling intervals could hide changed intervals for corresponding series.
   The failing regression now passes with per-series compatibility checks.
2. Detail budgets did not reserve later empty arrays and could restart the emitted prefix at the
   logs check. Shared accounting and the public cross-check regression close both cases.
3. Independent installed execution rejected noncanonical `sources` metadata. The corrected receipt
   uses canonical source references, original observation times, `sourced` and `untrusted: true`;
   full bundle identities remain in comparison data. The unchanged installed checker passes r2.

Initial compiler/format failures, public fixture setup corrections, regression failures and the
failed installed r1 remain in the packet. They are not counted as successful verification.
The [artifact manifest](evidence/evidence-comparison-2026-10-10/artifact-manifest.json) hashes 115
retained artifacts; manifest SHA-256
`0b614edc494172163a485376dd5f50c572274ab5434af69a4ca2cbe8e4feee3e`.

## Limits

Complete comparison means supported retained observations were analyzed, not complete telemetry
or service health. Incompatible, interrupted, partial, missing and truncated results remain explicit.
The home-lab Grafana/plugin pilot, scoped live identity, native Windows support and hosted evidence
grants remain unverified. No commit, publication or live-service change occurred. Guided runbooks
are next in the retained dependency order; this increment does not finish the whole backlog.
