# Grafana fixture acceptance — 2026-10-02

The [native Grafana contract](grafana.md) passed independent correctness review, a separate static
security pass with affected-path re-review, and independent executable verification. Both Cally
lanes passed the final frozen source. This closes WORKBENCH-003 fixture acceptance; broader work
continues in [the roadmap](roadmap.md).

## Accepted identity

- Disposable commit: `e22a1fe4193058b96afbca4a3e5dedc63dc7fbb9`.
- Tree: `0e23352e0c508589c42b208f65156e3648b7df2d`.
- Source archive SHA-256: `2848dd7f72969a492188f1f39408432489a4fd9f1e334f63f07466ae5ef6431d`.
- Snapshot: `/tmp/agenticsre-grafana-r2-final-_kguyno2`; 168 captured files matched before and
  after independent execution. The coordinator also matched all 75 current product, schema,
  test, manifest and tooling files against the archive before subsequent development.
- Final Cally binary SHA-256 in both images:
  `eda24309b143f5c48f97e804edfd9e967fa6356f4f2a90383303a2911fe5f889`.

Product changes remain uncommitted. These commits exist only in disposable verification
repositories. This report, evidence copies and subsequent planning documentation were added after
capture; the verdict does not automatically extend to later source changes.

## Evidence

| Check | Result and scope |
|---|---|
| Correctness review | APPROVE on the accepted identity; all three original P2 findings and the remaining timestamp variant closed; no open findings |
| Security review | APPROVE within operator-local scope; separate manual source-to-sink pass followed by R1/R2 affected-path reviews; zero reportable additional findings |
| Final full Rust suite | 68 passed in the builder's local sandbox and Cally's full image, including 17 native HTTPS tests and the existing Python regression runner |
| Final public HTTPS suite | 96 schema-valid results passed locally, independently, and on Cally; Cally's stdlib-only harness explicitly skips schema validation |
| Fresh independent R2 tests | Five focused Rust regressions; 67 timestamp cases; 15 boundary-closure calls; three independently implemented HTTPS missions; exact failed-R1 witness now denied |
| Carried independent evidence | R1's full 66-test suite, existing command/task public checks, Clippy, offline release installation and seven no-checkout mission/cap checks; unchanged paths matched by source identity |
| Format/lint/specifications | Final builder checks passed; 20 schemas, 21 positive and nine negative fixtures; independent R2 format/spec checks passed |
| Minimal runtime | Cally passed 17 native HTTPS tests without Python and explicitly returned `missing_dependency` for both Python tasks |

The [independent PASS packet](evidence/grafana-verification-2026-10-02.txt) distinguishes fresh R2
execution from retained R1 checks. Its SHA-256 is
`6decbc9b2eb9f58da66b65d596589c3e3671e845040e78c9a347858a71e3504d`.
The verifier compared 33 other crate/resource/manifest files and all Grafana production code
outside `date()` before carrying those results. It did not claim a fresh R2 release installation
or full independent Rust suite.

The formal final review envelope used repository `latent-sre/agenticsre-rust`, base
`378c6ed0504ee9ee5b6e1d2e1da252bc41b4bb62` and the accepted candidate/tree above. Correctness and
security-delta verdicts were both APPROVE, with no findings. Reviews ran no repository code.
The host rejected a dedicated security-auditor spawn because its agent-thread limit was reached;
the sre-tool pipeline's permitted second, security-only code-reviewer pass ran instead. This was
manual static review, not a Codex Security scanner run.

## Corrections proved before acceptance

Initial snapshot `8a301811eaafabc3e6b7a9d25d7ee07cfcc29063` failed independent acceptance despite
ordinary fixture tests passing. The verifier reproduced:

1. A near-limit dashboard emitted 2,097,473 bytes, exceeding the 2 MiB final cap by 321 bytes.
   Finalized source/timing metadata now participates in structured-response bounding. Fresh R2
   probes retained a successful 2,097,150-byte receipt without any oversized result; query overflow
   returned a schema-valid 2,632-byte failure with its structured response omitted.
2. A Unicode origin expanded to 2,420 characters during URL normalization and violated the result
   schema. The normalized origin is now bounded before HTTP.
3. `.0000000001Z` lost its tenth fractional digit in the time parser and dispatched a changed
   query. R1 fixed the ordinary form, but the parser also accepted a period date/time separator
   that bypassed the first guard. Full ASCII RFC3339 grammar and the captured fractional field
   are now validated before conversion. The independent matrix denied 58 invalid inputs before
   HTTP and preserved exact milliseconds for nine valid case/offset/precision variants.

Retained failures are explicit: [initial packet](evidence/grafana-initial-verification-2026-10-02.txt),
[initial witnesses](evidence/grafana-review-red-2026-10-02.txt),
[R1 packet](evidence/grafana-r1-verification-2026-10-02.txt), and
[separator witness](evidence/grafana-separator-red-2026-10-02.txt). Two review/fix rounds resolved
the findings. No schema, dependency or supervisor change was needed during these corrections.

## Environment and limits

Local checks used isolated user/network/PID/IPC/UTS namespaces, read-only source/runtime/registry,
private temporary storage and synthetic TLS/auth material. Independent checks retained one CPU,
one Cargo job, two test threads, per-process 4 GiB address space, 900-second CPU/outer limits,
1,024 descriptors and the previously authorized 1,024 shared-UID process/thread ceiling.
These are distinguished from aggregate cgroup limits in the packet.

Cally used the existing pinned Rust 1.98.0 Debian 12 images with no network or host mounts,
2.5 CPUs, 4 GiB aggregate memory, 512 PIDs, no new privileges, dropped capabilities and a
15-minute limit. [Full log](evidence/grafana-cally-python-2026-10-02.txt) and
[minimal log](evidence/grafana-cally-minimal-2026-10-02.txt) are unchanged copies. Archives remain at
`/tmp/agenticsre-poc-cally.6C2zpuYy` and `/tmp/agenticsre-poc-cally.wgIkSodT`. Both containers were
confirmed absent after completion. Independent disposable builds, installations and listeners
were removed; detailed test harnesses and hashes remain under
`/tmp/agenticsre-grafana-r2-verify-dt6d_hi1`.

This is controlled-fixture compatibility for `grafana-legacy-v1`. Live Grafana versions, real
credentials, positive system-DNS integration, Windows and cancellation of remote datasource work
remain unverified. The operator-local config/environment pair does not isolate secrets from an
unrestricted same-account agent. Masking covers literal credentials and tested canonical
percent/form/base64 variants; lowercase or mixed-case percent encodings and arbitrary sensitive
telemetry are outside that demonstrated coverage. No live deployment, product commit or push ran.
