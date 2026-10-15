# Reports and knowledge proposals

CAP-15 and AC-20 require handoffs to distinguish historical facts, missing evidence and proposed
changes. `report.create` produces a bounded Markdown or JSON document from selected verified
version-1 service-query bundles. It uses the same offline analysis and comparison semantics as
[evidence replay](offline-evidence.md). [The roadmap](roadmap.md) records acceptance and later work.

## Interface

```sh
save --json --state-dir /absolute/private/store report create --input report.json
```

The input file contains the operation's inner input:

```json
{
  "bundles": [{"id": "run-1-ab-1"}, {"id": "run-1-ab-2"}],
  "format": "markdown",
  "title": "Incident evidence handoff",
  "proposals": [{
    "kind": "runbook",
    "title": "Review observation coverage",
    "content": "Consider documenting the gaps shown by these observations.",
    "evidence": ["run-1-ab-1"]
  }]
}
```

Choose one to four distinct generated run IDs in the explicitly configured store. Each reference
may pin `expected_manifest_sha256`. `format` is `markdown` or `json`; `title` defaults to
`Evidence handoff`. At most eight optional proposals select `service_card`, `alert_card` or
`runbook`. Titles are limited to 160 characters; proposal contents to 2000. Each proposal must
reference one or more selected bundle IDs. Null/unknown fields, paths, unsupported formats,
duplicate selections and caller-supplied approval/publication controls are refused.

The [input schema](sre-workbench/schemas/report-create-input.schema.json) is shared by CLI and
structured `report.create` calls. `--json` selects the normal result envelope; the input's `format`
selects the document in `data.rendering.content`. Human output includes the rendered document and
the operation's status. A report uses `record=never` and is returned to its caller; there is no
implicit export, retained report file or external publication.

## Facts, disagreements and proposals

The [result data](sre-workbench/schemas/report-data.schema.json) carries the structured report and
its rendering. Both formats represent the same observations, comparisons, limitations, next
questions and knowledge proposals. Sources include exact bundle digests, original observation
times and `sourced`/`untrusted` labels. The report's generation time and tool version describe the
analysis rather than refreshing historical facts.

Reports retain collection selection/window, provenance, capture status and redaction notices.
Checks include typed summaries and bounded detail with explicit omissions. Pairwise comparisons
use `exact-window-v1` against the already verified in-memory bundles. Comparable differing
observations are labeled disagreements; they do not establish causation or successful remediation.
Incompatible identities/windows, missing or failed checks, redaction and unsupported observations
remain explicit. Missing data is never zero. A corrupt or invalid selected bundle fails the report
instead of silently excluding that source.

Proposal text is operator-supplied data. Every proposal is labeled `operator_proposed` and `pending`,
with exact digest-bound evidence references. A proposed statement is never promoted into a fact or
approved knowledge. This operation never edits service cards, alert cards or runbooks and never
submits a message. No model interpretation is generated in this increment.

Markdown uses trusted section headings and indented JSON code blocks for report data. Each
untrusted data line remains in its block, including titles, log text, URLs and proposal content.
Display controls are escaped. Source text cannot create HTML, active links or instructions outside
the document structure. JSON preserves typed source values and explicit provenance.

## Execution and limits

The fixed report analyzer runs under the Linux x86-64 offline worker profile before opening the
private evidence store. It neither loads live connection configuration nor requests credentials,
contacts sources, runs embedded commands or invokes plugins. Profile setup fails closed. The
[browser investigation extension](investigation-ui.md) accepts `report.create` only with its
independent launcher grant and existing private startup store. The
[MCP investigation extension](investigation-mcp.md) applies the same grant to
`workbench_report_create`. Generic hosted execution still has no report authority; other platform
profiles remain unavailable. The worker boundary is
not an arbitrary-code or credential/filesystem sandbox.

One parent deadline includes verification, observation projection, pair comparisons and rendering.
`--timeout-ms` defaults to 30000 with the existing 300000 ceiling. Detail budgets are bounded per
analysis; summaries retain full supported counts and disclose omitted detail. The structured report
is bounded to 512 KiB. The rendered document budget defaults to 1 MiB and is controlled by
`--max-output-bytes`; the normal 2 MiB final receipt ceiling also applies. A document exceeding its
budget is omitted whole with its required byte count and partial status, rather than returned as
apparently complete truncated Markdown/JSON.

Complete reporting exits 0, including a complete report of comparable disagreements. Missing,
incompatible, redacted or omitted analysis remains partial (exit 1). All reports retain
assessment `not_assessed`. Cancellation, deadlines, unavailable isolation, store errors and invalid
requests retain their established failure semantics. Evidence bytes and retention metadata do not
change during reporting.

## Acceptance

Exercise real collected complete, differing and redacted bundles after source shutdown and removal
of connection credentials/configuration. Verify Markdown/JSON and structured CLI parity, exact
source references and old timestamps, missing/incompatible checks, hostile Markdown/control text,
pending proposals with selected evidence, render limits, tamper refusal and unchanged store bytes.
Validate canonical and report-data schemas, and retain an ordinary installed-binary mission.
Existing offline/collection/comparison regressions must continue to pass.
