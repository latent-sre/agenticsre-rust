# Local browser interface verification

The agreed Linux browser PoC is implemented: launcher-granted Git/ripgrep checks and numerical
error-budget calculations, exact receipt downloads, cancellation and session history. Correctness
review and a dedicated independent security audit approve the final source. The final independent
focused native run passed. This is scoped GUI acceptance, with the underlying runner's actual
namespace-setup-refusal test still unverified; it is not production or cross-platform acceptance.

## Final source and artifacts

- Disposable snapshot: `/tmp/agenticsre-gui-r2-final-9zd3x237`.
- Commit: `76e6a3f316da685e12f3e741935f6dd55d9e64c0`.
- Tree: `9bde9db7f06e19e8b6e527e2f7b31934f5dd6776`.
- 259-file archive SHA-256: `c0f571485b61025e88ca2b067b2acfd1bea15f4736eb0526cab65085e480f404`.
- Local `save`: 35,178,976 bytes; SHA-256
  `ec57514bfcc1c13780360453b7a78cca715e929820451d49bfb1e60abdc0fd91`.
- Cally `save` SHA-256: `4137235c556d577eda6df72f9f0b56e4fbedf9af2ccab00dfed8633cc61be152`.

The local and Cally binaries are separate builds; no reproducible-binary claim is made. The product
checkpoint is committed separately at the owner's request. This acceptance record and subsequent status-only documentation
updates are outside the frozen executable source; production code and embedded assets match it.
Build/start instructions are in [development](development.md#local-browser-interface), with the
authoritative behavior and limits in [the GUI contract](ui.md).

## Evidence and retained coverage

| Check | Result and source scope |
|---|---|
| Independent correctness review | APPROVE on final R2, retaining unchanged initial/R1 review coverage; all findings resolved |
| Dedicated independent security audit | APPROVE on final R2, retaining unchanged whole GUI/affected-core coverage; no unresolved validated findings or P0/P1s |
| Final frontend checks | 34 tests, lint, strict TypeScript, Vite build and generated OpenAPI type parity passed on R2 |
| Final independent Cally GUI checks | Six UI library tests, seven HTTP/native tests and all five actual browser flows passed on R2 |
| Core/profile/Grafana regressions | Retained independent initial integrated run: 96 ordinary and 18 native outcomes, 51 schema-valid profile receipts, 96 Grafana cases and literal-printf mission |
| Live OpenAPI response validation | 15 coordinator validations passed; R1 server/API bytes remain unchanged in R2 |
| Documentation and formatting | Schema/example/link/ID checks and Rust formatting passed; affected Rust Clippy passed after the diagnostic correction |

The final command was `tools/cally-profile-check.sh gui-focused`, run independently from an exact
clone of the R2 snapshot. It exited 0 after 94.62 seconds. Evidence is retained in
`/tmp/agenticsre-profile-cally.KSgEc2Ky`; browser results and screenshots are in its `ui/` directory.
The independent verdict is **PASS — scoped GUI acceptance**. The final verifier packet is
`/tmp/agenticsre-gui-r2-verify-c_pfywgk/packet.txt`, SHA-256
`114893b8219e6437a0d863da9e6d85a2bf91cd0172c985c7066114ba27f059cb`; its `SHA256SUMS` binds the
retained records. Independent exact-name container/image readbacks are empty and disposable
staging was removed. No run-owned test container or derived image remains.

The browser exercised numerical results and downloads; light/dark themes, keyboard and mobile
navigation; memory-only token bootstrap; history selection/clear and receipt-cache disposal;
empty grants/invalid bearer; lost admission replies with same-ID reconciliation; real Git
status/diff/log and ripgrep listing/literal searches; nonzero tool results and inspection;
metadata refusal; text-only hostile output; cancellation and byte-exact download; clear preserving
active work; and a started FIFO read timing out with honest unconfirmed tool completion. The final
native fixture also verifies visible escaping of Unicode direction controls in complete JSON while
downloads retain the original contents.

Retained local logs are `target/ui/r2-controls-{red,green}.log`, `target/ui/r2-asset-build.log`,
`target/ui/r1-receipt-*.log`, and `/tmp/agenticsre-gui-diagnostics-{red,green,clippy}.log`.
Final local log hashes are in `/tmp/agenticsre-gui-r2-local-logs.json`; binary/source binding is in
`/tmp/agenticsre-gui-r2-local-evidence.json`. Coordinator live API evidence is
`/tmp/agenticsre-gui-r1-root-contract.log`. Builder/coordinator checks are not relabelled as
independent verifier execution.

## Corrections and earlier attempts

The initial integrated candidate was commit `8c44553a2f2f3a7dce8ab5df8d162740615c53e1`, archive
SHA-256 `8cbaca21a4f0f0509756875d84fb82def4de36f336776caa66165bea40b9be37`. Its independent full
regression run passed the Rust/native/public suites but stopped the browser after a successful
cancellation receipt because a test locator matched both a status badge and error heading.
The packet `/tmp/agenticsre-gui-verify-5ew3zhrn/packet.txt` has SHA-256
`28ff1ae772e60d67af2034e30707ed90f5e8ab86ca5d0a3ab9523d68ce902ae0`.

Correctness review identified three P2 issues: JavaScript receipt reserialization could round
large inode integers; reopened diagnostic writers could overwrite a redirected log; and the
browser timeout assertion incorrectly required confirmed tool completion after termination.
R1 preserves original receipt text, appends through independent nonblocking diagnostic descriptors,
and uses the accepted honest FIFO timeout semantics. Meaningful regressions failed before the
receipt/log fixes and passed after them. The status locator now selects the result header.

R1 was commit `365928a22609da8ee05036f2d5814aa27f63c15f`, archive SHA-256
`ff34b4d5c74951762caa26a2e692ae888680f501079626f875e471d349dfa3cc`. Review then found one display
regression: the raw JSON view bypassed existing direction-control escaping. R2 restores that
escaping only for display, leaving downloaded bytes untouched, with failing-then-passing coverage.
R1's active command completed all focused tests before its requested withdrawal took effect;
controller exit 143 and those observations are retained in
`/tmp/agenticsre-gui-r1-verify-i90scy0m/packet.txt`. R1 received no acceptance verdict.

Two product review/fix rounds resolved all findings. The dedicated security auditor independently
reviewed the whole GUI boundary and later affected paths; the R1 display finding was the same root
cause reported by correctness review and is counted once. No scanner or external advisory campaign
is claimed.

## Execution boundary and remaining limits

Cally used the pinned Debian/Rust base and a COPY-only image, root-owned read-only source,
UID/GID 1000, no host mounts or runtime network, dropped capabilities, no privilege gain,
2.5 CPUs, 4 GiB, 512 PIDs and a 900-second ceiling. Private `/tmp` and build tmpfs remain bounded.
Cached Python/Playwright/Chromium files were staged with recorded hashes; no browser download or
host package installation occurred. Runtime provenance is separate from product source identity.
Each run removes only its uniquely named container, derived image and disposable local staging,
retaining source archives, logs and bounded browser artifacts.

The GUI supports one active check and session-only history (50 completed receipts, 16 MiB).
It provides no AI chat, durable history, team access or arbitrary shell form. Browser evidence
covers Chromium on Linux; native Windows, other browsers, live targets, installation as a service
and production adoption remain separate work.

The runner's actual namespace-setup-failure criterion remains open because the standard control
cannot change the read-only container `/proc/sys`. Positive native execution and other refusal
tests do not close it. See [restricted-profile verification](read-profile-verification.md).
No host/container protections were weakened to obtain a green test.
