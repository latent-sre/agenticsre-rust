# Linux development bundle acceptance

Independent verdict: **PASS** for the current context-containing development bundle. Its installed
CLI/context/tasks, MCP/UI, upgrade and rollback checks passed on Cally. Original MCP/GUI bundle
acceptance and the separate unavailable-Python fixture remain retained below.
This record covers the Linux/stateless development slice of AC-34/AC-35. Release publication,
Windows acceptance and the AC-36 five-task human/agent pilot remain open.
The [sealed refresh packet](evidence/context-bundle-cally-verification-2026-10-03.txt),
[26 refresh receipts](evidence/context-bundle-receipts-2026-10-03.jsonl) and
[five context observation records](evidence/context-bundle-observations-2026-10-03.jsonl)
are copied unchanged from the validated native transcript.
The original [independent packet](evidence/packaging-cally-verification-2026-10-02.txt),
[17 positive receipts](evidence/packaging-positive-receipts-2026-10-02.jsonl) and
[five unavailable-runtime receipts](evidence/packaging-unavailable-python-receipts-2026-10-02.jsonl)
are retained in Git for review. Receipt JSON bytes were copied directly from the validated transcripts.

## Context-containing refresh

The resumed WORKBENCH-009 increment builds from committed source
`837c217838c7e6950e59f8e143daf60f4fe9615d`, tree
`8e22e9af0910063e2ba5d7c2050e1cc4afc443fa`. Its 318 tracked regular files total 3,637,638 bytes.
Independent preparation confirmed all 115 product/build/frontend/third-party paths match the
accepted context R1 capture, and the five package/notice/runtime tools match accepted packaging R1.
The previous native context, grant and package-integrity evidence therefore remains applicable
to those unchanged bytes. No product or packaging-tool correction was needed for the refresh.

| Identity | Context-containing artifact |
|---|---|
| Source-manifest SHA-256 | `5e4001ba226190e0d27a5dae7950985c97eadf42cdbfe8c20f35a4b08b8baf4f` |
| Binary SHA-256 | `22b8a16efbfce9fc03f1e6ed7ac0d64437757a38137124ba355f8bef36321153` |
| Binary bytes/mode | 37,301,160 / `0755` |
| Archive SHA-256 | `1ed48ee349b9c51e0be98bb0ada2ce1fd2577773f91d273f709ad669298917dd` |
| Archive bytes | 40,572,928 |
| Artifact ID | `save-0.1.0-src-5e4001ba226190e0-bin-22b8a16efbfce9fc` |
| Embedded build-provenance SHA-256 | `b0fd87029ac7b81b3db5757ee837b0ef4ce699af05fa4a8efc778118a7986e75` |
| Verification report SHA-256 | `55ffd11b7e36e12e577b045de56db29b987226536c1b026a4b87a729c9f107c9` |

The actual archive and build metadata are retained as `target/packages/linux-context-r1/current.tar`
and `verification.json`; original exports remain in
`/tmp/agenticsre-context-bundle-cally-isacf5wl`. The ordinary binary's digest matches both prior
accepted context builds. One fresh `cargo build -p workbench-cli --locked --offline` ran with
Rust/Cargo 1.98.0, debug/incremental output disabled and two build jobs. Identical package inputs
repacked to the same archive digest. Inventory remains 260 Cargo and 19 frontend records, with
the original distribution gaps unchanged.

The retained MCP-enabled archive below is the refresh's upgrade/rollback baseline; it is not
rebuilt. The original GUI archive remains retained with its separate identity. The selected
installed mission adds context CLI/structured parity, exact observed export provenance and
stale/ambiguous/missing refusals to the existing CLI/tasks/MCP/UI checks. This does not expose
context through the browser or MCP server.

All nine installed acceptance groups passed: corruption refusal before install state, prior
installation and missions, fresh/repeated current installation and selection, context resolution,
stdio MCP, embedded browser assets/authenticated numerical API, rollback, external canaries and
disposable cleanup. The exact `hello workbench\n` mission returned child exit 0, execution
`succeeded`, assessment `not_assessed` and recording `never`. Context CLI and structured requests
agree after excluding validated observation-derived fields; selected export digests/review age,
stale/ambiguous/missing refusal and unchanged source files passed. The older selected artifact
honestly refuses unknown context discovery before upgrade and after rollback.

All 26 exact receipts are schema-valid: six prior CLI, 12 current CLI, one current MCP, one current
UI and six rollback CLI observations. The receipt JSONL SHA-256 is
`f6e61e3d813b5e99a829681d2268a3057114aa5c0cf6fda8212dcae2d9a88b92`; the five context
observation records have SHA-256
`94ffd1313e46d4954a98554e430301f25d16629b878bb39a3217a201eaf4d336`.
Independent DATA-only returned-byte checks validated both real archives, member bounds/modes,
every digest, exact source/build identities and notice references. All 318 source hashes match
the committed Git blobs after execution; the original worktree documentation edits were excluded.

The cached pinned base and outer boundary are unchanged. COPY-only derived image
`sha256:6db3db364de30e91316f87cd3ff5338e8e4b31e2538b99c45f294e3c7028b0fb` ran with read-only
source/root, UID/GID 1000, private PID/network, no host mounts/credentials, no-new-privileges and
zero effective capabilities, with 2.5 CPUs, 4 GiB memory, 512 PIDs and a 900-second runtime bound.
The fresh 8 GiB pre-staging floor passed. Process residue was empty; exact container/image, staging
and local-driver absence and unchanged cached base were independently confirmed.

The independent sealed packet is
`/tmp/agenticsre-context-bundle-cally-isacf5wl/VERIFICATION.md`, SHA-256
`b8c56039ad431889a6488e8c09207cfb934f336e36138b1c11161f7afdf4fc1d`.
Its 46-artifact digest manifest has SHA-256
`6bf5d842f217b8314d4bf7d37661611646fc87418a84e2777e7dd96dcfe20638`.
Original evidence and verifier scripts are sealed read-only; repository evidence copies retain exact bytes.

Root independently verified the retained new archive as DATA and spot-checked the exact hello
receipt. An initial root checker assumed the assessment enum was an object; correcting that checker
passed without a product change or native rerun. Evidence:
`/tmp/agenticsre-context-bundle-root-verify.json` and
`/tmp/agenticsre-context-bundle-root-receipt-spot.json`.
Full workspace/native context, package-integrity and unavailable-Python suites were carried from
the accepted unchanged source rather than rerun for a new artifact identity. Current acceptance
does not broaden platform, release-clearance, live context or public in-flight signal claims.

## Original source and review

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

## Original MCP/GUI artifacts

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
