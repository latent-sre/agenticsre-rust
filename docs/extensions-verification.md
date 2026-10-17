# Extension package management acceptance — 2026-10-11

**Verdict:** PASS for WORKBENCH-021's Linux core/CLI package-management foundation. This covers
the inert inspection/installation portion of CAP-17/AC-26 through the
[package contract](extensions.md). External execution, independent enablement policy, protocol
conformance and the fourth-capability experiment remain unfinished; this is not overall CAP-17
or AC-29 acceptance.

Final frozen source: 1342-file manifest SHA-256
`ca3d0274e5afa62acdd08d31dad099d31cb4bf822270a4e8fcc8d937ea0dd389`.
Ordinary CLI binary SHA-256:
`5a8a0f9a45b96d79572072919526b4523049f153c04d09e7c7861cba69be28be`.
Adversarial native fixture SHA-256:
`3b6d6da9a87cddd5e2eb8200623804a55ac6f73a42e18eb9129392a4c0d37ce1`.
The local working tree remains uncommitted. Acceptance documentation follows the freeze without
changing tested product bytes; no package or executable was installed outside disposable tests.

The packet retains the [installed verdict](evidence/extensions-2026-10-11/verification.json),
[source manifest](evidence/extensions-2026-10-11/source-manifest.json),
[check record](evidence/extensions-2026-10-11/checks.json),
[workspace output](evidence/extensions-2026-10-11/workspace-r1.log),
[disk-full residual identity](evidence/extensions-2026-10-11/disk-full-residual.json) and
[artifact hashes](evidence/extensions-2026-10-11/artifact-sha256.json).
All 28 installed commands' stdout/stderr remain under the packet's `commands/` directory.

## Demonstrated behavior

| Requirement or risk | Evidence |
|---|---|
| Inspection and installation are inert | A compiled native adversarial executable would write a marker if run. Inspection, installation, repeat installation and listing leave it absent. No executable payload is materialized as an executable file. Installed metadata remains disabled, unavailable for execution and provenance-unverified. Dispatch to its declared capability refuses. |
| Identity and compatibility | Package and embedded executable digests match consumed bytes. Wrong pins refuse before store creation; duplicate JSON keys, malformed schemas, unsupported protocol declarations, access requests, hooks and payload mismatches refuse. A changed package with the same ID/version cannot replace an installation. |
| Filesystem integrity | Source and parent symlinks, hardlinks, FIFOs, directories and oversized packages refuse. Stores are private with nonexecutable 0600 files; inventory revalidates every retained package and filename digest. Installed-byte changes are detected and left untouched. |
| Atomic publication and failure evidence | Successful publication uses synced bytes and atomic no-replace rename. Injected interruption tests retain pending data and refuse subsequent inventory. Real 64 KiB private-tmpfs exhaustion leaves a 61,440-byte pending file, returns failed with unknown effects, publishes no package and preserves all residual bytes during a later refusal. |
| Quotas, deadlines and output bounds | Core tests install 32 packages, refuse the next without eviction, and retain inventory counts. Metadata above 1 MiB is omitted under the reported effective 1 MiB limit even when 2 MiB is requested. A one-millisecond installed preparation budget yields `timed_out`; bounded listing is partial with explicit omissions. |
| Human and structured interfaces | An ordinary installed copy runs outside the checkout. Human output labels disabled execution and unverified provenance; JSON receipts/data validate. Structured inspection returns the same metadata and digest. Source/package/binary guards remain unchanged. Hosted grant and startup paths refuse this operator-local configuration. |

## Verification and resolved findings

The full sandbox workspace passed **402 tests, zero failures**, with 22 existing native/fixture
cases ignored. It includes five public CLI tests, eight public core tests and two internal
interruption tests for the new boundary. Strict workspace lint, formatting, ordinary build,
whitespace and specification checks passed. The specification suite covered 65 schemas and
44 positive/33 negative examples. No new dependency was added.

Independent static review found two material issues: the installation effect used an unsupported
canonical value, and large inventories could exceed the receipt's reported effective limit.
Both were corrected; the reviewer confirmed closure and reported no remaining material findings
and no P0/P1 findings. The large-inventory regression uses actual installed packages rather than
a mocked receipt. The reviewer did not execute tests; the coordinator ran integrated verification.

Initial focused runs exposed three test defects: unsupported SHA-array hex formatting, an
unsuffixed large integer literal and an array-input expectation that overlooked common-envelope
validation. Their failing logs are retained. Corrected tests passed in the full suite. Initial
installed acceptance passed 26 commands; the final driver added two real disk-full observations
and passed all 28 with the same product binary.

Recovery currently means explicit refusal and evidence preservation. Automated restoration,
signing/revocation, live distribution, non-Linux support and protection from the same account/root
are not established. The fixture intentionally lacks a working external protocol and is not
evidence for the planned capacity-projection capability. Those requirements remain on
[the roadmap](roadmap.md).
