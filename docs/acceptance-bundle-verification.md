# Linux acceptance candidate — 2026-10-19

WORKBENCH-036 packages the accepted browser replay executable with truthful operation-specific
recording and external-store compatibility metadata. The [retained packet](evidence/acceptance-bundle-2026-10-19/summary.json)
establishes local fixture acceptance on **Ubuntu 26.04.1 x86-64, glibc 2.43, kernel 7.0.0-38**.
The [human walkthrough](user-acceptance.md) is prepared; human acceptance and release clearance
remain open. The date uses the operator's America/Chicago calendar; execution receipts use UTC.

## Available artifacts

Artifacts are local build outputs, not committed release assets. Both archives were freshly
prepared for this mission; the prior archive is based on published MCP replay commit `96476b2`.
It is not an unavailable historical October 3 archive.

| Artifact | Local path | SHA-256 |
|---|---|---|
| Current archive, 54,494,208 bytes | `target/acceptance-bundle/current.tar` | `5f4c6d3bbea35ecc5ece6eccbe00fa7b2f7d76ab5ded5e3e0943119bcea06002` |
| Prior archive, 54,487,552 bytes | `target/acceptance-bundle/prior.tar` | `6beb6e6d5d86ae18d1404dc093e1b3b68f20f9c5c939b80c91612ab0346c3b75` |
| Current executable | archive `bin/save` | `7ed9a73b0d4a98fc6977232fef30f1530a2b3b352a3ed9aabfedc5f81db5a2f5` |
| Prior executable | archive `bin/save` | `256c3ae501b0879e0a1892f7dc6ce42d5bf87fc94366ce4fbe4df286e65105a5` |

The current source capture has 2,815 files, manifest SHA-256
`3b5c3d68b34c032c58608657df03bdf75c220d91f36de94e34d4d808c8609c93`.
It is a disposable local Git snapshot at `ac541d16488ad6368286f468d4c21df1093753f9`,
preserved in `target/acceptance-bundle/current-source.bundle`; its digest is in the packet.
This snapshot records uncommitted product work without mislabeling it as the published base commit.
Later documentation and evidence capture are outside that frozen source tree.

The prior source capture has 2,772 files at `96476b24ae100f2a20d56b66c9ad5532346b7031`;
its manifest SHA-256 is `8a410c2c04ba2be37d2d8d7e96e03fc1dd9a8d7595bf5a91227094ff595221ae`.
Both clean captures were built with Rust/Cargo 1.99.1 using
`tools/test-sandbox.sh cargo build -p workbench-cli --locked --offline`.
Both resulting executables match their previously accepted product binaries byte-for-byte.

## Verified behavior

- All **34 packaging regressions** pass, including legacy revision-1 archives, revision-2
  compatibility, altered installation receipts and mixed-version selection.
- All **nine installed groups** pass: corruption refusal before installation, fresh/repeated
  installation, literal arguments and both offline tasks away from the checkout, MCP numerical
  output, authenticated UI/embedded assets, explicit selection and rollback, and cleanup.
- Prior/current/rollback replay produces identical historical observations and projections.
  All **25 external evidence-store entries** retain their bytes and modes throughout.
- The separate unavailable-system-Python lane passes **four groups**. Discovery reports the
  missing runtime; both tasks return `missing_dependency`, `not_attempted` and no child execution.
  A private namespace masks the fixed runtime path; the host interpreter remains unchanged.
- **25 canonical receipts** and three offline-data payloads validate against their schemas.
  A second archive from identical inputs has the same digest. Both temporary installations and
  their processes are cleaned up; no global path, service or host-agent registration is created.

Independent static review found no actionable P0/P1/P2 issue in the packaging implementation,
regressions and contract. Its exact scope/hash is retained. The later installed verifier extension
and evidence/documentation updates are outside that static review; their executed results are above.
Initial packaging attempts correctly refused relative paths and Cargo's hard-linked binary output.
The caller supplied absolute paths and a byte-identical ordinary file copy; production code did
not change in response. Those refusals remain in the packet.

The browser executable retains its [17-group real-browser acceptance](replay-ui-verification.md),
601 passing Rust tests, 88 frontend tests, strict lint, formatting and generated-contract evidence.
The baseline retains its [MCP replay acceptance](replay-mcp-verification.md). Those checks were
not rerun merely to package identical executable bytes.

## Reproduce or select locally

From the repository, reproduce the disposable installed mission:

```sh
tools/test-sandbox.sh .venv/bin/python -B tools/verify-package-linux.py \
  "$PWD/target/acceptance-bundle/current.tar" \
  --expected-sha256 5f4c6d3bbea35ecc5ece6eccbe00fa7b2f7d76ab5ded5e3e0943119bcea06002 \
  --previous-bundle "$PWD/target/acceptance-bundle/prior.tar" \
  --previous-expected-sha256 6beb6e6d5d86ae18d1404dc093e1b3b68f20f9c5c939b80c91612ab0346c3b75 \
  --evidence-replay --json-results
```

For an authorized installation, use an explicit private absolute root. These commands are a
handoff; only disposable sandbox installations were performed during acceptance:

```sh
python3 -I -B tools/package-linux.py install "$PWD/target/acceptance-bundle/prior.tar" \
  --expected-sha256 6beb6e6d5d86ae18d1404dc093e1b3b68f20f9c5c939b80c91612ab0346c3b75 \
  --root /absolute/private/install
python3 -I -B tools/package-linux.py install "$PWD/target/acceptance-bundle/current.tar" \
  --expected-sha256 5f4c6d3bbea35ecc5ece6eccbe00fa7b2f7d76ab5ded5e3e0943119bcea06002 \
  --root /absolute/private/install
python3 -I -B tools/package-linux.py activate \
  save-0.1.0-src-3b5c3d68b34c032c-bin-7ed9a73b0d4a98fc --root /absolute/private/install
/absolute/private/install/current/bin/save --json doctor
python3 -I -B tools/package-linux.py rollback \
  save-0.1.0-src-8a410c2c04ba2be3-bin-256c3ae501b0879e --root /absolute/private/install
```

Use the updated packager for manifest revision 2. Selection returns the selected manifest's
compatibility values. Stores remain external; the installer performs no migration. This mission
proves replay of the exercised version-1 store by these two executables, not future-format downgrade
compatibility. The [packaging contract](packaging.md) defines verification and authority boundaries.

## Remaining acceptance

The dependency inventory contains 283 packages and available notices, with explicit unresolved
distribution gaps. It is not release-level license clearance or a binary-linkage SBOM. Broader native
platforms, actual agent-host registration, selected live-source compatibility, and the AC-36 human
measurements/feedback/support owner remain open. The complete remaining capability queue is retained
in [the roadmap](roadmap.md). This candidate establishes a runnable fixture handoff, not completion
of the whole product.
