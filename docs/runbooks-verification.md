# Guided runbook acceptance

WORKBENCH-015 is accepted for the Linux core/CLI fixture scope. Installed `evidence-review`/1
performs real verified evidence reads and comparison, selects a typed human checkpoint, and resumes
without repeating completed observations. The [contract](runbooks.md) defines inputs, checkpoints,
journal semantics and limits. [The roadmap](roadmap.md#workbench-015--guided-runbooks) owns status.

## Verification

The [verification summary](evidence/runbooks-2026-10-11/verification-summary.json),
[build provenance](evidence/runbooks-2026-10-11/build-provenance-r1.json) and
[independent execution verdict](evidence/runbooks-2026-10-11/independent-execution-r1.json) bind:

- 317 passing workspace tests, zero failures, and 19 existing native tests ignored.
- Passing format, all-target/all-feature clippy, ordinary build and specification checks:
  44 schemas, 33 positive and 18 negative fixtures before acceptance documentation.
- Eleven public runbook tests: all checkpoint branches, restart/resume, preserved child identities,
  concurrent revision admission, stale/wrong/input/definition refusal, source drift, lock contention,
  explicit fresh attempts and private filesystem boundaries.
- Core tests for definition/cycle/capability rejection, typed inputs, corrupted state, failure policy,
  prerequisites, deadlines and filesystem restart/CAS/quota/layout safety.
- Independent installed execution: 14 commands and 29 unique canonical receipts, comprising 14
  terminal results, six stored saved-query receipts and nine retained runbook child receipts.

Three actual private HTTPS collections produced 18 requests. The fixture closed and its workers
joined before runbook execution; configuration/context files were removed and the provider was
absent. The installed binary ran from a private prefix and unrelated directory. Acknowledgement
preserved full child receipts, not merely their summaries; evidence bytes remained unchanged.
Fresh attempts used new run and child IDs while preserving earlier journal bytes. Incompatible
observations remained partial after acknowledgement.

Real ENOSPC used a dedicated 64 KiB private tmpfs. Journal publication returned `store_no_space`,
retained `record_mode=always` and an uncertain effect, and did not claim a waiting checkpoint or
published run state. The format header remained; no host disk was filled. All fixture threads
joined and before/after source and binary guards matched.

## Reviewed identity

Definition digest: `7bd4c25251342606602c0209404b7bc67e45a34b76ba324bc101430443672480`.
Frozen source: `target/runbooks/review-r1`, 898 files; manifest SHA-256
`f8dd2c1b1283395f5041e2f21e8709af5026e80d38c5810bff3fa515dde373b8`.
Ordinary binary: 42,400,672 bytes; SHA-256
`334c4bc1a876ee8873fb094b2ad15727b3bf7b2e47b95ecb15e2499d15664fad`.

[Independent static review](evidence/runbooks-2026-10-11/review-summary.json) has no unresolved
findings. Two P2s were corrected: resume now preserves cancellation/deadline/contention instead
of misreporting source drift; requested recording mode is set before journal publication so
persistence failures remain truthful. The lock regression, error-class tests and installed ENOSPC
case verify those boundaries. Initial compilation/lint failures, the corrected definition-error
expectation and a temporary syntax error during lint cleanup remain in the packet.

The [artifact manifest](evidence/runbooks-2026-10-11/artifact-manifest.json) hashes 130 retained
artifacts; manifest SHA-256
`6786b5b804d59b67ff6c56edb136409b97687d6f5baa6b4b97de4d3e26d79281`.
Acceptance documentation added afterward does not alter tested product bytes. Verification used an uncommitted working-tree snapshot. The subsequent PR preserves the tested
product bytes; this acceptance record does not establish merge or release readiness.

## Scope

This first installed runbook reviews existing evidence; it does not install arbitrary workflows,
execute commands embedded in text, contact live Grafana during review, or create a hosted grant.
Journal state is bounded and private. An uncertain in-flight state requires an explicit fresh
attempt; the negative recovery test establishes refusal rather than claiming a real kill/recovery
campaign. Saved budget snapshots exclude final publication overhead from the next invocation,
as documented in the contract; they are not an exact cumulative wall-time ledger.

Native Windows support and the home-lab live pilot remain separate. The next roadmap increment is
offline evidence opening/replay; reports and other remaining capabilities are not completed here.
