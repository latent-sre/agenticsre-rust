# Browser diagnostics acceptance

WORKBENCH-011 passed independent correctness, security and executable acceptance for the
explicit-resolver Linux fixture scope. It exposes the real connectivity pack through an explicitly
granted browser target, preserves its exact receipt in session history/download and cancels
witnessed active network work. Current execution and pause status belongs in [the roadmap](roadmap.md).
The [browser contract](diagnostics-ui.md) and [core acceptance](diagnostics-verification.md) retain
their separate boundaries.

## Source and binary

The accepted readonly snapshot is `/tmp/agenticsre-diagnostics-ui-r1-snapshot-0rzjumlc`, commit
`bca5b19a8d448ac72202f30ff120446127582a38`, tree
`4f221ff5cad9fe78342e25c866a0258529e4622c`, based on `2c4e422`. Its manifest SHA-256 is
`0b942d654f4013ed59e3b9e895ca08ee108eb4bd3a1c6c1dea7a7fb08bf11477`: 358 regular files,
5,391,853 bytes. Subsequent planning and acceptance documentation is outside that frozen source;
no product source changed after its accepted gates.

The ordinary installed CLI has SHA-256
`566b320c424ecc583a846baeda4fccaa594ed264e6d122dbed3b321ec253344d`, 38,993,736 bytes.
It was built once on Cally with the locked offline dependency set. Local retained copies are
nonexecuting mode-0444 DATA; execution used a separate private-container copy.

Independent correctness and security accepted R1 with no remaining findings and P0/P1 count 0.
One correctness P2 was corrected: failed TCP/DNS observations use neutral attempted/observed
captions, and nonempty retained observations no longer claim no observation was returned.
The affected renderer and complete regenerated bundle were rechecked. Product fix/review round 1
is resolved; private verifier repairs did not change product source or add a product review round.

## Executed evidence

| Criterion | Independent result |
|---|---|
| Native workspace and selected MCP regression | 175 Rust tests passed, 19 existing tests ignored; one selected native MCP test passed |
| Ordinary build and installed command | Both exited 0; exact `hello workbench\n`, child 0, succeeded, not assessed, record never |
| Public API | All six groups exited 0: startup/discovery, admission, parity/deduplication, pinning, outcomes and lifecycle |
| Real Playwright browser | Exited 0: five observations, aliases, keyboard, light/dark/mobile, history/download equality, inert hostile content, partial/stop, active cancellation and refused TCP captions |
| Frozen schemas and exact bytes | 28 new canonical values, 20 diagnostic-data values, 110 API records and 25 exact receipt-byte records validated |
| Screenshot export | Nine actual PNGs passed fixed-name/type/count/size/hash/pixel checks and visual inspection |
| Final guards and cleanup | Source/browser/binary guards passed; owned fixture/worker and exact remote/local resources absent |

The seven browser receipts are included in the 28 new canonical values, rather than additional
unique runs. One carried literal receipt yields 29 integrated canonical records. Their unchanged
bytes are retained in [the integrated receipts](evidence/diagnostics-ui-cally-receipts-2026-10-03.jsonl),
SHA-256 `190336dcc3477b8340acd95e808f28294ea785ceb85cecfb2f5d57bdcb984b65`.

The full [independent verification packet](evidence/diagnostics-ui-cally-verification-2026-10-03.txt)
has SHA-256 `1ba15b9b7b82644b30cabc7dee12319d342ff677c064692daef63895ed095b88`.
[The accepted evidence index](evidence/diagnostics-ui-cally-accepted-index-2026-10-03.json), SHA-256
`94fec124e54266145b32d4df8efead8cd7c491c28da4db78496f8dd2c239356c`, identifies source, build,
commands, receipts, screenshots, guards and cleanup. Root matched all 77 sealed artifact hashes.

The final remaining-only controller, authoritative handle 97467, exited 0. It reused the same
verified binary and completed API/browser work without another Cargo build or passed mission
replay. The earlier handle 59978 supplies the successful native tests/build/literal prefix. Its
[private assertion failure](evidence/diagnostics-ui-cally-partial-verification-2026-10-03.txt) remains
recorded as inconclusive for the then-unexecuted UI criteria. The earlier
[staging failure](evidence/diagnostics-ui-cally-staging-failure-2026-10-03.txt) and late-image cleanup
race are also preserved. Neither failure was rewritten as successful execution.

## Execution boundary and limitations

All product, fixture and browser execution used a COPY-only image derived from the cached Rust
digest `620dbcd124499c59e2406d3741574b5c5838cf9eb9656f0c3a03948f79b02959` on Cally. The private
boundary retained UID 1000, readonly root/source, private network/PID, no host mounts, capabilities
0, no new privileges, 2.5 CPUs, 4 GiB memory and 512 PIDs. No downloads, host listeners, live
targets or production credentials were used. Standard remote build ownership/group quiescence
was established before owned-image cleanup; final independently checked resource absence and
cached-base identity passed.

The native browser artifacts remain under
`/tmp/agenticsre-diagnostics-ui-cally-jp3hu_hv/browser-artifacts/`. The complete browser archive is
1,914,880 bytes, SHA-256 `d733fd07058b4bbe9dd6a73f1382c76947699b69beab336b48f0eec6a780e836`.
Root also viewed the actual final light/mobile images. Visual evidence is scoped to the exercised
mission, rather than a comprehensive accessibility certification.

The inherited positive system-resolver fixture remains inconclusive because private UID-1000
port 53 binding failed with EACCES. It was excluded from this explicit-resolver mission and was
not retried. Linux fixture evidence does not establish Windows, live compatibility, protected
same-account identity, durable history, AI chat, or a generic MCP diagnostic grant. Observations
remain untrusted data and do not establish service health. Publication and live adoption retain
their own authority boundaries.
