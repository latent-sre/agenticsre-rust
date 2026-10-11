# Evidence collection and store acceptance

WORKBENCH-013 is complete for the Linux core/CLI fixture increment. It executes the fixed
Prometheus/Loki pack through fresh context bindings and retains finalized sanitized receipts in
local evidence format 1. Comparison is the next roadmap item; this record does not establish
live Grafana compatibility, native Windows support, hosted grants or release readiness.

## Tested identities

The final installed mission used snapshot `target/evidence-collection/review-r4`, 423 source files,
with [manifest](evidence/evidence-collection-2026-10-10/source-manifest-r4.json) SHA-256
`df9ad3800f2d25eaa679d6ba30c39153b47543f6265df7b035b70156eb5a7be1`.
The ordinary CLI binary is 40,609,208 bytes, SHA-256
`c450790c52de7a7d84b200de2bf76f7953f78f417320969248b126eb7503eba2`.
A readonly DATA copy remains at `target/evidence-collection/save-r3.data`.

The [build provenance](evidence/evidence-collection-2026-10-10/build-provenance-r3.json) records
Rust/Cargo 1.98.0 and the exact commands, exits and logs. Build/tests and the corrected product
review bind snapshot r3, SHA-256
`f88e334bafbea920fd6f693bf6e87f146365e1eb3ed8f9af3058c3885568d173`.
R4 changes only a verifier assertion; the independent executor checked that delta and reused the
same binary. Subsequent acceptance documentation does not alter these tested product bytes.
All implementation remains local and uncommitted.

## Verification

| Check | Result |
|---|---|
| Locked offline workspace tests in the existing sandbox | 260 passed, 0 failed; 19 existing native-environment tests explicitly ignored |
| All-target/all-feature clippy with warnings denied | Passed |
| Formatting, ordinary CLI build and specification checks | Passed; 36 schemas, 31 positive and 16 negative examples |
| Independent static correctness/security review | Four product P2 findings resolved; no further concrete product finding in the correction delta |
| Independent installed mission | 23 command outcomes, 34 HTTPS requests, 32 canonical receipts, 10 strict query receipts, 5 collection receipts and 5 manifests |
| Real disk exhaustion | Private 64 KiB tmpfs returned ENOSPC; the prior receipt remained valid and no bundle was claimed |
| Frozen source, binary and worker lifecycle | Before/after identities matched; all fixture workers joined |

The [verification summary](evidence/evidence-collection-2026-10-10/verification-summary.json),
[independent verdict](evidence/evidence-collection-2026-10-10/installed-r4/independent-verdict.json),
[source review record](evidence/evidence-collection-2026-10-10/review-summary.json) and
[artifact manifest](evidence/evidence-collection-2026-10-10/artifact-manifest.json) retain the evidence.
The artifact manifest has SHA-256
`47c4a6ffa161a09949f058950c1b1bb98e0da17a91430b45ae75b70e3a875f44`.

Coverage includes real shared-window dispatch; strict pre-effect input/context/org/provider/CA
admission; finalized receipt and manifest integrity; private credential canaries; partial/stop
outcomes; timeout, signals and consumer loss; owner-private modes and no-follow access; quotas,
locking, corrupt/future-version refusal; protection and retention; interrupted collection recovery
without replay; and digest-pinned deletion retry on both sides of directory removal. Store tests
inject write, file-sync, publish and directory-sync failures. These injected checks are distinct
from the separately executed real ENOSPC case.

Review added receipt-depth boundary coverage, consistent domain validation across bulk operations,
cancellation with confirmed management progress, and immutable sealed payloads with separate
delivery cancellation metadata. A subsequently reproduced timing race let a child finish less
than one millisecond before its parent's deadline and lose receipt finalization time. The fix
enters one capped cleanup phase; the focused test passed 21 times afterward, with deterministic
tests for rounding and the non-resetting 2.5-second allowance. Earlier failures remain in the packet.
Two verifier defects—a provider-name prefix and a second terminal-field projection—were corrected
without changing the then-verified product binary.

## Scope and reproduction

Use the [collection commands](development.md#evidence-collection-and-local-store) and
[contract](evidence.md). The installed checker requires explicit source-manifest and binary
digests; consult `tools/verify-evidence.py --help` for its arguments. Run it inside
`tools/test-sandbox.sh --small-evidence-disk` with `--enospc-root /tmp/evidence-small-disk`
to include the bounded disk-full mission. The checker copies the ordinary binary into a private
prefix and invokes it from an unrelated working directory.

All network tests used private namespaces, synthetic HTTPS certificates and synthetic providers.
No lab service, firewall, credential account, installation, agent registration or publication was
changed. Existing native/browser evidence carries for unchanged behavior; the inherited positive
system-resolver fixture gap remains open. Synchronous filesystem calls cannot forcibly preempt
uninterruptible kernel I/O. Store hashes establish byte integrity, not source truth or authorship.

The owner selected the home lab for live acceptance. Dradis Grafana is identified in local lab
configuration; a suitable scoped read identity and a confirmed native Windows test lane remain
prerequisites. Those checks cannot be inferred from this fixture increment.
