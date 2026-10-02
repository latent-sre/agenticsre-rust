# Linux PoC acceptance — 2026-10-02

The selected [PoC contract](poc.md) passed independent code review and executable verification.
This closes the local command-runner increment, not the broader P1 platform milestone or production
acceptance. Current and next work remain in [the roadmap](roadmap.md).

## Candidate identity

The product checkout remains uncommitted. Verification used a frozen source archive and a
disposable Git repository; it did not create or move a product-repository ref.

- Snapshot commit: `c469973422cac1745ce35ee3c3502ef91395a247`.
- Snapshot tree: `a156201636bd3fbd158d9fe5bd7ebfef864edb47`.
- Source tar SHA-256: `5f332947d042313979ebaf08471c2c5769e142e56bf46b3f4378bde8a55c62dc`.
- Capture: all 90 tracked/unignored source files, excluding Git metadata, virtualenv and target
  output. The independent verifier matched every captured file to the snapshot before and after
execution, and matched all 16 Rust source/test/manifest/toolchain files to the product checkout.
- Original archive and build inputs: `/tmp/agenticsre-poc-cally.yohaFnHM/`.

This acceptance record and final README/roadmap status were added afterward. They do not change
the verified executable source. Future source changes require affected verification again.

Later task work expanded strict-input checks to reject positional JSON arrays in object-only
request controls. Those forms were absent from this historical packet; follow-on source and
no-dispatch regression evidence are tracked in the roadmap.

## Results

| Criterion | Evidence and result |
|---|---|
| Literal command and public interface | Exact documented cargo-run mission passed; text, call and schema receipts agree |
| Strict admission | 77 independent CLI invocations, including 62 denial cases checked against a real marker and a successful control |
| Environment/stdin | Exactly four documented child variables, synthetic secrets/proxies absent, stdin closed |
| Output limits | Both pipes drained; UTF-8 replacement and truncation disclosed; encoded result below 2 MiB |
| Lifecycle | Deadline, TERM refusal/KILL, descendants, held pipes, closed/stalled consumer and unknown cleanup exercised |
| Interruptions | SIGINT/SIGTERM map to 130/143 during execution, cleanup and delivery; original outcome preserved |
| Discovery | Inspection, capabilities and doctor ran offline; inspection created no child marker |
| Code/checks | 19 Rust tests, 14 public-schema cases, specification verifier, formatting and Clippy passed |
| Development artifact | Isolated offline install, installed mission, removal and return to the previous explicit binary path passed |
| Cally | Same frozen source built offline on Debian 12; all 19 Rust tests and command/doctor demonstrations passed |

A measured fixture generated 64 MiB on each pipe. The supervisor drained all output, emitted a
1,836,921-byte receipt and disclosed truncation; observed peak RSS/high-water was 9,596 KiB over
106 samples. This describes that capture workload, not an arbitrary-child memory guarantee.

Independent review found one P2: signals arriving during timeout cleanup or final delivery lost
the documented CLI exit mapping. Synchronized tests reproduced both cases before the fix; the
same independent probes pass on the accepted snapshot. Verification also caught Cargo's ambiguous
default executable, corrected by `default-run = "save"`. The focused immutable re-review approved
the changes with no remaining findings and zero P0/P1 findings.

## Environments and retained evidence

Local independent verification used Rust/Cargo 1.98.0 and Python 3.14.7 on Linux x86-64, kernel
6.8.0-142-generic. Bubblewrap isolated network, processes, credentials and writable storage, with
read-only source/runtime mounts. Process/address-space limits and two Cargo build jobs bounded the
verification workload. See the [complete independent packet](evidence/poc-verification-2026-10-02.txt)
for exact criteria, commands, exits, limits and baseline failures.

Cally used rootless Podman and Rust 1.98.0 on Debian 12, with no network/host mounts, no inherited
proxy, all capabilities dropped, no privilege gain, 2.5 CPUs, 4 GiB memory, 512 processes and a
15-minute limit. `tools/cally-check.sh` exited 0. Its [retained log](evidence/poc-cally-2026-10-02.txt)
includes all test results, actual receipts and binary SHA-256
`99b5e8e04ad3c670409425262962ea5010ab3c85b85163fc4407051096ff904b`.
The original PoC image was
`docker.io/library/rust@sha256:af0579d28b9a7ec5251aaafcb0c0a23dcde5c97065112aae0cc3abeda42d5394`.
The helper's additional task lane is documented in the [development guide](development.md).

Both environments were cleaned: no test containers or verifier processes remained. Detailed
independent per-command records and test-only harnesses remain under
`/tmp/agenticsre-poc-verify-jurhBUub/`, with their `SHA256SUMS`. The two linked evidence files are
unchanged copies retained in this repository.

## Remaining scope

Native Windows/macOS behavior, protected agent command profiles, tasks, Grafana/MCP/live acceptance,
persistence and remaining product capabilities are open. Process groups do not contain deliberately
escaping programs; the tests verify honest uncertainty instead. Interrupted delivery may leave
incomplete JSON, which consumers must detect using transport/exit status. The artifact exercise
used two paths to this same revision; it does not prove a prior-release or stored-data migration.

GitHub CI configuration passed actionlint and zizmor locally; it has not been published or executed
as a hosted workflow. Dependency metadata declared licenses for all 40 resolved workspace packages;
this is not a completed distribution/license or vulnerability review. No live integration,
production deployment, paid review campaign, product-repository commit or push was performed.
