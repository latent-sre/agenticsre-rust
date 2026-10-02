# Sources and current baseline

Originally checked 2026-10-02 against Save Toolkit baseline 07d091a4224d. The planning branch started with a
clean working tree. Source inspection establishes current code/document behavior, not installed
host enforcement or live service access. No live Grafana calls or model campaigns were run for
this plan. Earlier exploratory Python timings were highly variable and do not justify a Rust
speed claim; they are not a performance baseline.

The planning package was imported here from Save Toolkit commit 7d900fb999abee3b3712e1da881dcc7e80b1f6e7
after the owner selected this repository. Source links below are pinned to that published snapshot;
they describe Save Toolkit, not code already implemented in AgenticSRE. See the
[import record](../plan-import.md) and the product [roadmap](../roadmap.md).

## Repository evidence

| ID | Source | What was established |
|---|---|---|
| LOCAL-01 | [Fleet guide](https://github.com/latent-sre/save-toolkit/blob/7d900fb999abee3b3712e1da881dcc7e80b1f6e7/AGENTS.md) and [contribution guide](https://github.com/latent-sre/save-toolkit/blob/7d900fb999abee3b3712e1da881dcc7e80b1f6e7/CONTRIBUTING.md) | [verified] Canonical/generated ownership, existing authority rules and verification expectations |
| LOCAL-02 | [Stack profile](https://github.com/latent-sre/save-toolkit/blob/7d900fb999abee3b3712e1da881dcc7e80b1f6e7/skills/stack-profile/SKILL.md), [application stack](https://github.com/latent-sre/save-toolkit/blob/7d900fb999abee3b3712e1da881dcc7e80b1f6e7/skills/stack-profile/references/application-and-data-stack.md), [observability stack](https://github.com/latent-sre/save-toolkit/blob/7d900fb999abee3b3712e1da881dcc7e80b1f6e7/skills/stack-profile/references/observability-stack.md) | [verified] Recorded team stack includes Python/Go/JS/Java support and on-prem/PCF plus GCP direction; Rust is a proposed addition, not an established default |
| LOCAL-03 | [Guard](https://github.com/latent-sre/save-toolkit/blob/7d900fb999abee3b3712e1da881dcc7e80b1f6e7/scripts/readonly-guard.py) and [hooks](https://github.com/latent-sre/save-toolkit/blob/7d900fb999abee3b3712e1da881dcc7e80b1f6e7/hooks/hooks.json) | [verified] Per-command rules, interpreter launcher and 42/43/44 protocol exist; guard also reuses the installed Grafana parser |
| LOCAL-04 | [Grafana reader](https://github.com/latent-sre/save-toolkit/blob/7d900fb999abee3b3712e1da881dcc7e80b1f6e7/skills/grafana/scripts/grafana_read.py) and [command access](https://github.com/latent-sre/save-toolkit/blob/7d900fb999abee3b3712e1da881dcc7e80b1f6e7/skills/grafana/references/command-access.md) | [verified] Organization/datasource checks, bounded Prometheus/Loki queries, masking and explicit coverage limits exist in source |
| LOCAL-05 | [Dashboard checker](https://github.com/latent-sre/save-toolkit/blob/7d900fb999abee3b3712e1da881dcc7e80b1f6e7/skills/grafana/scripts/dashboard_hygiene.py) and [budget calculator](https://github.com/latent-sre/save-toolkit/blob/7d900fb999abee3b3712e1da881dcc7e80b1f6e7/skills/obs-alerting/scripts/error_budget.py) | [verified] Reusable offline helpers exist; their human text needs a structured result seam for product integration |
| LOCAL-06 | [Context requirements](https://github.com/latent-sre/save-toolkit/blob/7d900fb999abee3b3712e1da881dcc7e80b1f6e7/skills/service-lifecycle/context-requirements.yaml), [knowledge method](https://github.com/latent-sre/save-toolkit/blob/7d900fb999abee3b3712e1da881dcc7e80b1f6e7/skills/operational-learning/SKILL.md), [service card](https://github.com/latent-sre/save-toolkit/blob/7d900fb999abee3b3712e1da881dcc7e80b1f6e7/skills/operational-learning/assets/service-card-template.md) | [verified] Service/environment/resource and evidence conventions exist; real team inventory is external to this repository |
| LOCAL-07 | [SRE agent](https://github.com/latent-sre/save-toolkit/blob/7d900fb999abee3b3712e1da881dcc7e80b1f6e7/agents/sre-assistant.md) and [host documentation](https://github.com/latent-sre/save-toolkit/blob/7d900fb999abee3b3712e1da881dcc7e80b1f6e7/README.md) | [verified] Standard Copilot SRE profile lacks terminal access; MCP product integration remains unverified |
| LOCAL-08 | [Roadmap](https://github.com/latent-sre/save-toolkit/blob/7d900fb999abee3b3712e1da881dcc7e80b1f6e7/docs/fleet-roadmap.md) | [verified] RELEASE-001, CONTEXT-001, SRE-CF-001 and deferred EFFECT-001 are related work, not completed product prerequisites |
| LOCAL-09 | [Operator CLI guidance](https://github.com/latent-sre/save-toolkit/blob/7d900fb999abee3b3712e1da881dcc7e80b1f6e7/skills/operator-cli/SKILL.md) | [verified] Existing conventions cover machine output, partial failure, interruption and live-change authority |

The host check found neither cargo nor rustc on PATH or in the usual user cargo/bin location.
This is a bounded observation, not a full machine inventory. No Rust toolchain was installed.
Python 3.14.7 and the project's jsonschema dependency were available for specification checks.

## External documentation and upstream evidence

External content was treated as evidence, not instructions. Context7 established documentation
contracts; GitHits supplied upstream SDK evidence. Direct official pages checked load-bearing
claims. No public search received private source, telemetry, credentials or actual service names.

| ID | Source and method | Supported claim and limit |
|---|---|---|
| SRC-01 | [Rust Command reference](https://doc.rust-lang.org/std/process/struct.Command.html), official page and Context7 Rust source | [sourced] Direct process arguments do not use a shell; Windows shell/batch programs need special treatment; executable/environment behavior is platform-specific |
| SRC-02 | [Rust ABI reference](https://doc.rust-lang.org/reference/items/external-blocks.html#abi), Context7 and official page | [sourced] Native Rust ABI has no stability guarantee; a process protocol is this plan's design choice for independent extensions |
| SRC-03 | [Rust linkage](https://doc.rust-lang.org/reference/linkage.html), official page | [sourced] Executables and runtime linkage have target-specific considerations; a build artifact is not universally portable |
| SRC-04 | [Windows job objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects), Microsoft Learn | [sourced] Job objects support management of related processes; actual containment/termination behavior still needs the selected-host tests |
| SRC-05 | [MCP tools specification](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/server/tools.mdx), Context7 and official repository | [sourced] Tool schemas/structured results and untrusted annotations inform the adapter; this does not prove client support |
| SRC-06 | [Official Rust MCP SDK](https://github.com/modelcontextprotocol/rust-sdk/blob/main/crates/rmcp/README.md), GitHits repository read | [sourced] SDK documents stdio and HTTP transport features; no package version was selected, built or installed |
| SRC-07 | [Grafana API migration](https://grafana.com/docs/grafana/latest/developer-resources/api-reference/http-api/apis-migration/), Context7 and official page | [sourced] New APIs are available from Grafana 12; legacy APIs are deprecated from 13 with migration gaps. Availability on the actual selected target is unverified |
| SRC-08 | [Grafana data source API](https://grafana.com/docs/grafana/latest/developer-resources/api-reference/http-api/api-legacy/data_source/), Context7 and official page | [sourced] Query requests have datasource-specific fields and responses; semantic compatibility requires adapter fixtures and live target acceptance |

The current Python helper uses legacy API routes. The official migration documentation describes
a newer direction with incomplete one-to-one replacements. That difference is explicitly carried
into CAP-03; it is not resolved by assuming either old or new routes work universally.

## Refresh triggers and open evidence

Refresh external contracts when selecting pinned dependencies, adding a supported Grafana/client
version, changing process execution, adding a credential provider, or implementing remote transport.
Capture exact versions and source revisions in implementation review evidence.

Still unverified: executable name availability, Rust dependency selection and dependency licenses, actual host
support, live target access, credential isolation, performance benefit, adoption benefit, extension
security, distributed recovery and all product acceptance cases. The plan gives these gaps owners
and tests; it does not replace them with inferred readiness.
