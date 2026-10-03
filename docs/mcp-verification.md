# Bounded stdio MCP verification

The Linux fixture increment is accepted: launcher-granted command/task/Grafana tools use the
shared core, with two explicit protocol revisions and bounded transport/cleanup. Independent
correctness and security reviews approve the product source. The remaining independent R2c
native lane passed, retaining earlier unchanged coverage. Actual host registration, live targets,
Windows and the existing restricted-profile namespace-refusal gap remain open.

The [MCP contract](mcp.md) defines the five tools, grants, transport and lifecycle limits. It
supports modern `2026-07-28` and legacy `2025-11-25`; compatibility with an installed host is a
separate acceptance requirement. Tool outputs contain the unchanged core receipt, with a short
summary and honest error mapping. No model campaign, HTTP MCP listener or persistence is added.

## Source and binary identities

| Candidate | Exact identity and role |
|---|---|
| Product R2 | Commit `191c5f55e458eb5deecb4e6bc07407ce0281410e`, tree `786ae372082a2899bb9a6489d15b176a899a1451`; approved production source |
| Test/tooling R2b | Commit `55ba7961421ed8c8e481e8730807595deaf22771`, tree `8b672fb6ee63f23991529812b9e2913ef27005e0`; corrected native test and ordinary-build ordering, product unchanged |
| Acceptance R2c | Commit `8b6d6224becda431999bf27c75cf8621db2d5eca`, tree `c4a6105f95dea22315037a46022b35468b336e00`; corrected parity client, product unchanged |

The final frozen snapshot is `/tmp/agenticsre-mcp-r2c-snapshot-w2111tqx`. Its 274-file capture
manifest `/tmp/agenticsre-mcp-r2c-snapshot.json` has SHA-256
`be0e716b264bbb5274dcbe7bd54555d6d21dbd54563055c7506dd95262441661`.
Independent comparisons checked every captured hash and confirmed all product files unchanged
from approved R2. These disposable commits identify verification bytes, not release versions.
Subsequent status/packaging documents are outside this executable snapshot.

The Cally ordinary CLI used by public checks in R2, R2b and R2c has SHA-256
`939e5f5b5d2bccea2630df6d4870b12a18e2d72f3e834531195123965169aa56`, size 36,887,184 bytes.
It was built after any feature-enabled tests. The disposable Cally binary was not retained.
The separate local R2 binary is retained at `target/mcp-evidence/save-r2`, SHA-256
`b74516725a023fb1451b28afc5d321cf6f16b8d846481a03018a69f91af744b0`; separate build identity is
not a reproducible-binary claim.

## Independent evidence

| Check | Result and attribution |
|---|---|
| Correctness review | APPROVE on R2 and bounded R2b/R2c tooling deltas; all source findings resolved |
| Dedicated security audit | APPROVE on R2; unchanged product authority/mapping/lifetime coverage retained |
| Workspace regressions | R2: 140 passed, 19 explicitly ignored |
| Native restricted profile | R2: CLI target 19 passed and core filtered target 10 passed, including the existing 13 CLI/four core native proofs |
| Public profile/Grafana regressions | R2: 51 exact profile receipts schema-valid; 96 public Grafana runtime cases passed with native schema checks disabled |
| Native MCP integration | R2b: nine passed, none ignored, including real Git/ripgrep and pinned policy |
| Independent raw protocol | R2b: 23 groups passed, covering both revisions, grants, numerical parity, malformed input, descriptor preservation and transport bounds |
| Parity-client regression | R2c: three tests passed; timestamp variation excluded, semantic changes retained, original receipt unchanged |
| Native command client | R2c: 15 checks, 16 exact receipts; all six rg/FIFO lifecycle cases passed with PID/start-time witnesses |
| HTTPS MCP client | R2c: 23 checks, 15 exact receipts, including malformed cancellation metadata and stdout saturation after HTTP dispatch |
| Exact schemas | R2c: all 31 exported command/HTTPS receipts and 20 advertised Grafana tool observations validated against the frozen schemas |
| Product/process cleanup | R2c: final surviving-product-process list empty; exact container/image absence and staging removal confirmed |

R2c's independent verdict is **PASS for the remaining scoped MCP acceptance**. Packet:
`/tmp/agenticsre-mcp-r2-cally-2xz83ozx/VERIFICATION.md`, SHA-256
`a823b62b89ad35214292ef4cd6af8d59330113cfc9c831caeec1c591abf08051`.
Its `artifact-digests.json` binds 29 sealed records, with SHA-256
`31cc16be89aefc23dfbc0b670000941440b27964b36281c96ee960ff3921fc4e`.
The packet attributes retained R2/R2b checks to their own immutable revisions. No entire-suite
rerun is claimed on R2c. Current lint was already passed on unchanged Rust bytes; the comparator
regressions were demonstrated failing before correction and passing after.

## Corrections and failed attempts

R0 review found cleanup uncertainty discarded at the worker boundary, an unbounded active worker
when stdout filled without a queued reply, and malformed notification metadata changing state.
R1 addressed those mechanisms. Review then found malformed traffic could still enqueue errors
and renew shutdown after cleanup uncertainty. R2 closes terminal admission before ordinary
parsing and latches one absolute drain deadline. Five cases failed before correction; all seven
terminal-drain regressions passed afterward, including paced input and partial pipe delivery.
Two product review/fix rounds resolved all findings.

R1 independent execution stopped at a real environment prerequisite: the pinned Cally image has
no Clippy component. The documented local lint/native-runtime split was restored; no packages
were installed and no lint gate was skipped.

R2 native MCP testing correctly denied a test fixture that omitted the restricted grammar's
required `-e` pattern option. R2b fixes only that fixture and runs feature-enabled tests before
restoring the ordinary CLI. Its command client then compared separate observation timestamps.
The exact receipt pair showed only `sources[0].observed_at` differed after existing exclusions.
R2c excludes that volatile field without altering receipts or other comparisons. Both failed
verification packets remain retained; neither is relabelled as a passing run.

## Boundary and remaining limits

All native builds/product execution ran in Cally's cached Rust 1.98/Debian 13 image using root-owned
read-only source, UID/GID 1000, no host mounts/runtime network/credentials, dropped capabilities,
no privilege gain, 2.5 CPUs, 4 GiB, 512 PIDs and a 900-second ceiling. Private scratch and build
tmpfs were bounded. Host-side staging inspected/copied/hash-verified data only; no source build
scripts or product listeners ran on Adama. Containers, derived images and disposable extraction
were removed while source snapshots, logs and evidence were retained.

Actual namespace-setup-refusal remains inconclusive under the unchanged read-only `/proc/sys`
container prerequisite. Positive execution cannot close that negative criterion. See
[restricted-profile verification](read-profile-verification.md). Installed MCP-host behavior,
live Grafana/API compatibility, credentials and protected remote identity still require their
separate explicitly configured acceptance.
