# Linux development bundle acceptance

Independent verdict: **PASS**. The real current and previous bundles passed installed CLI, task,
MCP/UI, upgrade and rollback checks on Cally. A separate unavailable-Python fixture also passed.
This record covers the Linux/stateless development slice of AC-34/AC-35. Release publication,
Windows acceptance and the AC-36 five-task human/agent pilot remain open.
The [unchanged independent packet](evidence/packaging-cally-verification-2026-10-02.txt),
[17 positive receipts](evidence/packaging-positive-receipts-2026-10-02.jsonl) and
[five unavailable-runtime receipts](evidence/packaging-unavailable-python-receipts-2026-10-02.jsonl)
are retained in Git for review. Receipt JSON bytes were copied directly from the validated transcripts.

## Source and review

Packaging R1 is frozen at commit `9c1ee1c7e058f48afbb99bcbb867a519392ba066`, tree
`81d6b895a346475bc9017a6c39778fc2b4208df6`, in
`/tmp/agenticsre-package-r1-snapshot-21jfbzjo`. Its 301-file capture manifest is
`/tmp/agenticsre-package-r1-snapshot.json`, SHA-256
`d264a8f5c105ba8276fd27714102850b82caac8ebc2e8639094850c624ee1009`.
All Rust/resources/frontend production bytes match the accepted MCP R2c source; packaging does
not introduce another execution path in the product.

Independent R0 review requested two corrections. A same-size archive replacement could substitute
different bytes between digest verification and parsing/extraction. R1 consumes one private,
bounded archive snapshot for all passes. Dependency notice references could point to absent files;
R1 requires safe references to included files during creation and shared validation, including
installation and selection. Real replacement regressions and missing-notice public operations
failed before their corrections and passed afterward. Independent R1 re-review APPROVE;
zero unresolved findings, one packaging correction round.

All 31 package tests passed on Python 3.12.3 and 3.14.7 locally and Python 3.13 on Cally. Root's
selected race and missing-notice checks also passed. Red/green evidence is retained in
`/tmp/agenticsre-package-r1-p1-{red,green}.log`,
`/tmp/agenticsre-package-r1-p2-{red,green}.log` and
`/tmp/agenticsre-package-r1-root-spot.log`.

## Actual artifacts

These are ordinary builds from two independently accepted product source captures. Neither is a
fixture executable or a new product release version. The supplied metadata and source manifests
are preserved in each archive, and independent external validation checked the actual exported
archive bytes. Archive hashes must come from this separately trusted record, not from an
untrusted archive alone.

| Artifact | Current | Previous GUI baseline |
|---|---|---|
| Source commit | `9c1ee1c7e058f48afbb99bcbb867a519392ba066` | `76e6a3f316da685e12f3e741935f6dd55d9e64c0` |
| Source tree | `81d6b895a346475bc9017a6c39778fc2b4208df6` | `9bde9db7f06e19e8b6e527e2f7b31934f5dd6776` |
| Source-manifest SHA-256 | `8f4d42b935291593b95860285ce67da717be23ae1e1f31ab6e525afb74824074` | `60395b4757c8ad83feeb87c5196255d54475b977c8e8929d3ff930abd53e3663` |
| Binary SHA-256 | `939e5f5b5d2bccea2630df6d4870b12a18e2d72f3e834531195123965169aa56` | `2990afe949f1279ad87f59bdf7c515d88c3622ec4706de485c13a11e8bdc0495` |
| Binary bytes | 36,887,184 | 35,114,600 |
| Archive SHA-256 | `76a2ca3414ae624cb51f7ff6378e78771336ec9360b8f4c9e3618e8f52aafa02` | `efa9fd14bb7daebe7cca729315f867aa34fbfad7e917f0f200c0346ecb179ba2` |
| Archive bytes | 40,156,672 | 38,209,024 |
| Artifact ID | `save-0.1.0-src-8f4d42b935291593-bin-939e5f5b5d2bccea` | `save-0.1.0-src-60395b4757c8ad83-bin-2990afe949f1279a` |

Retained archives are `target/packages/linux-r1/current.tar` and `previous.tar` in this checkout,
with a hash-checked copy of `verification.json`. Original exports also remain in
`/tmp/agenticsre-package-cally-oyrt09pp`. They are data artifacts kept outside Git; no installation
or product execution occurred on Adama. They embed the browser assets, so Node is unnecessary at
runtime.

## Runtime and passed transactions

