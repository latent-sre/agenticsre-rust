# Dashboard task acceptance — 2026-10-02

The selected [dashboard task contract](tasks.md) passed independent static review and executable
verification. Cally passed both Python 3.11 execution and missing-runtime checks. The next work
remains in [the roadmap](roadmap.md); this record covers the frozen dashboard increment.

## Source identity

- Disposable snapshot commit: `9b1fc647c8f20dda38876e1d25c9486c7bbfd5b2`.
- Tree: `03715bdfbc85314271498fafc10e7186caba90dd`.
- Source archive SHA-256: `a3e8c66333e6e8f5d748d321c135945a87a75fc3897f604477b7589038d219df`.
- All 112 captured files matched the independent snapshot before and after verification.
- Embedded binding SHA-256: `0821152ed6d43e3de75b6f3e06029998f777069191d504792ceb56d909269b48`.
- Original checker SHA-256: `8a109e921332b2da99885cda111ad6b6e7d22965291fd2bc4ddd0199876ec5b2`.

The product checkout remains uncommitted. Verification used a disposable Git repository and
archive under `/tmp/agenticsre-task-final-t5q88tw3` and
`/tmp/agenticsre-poc-cally.eFdXlRfC`. This report and later feature work were added after capture;
they are not implicitly covered by this verdict.

Subsequent error-budget regressions added rejection of positional JSON arrays where task inputs
and request controls require objects. Those forms were not covered by this historical packet.
The follow-on increment adds explicit shape guards and public no-dispatch checks; consult the
roadmap for its later candidate and verification.

## Accepted behavior

`task list`, `task describe dashboard-hygiene` and `task run` work through the shared CLI/core
contract. Successful checks report structured findings, input/source identity, actual Python
version and limited static coverage. Findings preserve successful execution, with an optional
finding-based CLI exit. Malformed, unsupported, uncheckable and incomplete results fail explicitly.

| Check | Observed result |
|---|---|
| Independent static review | APPROVE; no reportable findings; previous supervisor implementation unchanged |
| Rust regression suite | 34 passed, including all 19 command-runner PoC tests |
| Public contracts | 23 task results and 14 command-runner results validated against schemas |
| Independent task boundaries | 36 cases, including exact byte/depth/panel/query limits and denial without dispatch |
| Runtime/protocol fault injection | 25 cases, including malformed/truncated output, source/task mismatches, runtime errors and cancellation |
| Missing interpreter | Four local schema-valid results; Cally independently exercised its image without Python |
| Installed artifact | Offline installation followed by execution with no checkout mounted, poisoned cwd/modules/environment and unchanged input |
| Static checks | Formatting, Clippy with warnings denied and specification checks passed: 16 schemas, 18 positive/9 negative fixtures |

The [independent packet](evidence/dashboard-verification-2026-10-02.txt) retains exact commands,
criteria, observed exits, isolation and limitations. Detailed records and authored independent
tests remain at `/tmp/agenticsre-task-verify-ihpgrZuy`, with `SHA256SUMS`. Local execution used
Python 3.12.3 and Rust 1.98.0. A shared-UID thread limit initially prevented the linker from
starting; reducing build/test concurrency within the same limits resolved that environment
failure. No source change or product-test failure was involved.

## Cally and cleanup

Both Cally lanes used identical frozen source, Rust 1.98.0 and Debian 12, with network disabled,
no inherited proxies or host mounts, dropped capabilities, no privilege gain, 2.5 CPUs, 4 GiB
memory, 512 PIDs and a 15-minute limit.

- [Python lane](evidence/dashboard-cally-python-2026-10-02.txt): Python 3.11.2, all 34 Rust tests,
  actual dashboard task, command-runner mission and doctor passed.
- [Missing-Python lane](evidence/dashboard-cally-missing-python-2026-10-02.txt): build and command
  mission passed; task discovery reported `missing_dependency`; execution returned exit 2 with
  `unsupported` and `not_attempted`.

Both produced binary SHA-256
`603b7b37d8618d7109344f3dff3e5087fb321ebe33e47a54d3817c2c7ddde327`.
The locally installed binary was
`7dc1388b9c7c124f6449d549355216c2d14e1e3a07bdd76b3e8c1a568852f27f`.
Tooling and image pins are in [development](development.md). Remote container removal was
confirmed; independent disposable builds, installations and processes were removed. Source
snapshots and retained evidence were preserved.

Native Windows/macOS, live Grafana, protected agent permissions, full P1/P2 acceptance and
production readiness remain outside this verdict. Optional-runtime handling below Python 3.11
was fault-injected locally, not tested with an actual older installation. No product commit,
push, deployment or paid model campaign was performed.
