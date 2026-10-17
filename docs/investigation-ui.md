# Browser context and historical evidence investigations

WORKBENCH-027 extends the existing local checks, results and session-history interface with
service context, saved evidence inspection, comparisons and report previews. The existing
[HTTP/session contract](ui.md) remains the boundary. Live status and verification belong in
[the roadmap](roadmap.md).

## Scope and authority

The launcher independently grants exactly `context.resolve`, `evidence.list`, `evidence.open`,
`evidence.compare` and `report.create` through repeated `--allow`. Context uses the existing paired
`--context-source` and `--context-max-age-days`; evidence/report operations use an existing private
`--state-dir`. Required startup inputs must be present, and inputs without matching grants are
refused. Evidence retention/size overrides remain unavailable to the browser launcher.

A new opaque core investigation grant owns only the selected offline source/freshness policy and
store. Context execution validates the pinned source identity and digest against the same bytes
used for resolution. Evidence execution checks the selected root identity inside the actual store
open, before reading its data. Replacing a source or store cannot redirect an existing session.
An explicitly granted store may gain new valid bundles through separately authorized CLI work;
each selected bundle still undergoes its normal integrity checks and optional manifest pin.

Generic hosted execution refuses context, evidence and report operations without this grant.
The grant cannot authorize commands, collection, queries, protected-state changes, deletion,
runbooks, remote execution, schedules or controlled changes. Core report creation renders content
to the caller and never publishes or writes a report file. The existing offline seccomp profile
for evidence inspection and report rendering is preserved.

All operations keep `target=local/workstation`, `record=never` and the existing honest core receipt.
Store reads acquire the existing private lock without changing retained bytes, protection or
retention metadata. No missing store is created. Filesystem access-time changes are outside the
retained-byte guarantee. Browser-origin requests never carry paths, credentials or grants.

## Browser behavior

The Check selector adds Service context, Saved evidence, Inspect evidence, Compare evidence and
Evidence report. It lists only operations granted by this process. Forms submit the existing core
input schemas through `POST /api/v1/runs`; no additional execution endpoint or alternate executor
is introduced. Root/diagnostic target selectors are absent for these operations.

| View | Input and presentation |
|---|---|
| Service context | Service/environment and optional team/deployment; source identity, selected ownership/bindings/dependencies, freshness and reasons a mapping is unusable |
| Saved evidence | Empty input; retained run identifiers, state, observation/storage times, protection and errors |
| Inspect evidence | Run identifier and optional exact manifest digest; original window, capture, checks, missing/omitted details and provenance |
| Compare evidence | Before/after references and exact-window or window-offset normalization; compatibility, reasons, metric/log changes and omissions |
| Evidence report | One to four distinct references, format and optional title; plain-text Markdown/JSON preview and explicit local download |

Timeout and output controls preserve existing core ceilings. Item limits apply only to open and
compare. Unknown/null controls, filesystem paths, duplicate report references and malformed pins
are refused. Mutations are never retried automatically; the existing submission identity,
reconciliation, cancellation and history rules remain unchanged.

Source records, log text, labels and report Markdown are untrusted data. They render as text, with
no HTML execution, automatic links or remote images. Historical observation times are distinct
from analysis/generation time. Missing samples remain missing, incompatible comparisons remain
incompatible, and completed analysis does not claim current service health. Proposals remain
pending and cannot approve themselves.

The canonical JSON receipt remains downloadable byte-for-byte from the same retained invocation.
Report download is an explicit browser Blob action with a safe fixed filename and revoked object
URL. Results remain in the existing bounded process session; closing it discards browser history,
while saved evidence stays in the separately granted store. Only theme preference is persisted by
the browser.

## Implementation and verification

Core owns grant creation, exact-consumed identity checks and typed input validation. The Rust UI
adapter owns startup pairing, normalization and the dedicated executor call. OpenAPI embeds the
existing authoritative schemas; generated TypeScript consumes that artifact. The frontend extends
the accepted forms, themes, result panels and history without replacing their lifecycle.

Verify the actual context → list → inspect → compare → report browser workflow against copied
retained fixtures with no source credentials or live targets. Check exact receipt/download bytes,
source/store immutability, swapped-source/store refusal, missing/corrupt/pinned evidence, bounded
output, hostile content, deduplication, grants and existing token/Host/Origin controls. Exercise
keyboard flow, light/dark/mobile rendering and report download in a real sandboxed browser, and
inspect captured screenshots. Retain installed source/binary identities and independent review of
the new grant boundary. Native Windows, live data collection and team/remote hosting remain separate.
