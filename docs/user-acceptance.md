# User acceptance walkthrough

The [current Linux development candidate](acceptance-bundle-verification.md) is ready for a local
fixture walkthrough. The [roadmap](roadmap.md) remains the status record. This walkthrough prepares
AC-36; passing automated tests does not supply human feedback, measured usefulness or support
ownership. It also does not close the remaining capability and native-platform requirements.

## Select the pilot

The existing decisions select Linux x86-64 first and the home lab for the later live pilot.
Confirm the actual host/version and agent client before expanding the tested configuration.
The current candidate was exercised on Ubuntu 26.04.1 x86-64, glibc 2.43, kernel 7.0.0-38.
Actual agent-host registration and client interactions remain pending.

Use fixtures for the first walkthrough. A live Grafana pilot additionally needs the named
service/environment, reviewed context export and a scoped read identity. Dradis is the recorded
Grafana candidate; existing administrative credentials do not establish the required read grant.
Installation, agent registration and live access retain their separate authorization boundaries.

## Five proposed workflows

Use the same inputs for the existing/manual method and Workbench. Record elapsed time, manual
steps, corrections and outcome for each; automated test duration is not a human baseline. Start
with these five concrete workflows, or substitute the operator's representative daily tasks before
recording a comparison.

| Workflow | Operator action and expected result | Existing fixture evidence |
|---|---|---|
| Inspect a dashboard | Run `dashboard-hygiene` on the clean fixture. Expect one checked panel, no findings and unchanged input. Then inspect a known defective fixture. | [Task contract](tasks.md), [dashboard verification](dashboard-verification.md) |
| Calculate an error budget | Run `error-budget` with `{"slo":99.9,"bad_minutes":40.32}`. Expect an exhausted budget with 40.32 allowed bad minutes; inspect the calculation rather than treating it as current service health. | [Budget contract](error-budget.md), [installed candidate](acceptance-bundle-verification.md) |
| Inspect and replay saved evidence | Use the private copy of the retained comparison fixture. Select `run-9-18dd60cacef0724e-1`, inspect its original window, replay, and repeat with a one-item limit. Expect historical observations and explicit omissions. | [Offline interface](offline-evidence.md), [browser replay](replay-ui-verification.md) |
| Compare observations | Compare that bundle with `run-16-18dd60cad194352b-1` using `exact-window-v1`. Inspect typed differences and their sources; differences do not establish causation or remediation. | [Comparison interface](evidence.md), [comparison verification](evidence-comparison-verification.md) |
| Produce a handoff | Create a Markdown report from both bundles, add an evidence-bound proposal and inspect/download it. Expect historical provenance and a pending proposal; no external publication occurs. | [Report interface](reports.md), [browser investigation verification](investigation-ui-verification.md) |

The retained synthetic store is
`docs/evidence/evidence-comparison-2026-10-10/installed-r2/store`. Use a separate copy with private
directories (0700) and files (0600); never alter the retained acceptance fixtures. CLI examples
are in the linked contracts. For the browser walkthrough, launch the selected installed binary
with explicit grants and the absolute private store:

```sh
/absolute/install/current/bin/save --state-dir /absolute/private/store ui serve \
  --allow task.run --allow evidence.list --allow evidence.open \
  --allow evidence.replay --allow evidence.compare --allow report.create
```

Open the printed local URL. Select each operation in Checks, inspect its result, download the
receipt, and reopen it from Run history. The MCP replay grant and client-level prerequisites are
documented in [MCP investigation setup](investigation-mcp.md). Raw-protocol acceptance is available;
the selected real agent client must still be exercised.

## Record the decision

For each workflow, retain the exact candidate digest, host/client versions, input identity,
baseline and Workbench measurements, result/receipt reference, errors or corrections, and the
operator's feedback. The human owner should record accept/revise/reject, any agreed limitations,
and the person responsible for support. Those values are currently unfilled; do not infer acceptance
from an automated pass or lack of a reply.

If a workflow fails, keep its receipt and return to the retained prior executable using the
[candidate rollback instructions](acceptance-bundle-verification.md). Switching versions does not
migrate or restore external stores. Keep the original evidence and configuration separately.
