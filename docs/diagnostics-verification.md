# Configured diagnostic pack verification

Executable acceptance is **qualified PASS for the required diagnostic mission and affected
Grafana regression in the tested private Linux boundary**. Independent correctness and security
reviews accept the corrected R1 source. The completed native continuations close the required
HTTP/body, process, cancellation, consumer-loss and regression criteria. Positive system-resolver
first-server/no-fallback traffic remains **INCONCLUSIVE**: UID 1000 could not bind private port 53.
That specific fixture gap is excluded from the pass. Earlier failed attempts remain preserved
below with their original attribution. Current execution status belongs in
[the roadmap](roadmap.md#workbench-010--configured-service-diagnostic-pack).
The binding behavior is in [the diagnostic contract](diagnostics.md) and
[the configuration/receive profile](diagnostics-config.md).

## Exact source and binaries

- Sealed snapshot: `/tmp/agenticsre-diagnostics-r1-snapshot-oknmjwv0`.
- Candidate commit: `0e7ec5388bcc96def61571b500e8159deea00621`.
- Tree: `d9d64fc09a80f27a327718fa7892fba927c9d688`.
- Base: `2c4e4220654d85d5be22ac42a034beb51fcc5624`.
- Complete source manifest: `/tmp/agenticsre-diagnostics-r1-snapshot.json`, SHA-256
  `c2a4100c4f209575e7f47e58a45e5df551e3a03824fff4192a395df9fc9416ce`:
  340 regular files, 4,003,181 bytes.
- Local ordinary binary: `target/diagnostics/save-r1-candidate`, SHA-256
  `fcb5a72bd48f3c7faa69cc0ab99b0b18cfb3463fac453c469b3d6401745d95d3`, 38,975,168 bytes.
- Cally ordinary binary: `/tmp/agenticsre-diagnostics-cally-rb6dntnd/save`, SHA-256
  `b33d7f2dc713cfb4e2682d6db48d9a2c91a45771c0412c126ceda92480674eea`, 38,908,104 bytes.

Both retained binaries are non-executable DATA. The two builds have separate runtime identities;
no reproducible-binary claim is made. The immutable candidate is a disposable capture, not a
source commit or release. The merged-main commit `9f77f8a9c24b0c83aa9a574e831751fa7e84a17f`
has the same file tree as the recorded base, so its history change does not change these product
bytes. Subsequent documentation is outside the frozen executable source.

## Reviewed and locally verified criteria

| Criterion | Evidence and scope |
|---|---|
| Independent correctness | ACCEPT on exact R1; no reportable findings, independent P0/P1 count 0; affected implementation, locked dependencies, contracts and immediate callers inspected |
| Independent security | ACCEPT on exact R1; no remaining validated findings or unresolved P0/P1 candidates; affected DNS/HTTP source-to-sink paths and immediate authority/lifecycle seams inspected |
| Local workspace | 167 Rust tests passed, 19 existing native tests ignored; Clippy, format and whitespace gates exited 0 |
| Local public checks | 63 exact receipts and 38 focused DNS/HTTP/alias-deadline witnesses passed; private copied installation, real DNS/HTTPS/process fixtures and joined cleanup |
| Local specification | 29 schemas, 27 positive and 12 negative examples; documentation/ID structure checked |

The local builder packet is `/tmp/agenticsre-diagnostics-r1-build.31pxy06M/PACKET.md`, SHA-256
`9465b11dc86387f2e062b7e8fda6911d1ae402a4ca6d112c8046bfad51791922`.
It preserves the R0 red witnesses, exact commands/own exits, source hashes and development failures.
Static reviews supply no execution PASS. Root's two focused sandbox checks are spot checks and
are not relabelled as independent acceptance.

## First independent Cally attempt

The verifier packet is [the retained partial record](evidence/diagnostics-cally-attempt1-verification-2026-10-03.txt),
copied byte-for-byte from `/tmp/agenticsre-diagnostics-cally-rb6dntnd/VERIFICATION.md`, SHA-256
`6d4a91fdc9760bd7287cb06ccc5a33e4d76c12677c99019e9683a8aaeac9bda5`.
Its verdict is **INCONCLUSIVE product acceptance; FAIL verifier execution**.

| Criterion | Actual result |
|---|---|
| Frozen source/boundary | PASS: separate copy matched all source bytes and the pinned cached/derived image |
| Cold workspace/native grant | PASS: 167 tests, 19 existing ignored; one explicitly selected native MCP grant case |
| Installed mission/parity | PASS: literal printf mission plus direct/structured five-check calls, exact config/CA/leaf/process identities and actual private DNS/HTTPS ledgers |
| Independent completed portion | PASS: 114 receipts, including all 33 R1 DNS/framing cases, literal IPv4/IPv6 and admission/authority negatives |
| Returned schemas | PASS: all 117 exact canonical receipts and 51 nonempty diagnostic data objects validated against frozen schemas |
| Expired certificate | Not executed: OpenSSL rejected fixture creation with `Error: end date before start date` for `x509 -days -1` |
| Remaining public criteria | Not reached: broader HTTP/body, process-state negatives, signals/consumer loss, supplemental resolver/proc views and Grafana regression |
| Internal verifier residue | FAIL: 19 adopted zombie-only descendants remained under the Python PID 1 before teardown; no live state was observed |
| Final owned cleanup | PASS after reconciliation: exact container/name/image-ID absent, cached base unchanged, controller terminal, only owned local staging removed, sealed sources unchanged |

The [117 exact receipt records](evidence/diagnostics-cally-attempt1-receipts-2026-10-03.jsonl)
are copied unchanged; the verifier's schema-validation record remains in the sealed packet.
Root matched all 79 retained evidence files. The packet's artifact manifest SHA-256 is
`ad3bbc93d5c22ec2aa9459e4f0a4e689e0f31280da70ef67eed953722f869a23`.
Original commands, failed creation output, socket/kernel ledgers and cleanup records remain under
that packet path. Passed partial evidence does not close the unexecuted criteria.

## Artifact continuation and certificate diagnosis

The [second native packet](evidence/diagnostics-cally-attempt2-verification-2026-10-03.txt)
is copied unchanged from `/tmp/agenticsre-diagnostics-cont-cally-nirie7sk/VERIFICATION.md`,
SHA-256 `2474ea20d6178df52ffada5c20bf88b2e53159e0ab295a5b54d1ef44d6a3c8d6`.
Root matched all 65 artifact files; its manifest SHA-256 is
`242fc4820b3f0c5fb1afd4e2f32c2f37eb94d3a3c16470f02f67988a6814662d`.
It reused the exact ordinary Cally binary without Cargo, vendoring or replaying passed criteria.
The ordered 2020–2021 expiry fixture and same-key checks passed, but its time-disabled chain/name
self-check failed with OpenSSL error 79 at CA depth 1. No new product invocation or receipt
occurred; overall acceptance remained INCONCLUSIVE.

The actual PID 1 adoption, zombie-state and `waitpid` proof passed in that continuation, with an
empty final process inventory. Exact container/tag/image-ID and owned copied-directory cleanup
also passed. These results resolve the private lifecycle defects while preserving the first
attempt's original failures.

A separate standard Python/OpenSSL fixture-only diagnosis uses the same cached native runtime
and container bounds, with a 120-second ceiling and no product binary or build. Its
[initial instrumentation failure](evidence/diagnostics-ca-instrumentation-failure-2026-10-03.txt)
occurred before any CA command: a global subprocess logger intercepted an internal platform
query. The correction scopes logging to the two fixture modules and reads kernel provenance
with `os.uname()`. This changes neither the CA commands nor the product.

The [sealed corrected fixture proof](evidence/diagnostics-ca-fixture-verification-2026-10-03.txt)
is copied unchanged, SHA-256
`163212bb918ac1b50aec8451b0f713ebf40be423eda268d4195ada15ee08e9fe`.
Root matched all 42 artifact files; manifest SHA-256 is
`80fc75dd6eb12e692afe8274b210ec43402c08dd8c664d4e1ff9842f9b499574`.
The corrected native fixture proof executed 44 OpenSSL commands and no product invocation.
It reproduced the unextended version-3 CA's absent Basic Constraints/Key Usage and error 79,
then passed with explicit critical `CA:TRUE` and `keyCertSign,cRLSign`. Normal current CA/leaf
chain, purpose and hostname checks passed; a wrong hostname failed with error 62 at depth 0.
The same-CA/key expired leaf passed time-disabled chain/name verification and failed normal
verification solely with error 10 at leaf depth 0. Actual public certificate bytes, commands and
extension facts are retained at `/tmp/agenticsre-diagnostics-ca-proof-cally-t_kyyd89` and copied
unchanged in [the native observations](evidence/diagnostics-ca-fixture-observations-2026-10-03.json),
SHA-256 `fd8a3d5fe61beb6791eb871d00c5c4e68aca871bdd0957ced62504072daafa6f`.
Root independently matched all six public PEM/DER digests. Final residue was empty, and independent
container/tag/image-ID and cached-base readbacks confirmed cleanup; its record SHA-256 is
`06b1b2472a3b0142b2ac8c4467aa7914044f549f45773221e020c86036bb5bd1`.
This fixture-only proof did not establish product expiry or other diagnostic criteria; the
subsequent product continuations below supply those results.

## Expiry and HTTP parsing continuation

The [third native packet](evidence/diagnostics-cally-attempt3-verification-2026-10-03.txt)
is copied unchanged, SHA-256
`844a4d7007147ce9406093692692cf99fd0e70761ad1c68f0b03a30b6ca5cbb2`.
Root matched all 70 artifact files; its manifest SHA-256 is
`34097e658c842ec29394b945a0059368e97bd7c9165c5acb2eb88e48dd05a73f`.
The same binary actually refused the valid-chain/name expired leaf and passed 27 HTTP parsing
failure cases. These include duplicate/conflicting framing, incomplete Content-Length/chunks,
metadata limits, unsupported encodings, informational responses and malformed headers/version.

The private checker stopped after requesting body capture for a 301 response and then incorrectly
asserting null body content. The actual result follows the frozen profile: failed HTTP observation,
four opt-in untrusted bytes in lossless base64, no redirect/proxy/retry, exactly one GET and no
denied-endpoint effects. This is a verifier expectation mismatch, not a product defect. The
original red assertion and exact result/ledger remain preserved.

All [29 new exact receipts](evidence/diagnostics-cally-attempt3-receipts-2026-10-03.jsonl)
and all 29 diagnostic data values passed their frozen schemas. Their validation record SHA-256 is
`55b4a0c03fddd5b2c86d0d21d301f4553a5b633ee1596fa51727b0348c828415`.
Combined with the first attempt, 146 canonical receipts and 80 diagnostic data values are retained.
Final process residue was empty, and independent exact container/tag/image-ID, cached-base,
staging and unchanged-source readbacks passed. Cleanup record SHA-256 is
`9890aee29dc8f10eb5e20fc614cac8574829a068d847aec3c5847741ab84b76a`.

At this attempt's boundary, body/status, process, signal/consumer, resolver/proc-view and Grafana
criteria were still open. The following continuations corrected the private expectation and
checked the remaining assertions against the binding contract without replaying passed criteria.

## Completed diagnostic suffix

The [retained suffix packet](evidence/diagnostics-cally-tail-verification-2026-10-03.txt), SHA-256
`26c74dc2ec70eeec047062de5b86bc501082c61c3bb277d7d96ce6bc41f6ac26`, preserves the controller's
terminal exit 1 and the main diagnostic checker's separate **exit 0**. Its 28 public invocations
produced [27 exact receipts](evidence/diagnostics-cally-tail-receipts-2026-10-03.jsonl), SHA-256
`e4c0dbb796537e89417180e569a1572a987dec2f95a8ac5eb1056a404a2dc880`; the closed-consumer case
deliberately could not deliver a receipt.

The executed checks passed valid framing, opaque opt-in body capture and body limits, non-2xx
capture on/off, wrong process UID/start ticks, stopped/zombie/gone/optional process outcomes,
signals during actual DNS/TLS/header/body phases, an actual body-phase parent deadline and closed
result-consumer cleanup. Kernel identity/gone witnesses and no-later-dispatch assertions accompany
the signal results. Actual inaccessible metadata and readable non-proc copies were refused;
genuine proc-descriptor configuration admission and the private namespace boundary were verified.
All 27 canonical receipts and diagnostic data values passed their frozen schemas.

The subsequent Grafana checker exceeded the private observer's old 128 KiB line limit. Its
145,925-byte partial fragment is retained and uncounted. The first cleanup returned 125 and masked
the primary observer error; final cleanup reconciled the owned container/image removal. This
verifier failure did not invalidate the completed diagnostic suffix or establish a product defect.

## Final integrated acceptance

The [final independent packet](evidence/diagnostics-cally-final-verification-2026-10-03.txt)
is copied unchanged from `/tmp/agenticsre-diagnostics-cont-cally-i5soqfo6/VERIFICATION.md`, SHA-256
`685da9ea4aa1b038b01966b90052cf666b484162ac9a61e3f46d02920379884b`. Root matched all 70 final
artifact hashes; manifest SHA-256 is
`59d99a3d33ede3f771eab03aae449ddad13b11924e04fb1e9101fafeec5a309b`.
The ordinary Cally binary and all frozen product bytes were reused unchanged, with no rebuild.

| Criterion | Final evidence |
|---|---|
| Required diagnostic mission | PASS using the carried first/expiry/suffix results on the exact R1 binary |
| Malformed/empty system resolver | PASS: two actual private readonly views, no port-53 listener, unchanged IPv4/IPv6 UDP-send counters, no DNS/service effects |
| Affected Grafana regression | PASS: full checker own exit 0, 96 canonical results, 56 dashboard-data and 33 query-data values |
| Final controller and identity | PASS: handle 76710 own exit 0, source/binary postchecks, exact DATA export and empty native process inventory |
| Exact returned schemas | PASS: final raw log fully captured and externally validated against frozen schemas without code import or external reference fetch |
| Final owned cleanup | PASS: exact container/tag/image-ID absent, cached base unchanged, source/prior packets unchanged and no owned job, controller or staging residue |
| Positive system resolver | INCONCLUSIVE: actual private port-53 bind returned EACCES 13; first-server/no-fallback traffic was not exercised |

The bounded observer repair derives a 6,295,552-byte line cap from the frozen 2 MiB UTF-8 envelope
and JSON ASCII-escape expansion, retaining the 16 MiB total cap, fixed export parser, barrier and
cleanup bounds. Preparation passed nine observer DATA checks, 21 other DATA checks and 19 fixed
export-parser checks. The native green then captured the full 8,724,397-byte log; schema validation
exited 0, with record SHA-256
`d54b434c5ca22c5fc881b63014b6cdf60c74f86a242ebf0e9a282b99e6eead1f`.
Primary exceptions and cleanup failures are now recorded separately. Earlier failures are not
rewritten or counted as successful full-regression coverage.

The unchanged [accepted evidence index](evidence/diagnostics-cally-accepted-index-2026-10-03.json),
SHA-256 `cdd2813a9da2dde9c52dd77001ca64efe5f37154d31699d0a55911d72102edd6`, selects 271 canonical
receipts: 175 baseline/diagnostic/resolver observations and 96 from the completed Grafana checker.
It also identifies 109 diagnostic-data and 89 completed-Grafana data values. Earlier interrupted
Grafana records remain separately retained; including their 87 complete canonical observations
yields 358 historical observations, not 358 accepted full-regression cases.
The [two exact final resolver receipts](evidence/diagnostics-cally-final-resolver-receipts-2026-10-03.jsonl)
are copied unchanged, SHA-256
`1896dade0b74289f5c2fee6d62f767fdfc140bdc8761ee70bc13a2e24a234e87`.
The larger exact Grafana record file remains under the final packet's `schema-validation/`,
SHA-256 `caf68dc1a8e32b21bcd65772ea575931e2f71b8ba0e4127955e4ef94c2bc3dc1`.

Final independent cleanup readback SHA-256 is
`9f282a80253916b8ea894c887780935a47188e3e75a3714129e5f4e0876edc18`.
The unchanged binary was exported as mode-0444 DATA, and all 340 frozen source files plus all six
earlier packet manifests/bytes matched. Root's separately recorded consolidation of identical
readonly DATA copies preserves paths, bytes, sizes and modes while explicitly recording changed
inode/link-count/ctime/shared timestamps. No unique source, archive, receipt or failed evidence
was removed.

## Execution boundary and remaining limits

All product/fixture execution used the supplied private local sandbox or bounded Cally lane.
Cally used the pinned Rust/Debian image, COPY-only staging, UID/GID 1000, no host mounts or
credentials, private PID/network, no runtime external network, read-only source/root, dropped
capabilities and no privilege gain. Bounds were 2.5 CPUs, 4 GiB memory, 512 PIDs, 900 seconds,
512 MiB private temporary storage and 2 GiB build tmpfs. Cargo used two jobs/test threads,
debug symbols off, incremental off and locked/offline dependencies. No package download occurred.

Continuations used the same verified Cally binary, corrected private CA profile, child reaping and
owned copied-directory cleanup. Their outer ceiling was 420 seconds; the final remaining-only lane
bounded Grafana to 90 seconds and invalid-resolver controls to 25 seconds. Completed cold
build/tests and exact returned records retain their original attempt attribution.

This record accepts WORKBENCH-010 core/CLI bytes, not the subsequently edited WORKBENCH-011 GUI
candidate. No result here establishes native Windows, live endpoints, systemd/service health,
GUI/MCP diagnostic grants, durable history or release installation. Synchronous pinned regular/proc reads
cannot guarantee hard preemption of an uninterruptible kernel/filesystem call. Unsupported
fixture prerequisites remain specific gaps; no isolation protection is weakened to obtain a pass.
