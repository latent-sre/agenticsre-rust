# Planning import record

The owner selected this repository on 2026-10-02 as the home for the proposed SRE product.
The source is Save Toolkit commit 7d900fb999abee3b3712e1da881dcc7e80b1f6e7, published in
[PR 308](https://github.com/latent-sre/save-toolkit/pull/308). That PR was open at import time.
The complete planning directory was copied from the committed source, not from the unrelated
dirty copy in the shared Save Toolkit checkout.

## Preserved scope

The import preserves all 25 capability specifications, 17 requirements, 12 nonfunctional
requirements, 36 acceptance cases, 16 work packages and 13 draft schemas. The original 24
positive/negative fixtures remain; a timestamp rejection fixture is added to enforce the
existing date-time contract in an independently installed validation environment.

## Adaptations

- Internal package links remain local. References to Save Toolkit code, agent rules and historical
  roadmap items now use immutable source-commit URLs.
- Product status moves to this repository's [roadmap](roadmap.md). Save Toolkit remains a future
  consumer and the historical source of the plan.
- Repository selection is recorded in DEC-02; product/CLI branding remains provisional.
- The existing MIT license is preserved, with the original imported copyright/license notice.
- The verifier now checks this repository's README, contribution guide, roadmap and import record.
  Pinned documentation dependencies and a CI workflow make it independently runnable.
- Date-time checking is required explicitly. The source environment lacked the optional
  RFC3339 checker, which allowed an invalid timestamp through a schema format annotation.
  The import adds that dependency, a missing-checker failure and a regression fixture.

## Limits

Importing the plan does not accept every proposed architecture decision, implement the Rust
product, install Rust, configure credentials, change an agent's grants or authorize live reads/writes.
Existing source observations retain their original checked dates and evidence limits.
Source PR 308 remains a historical review reference; this product's evolving plan and implementation
status are maintained here.