Both `cargo build -p workbench-cli --locked --offline` invocations ran sequentially in Cally's
Debian 13.6 x86-64 GNU/Linux environment, with Rust/Cargo 1.98.0 and glibc 2.41. The cached base is
`docker.io/library/rust@sha256:620dbcd124499c59e2406d3741574b5c5838cf9eb9656f0c3a03948f79b02959`;
the COPY-only derived build image is
`sha256:3b953be2fbd6d8dec10bea884a08d76181afb3c0eecdd14c846c462c32c9e506`.
The bounded job used read-only source/root, UID 1000, private PID/network namespaces, no host mounts
or credentials, no-new-privileges and zero effective capabilities, with 2.5 CPUs, 4 GiB memory,
512 PIDs and a 900-second limit. Synthetic inputs and disposable installation roots were used.

Eight installed acceptance checks passed: fresh/repeated installation, explicit selection,
offline help/version/doctor, literal command execution away from source, both embedded tasks,
current stdio numerical MCP and embedded UI/assets/authenticated numerical API, distinguishable
upgrade, verified rollback and unchanged external canaries. The archives repacked deterministically;
binary/data modes and installed hashes passed. Ordinary package integrity tests covered refused
corruption, hostile paths/types, activation-pointer tampering and changed installed files.

The exact 17 exported receipts validate against the canonical result and applicable task schemas:
five prior CLI, five current CLI, one current MCP, one current UI and five rollback CLI results.
External validation also checked archive member bounds/types/modes, every manifest digest, source
and build identity, and all declared dependency notice references without executing archive data.

Evidence: `/tmp/agenticsre-package-cally-oyrt09pp/package-cally.log`, `verification.json` and
`external-validation.json`. The build metadata file has SHA-256
`c890f94a87f232eb946a872ec3e6b4c12863210348ba150879715e1646bd2bde`.
The job's final process residue was empty; exact-name container/image/staging cleanup was confirmed.

## Unavailable optional runtime — passed

The first full lane exited 1 after the passed transactions when nested Bubblewrap could not mount
proc. The product negative payload did not run. A focused follow-up retained private outer proc,
then stopped before the payload on an extra verifier namespace assertion. Its observer recorded
a `bwrap` process without a state; it cannot establish whether it was live or a zombie. Exact
outer container/image removal was confirmed. Both original failures remain in their records.

The final focused fixture uses the same archive and tool bytes in COPY-only disposable image
`sha256:0b3e328574f60d28c79d1931433bb0162499bcf14c3497c96bde2e85a0e8d3d6`.
Both `/usr/bin/python3` and `/bin/python3` resolve to a root-owned mode-0644 placeholder. The
verifier uses an authentic renamed base interpreter with SHA-256
`17b78e0a93175e86f9ac03141924fd7a7f0c0c52e66b34bfa0de20ffef989df1`.
This tests unavailable fixed runtime bindings, not total removal of every interpreter. No nested
namespace, Rust rebuild or product source correction was needed; outer isolation and limits were
unchanged and verified before product execution.

All four focused groups passed, including corruption refusal, fresh/repeated installation and
selection, basic help/version/doctor and literal-command mission, both embedded tasks' honest
`missing_dependency` refusals with `not_attempted` and no child, unchanged canaries and cleanup.
Five exact receipts validated against the canonical schema. Combined with the retained positive
lane, there are 22 validated receipt observations. No process residue remained; exact container/
image removal passed. Final readback found no run-owned jobs, containers or disposable images.

The independent combined packet is
`/tmp/agenticsre-package-unavailable-python-mhf8ewgw/VERIFICATION.md`, SHA-256
`d5fd9c9e29e4f91ef96db21602d4a2948c995b9dd637942f661b8a4255dc9044`.
Its 19-artifact digest manifest has SHA-256
`ff650194e0056f62ede74fa1ed91f3e3fd25bc5c6fe18ec781788ed45492269d`.
All three original evidence directories are sealed read-only; the two failed setup records remain
unchanged and are not relabeled as passing runs.

## Remaining limits

The tested ABI is Debian 13 x86-64/glibc 2.41; other distributions, Windows and protected managed
installations need native evidence. Same-account edits remain possible. Selection is atomic within
the filesystem; power-loss durability is not claimed. There are no state/config migrations,
durable history, signatures, MCP-host registration or live service acceptance in this slice.

The inventory covers 260 current or 245 previous Cargo registry packages plus 19 frontend notice
records, including selected Tailwind/Vite contributions. It is not a linked-binary or complete
frontend build-tool SBOM. Release license clearance, unavailable npm archive integrity revalidation,
Rust standard-library/compiler and system-library notices remain gaps. Available license texts
are absent for several locked crates; the exact list is in each archive's manifest and inventory.
Those missing texts are disclosed rather than replaced with guessed licenses. No cleared release
or permission to distribute is asserted.
