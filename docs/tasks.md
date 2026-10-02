# Fixed offline tasks — first implementation contract

This increment follows the accepted Linux command-runner PoC. Its live status belongs in
[WORKBENCH-002](roadmap.md#workbench-002--fixed-offline-tasks-and-adapter-foundations).
The source behavior comes from the pinned [adapter baseline](adapter-notes.md).
The second task has its own [error-budget contract](error-budget.md), sharing these fixed-runtime
and supervision boundaries with different typed inputs and results.

## Public operations

- `save task list` and `save task describe dashboard-hygiene` discover installed task metadata
  without executing a task, interpreter probe or network call.
- `save [--json] task run dashboard-hygiene --input INPUT.json [--timeout 30s]
  [--max-output-bytes 1048576] [--fail-on-findings]` invokes the fixed version-1 task.
- The input file contains `{"file":"/absolute/path/dashboard.json"}`. Human CLI convenience may
  resolve a relative model path against its invoking directory; normalized structured calls
  require the absolute path. The input file itself is a bounded regular JSON file.
- A structured call uses operation `task.run` version 1 with
  `{"id":"dashboard-hygiene","version":1,"input":{"file":"/absolute/path/dashboard.json"}}`.
  No executable, script, digest, runtime, environment or permission override is accepted.
- `task.list` uses empty inputs; `task.describe` uses `{"id":"dashboard-hygiene"}`. Unknown task
  identities or versions are refused. This increment adds no task registration or installation API.

All operations use the same request/result envelope, strict controls, record=never and local
workstation target as the PoC. Successful checks with findings have execution=succeeded and
assessment=not_assessed. Findings are typed data, not parsed terminal prose. The optional
`--fail-on-findings` changes only the CLI exit to 1 for a completed check with findings; it does not
change execution status. Operational failure, timeout, cancellation and partial output retain
their usual exits and take precedence. A check with incomplete coverage cannot claim success.
Human text summarizes checked panels and finding counts, rule/location/detail and coverage limits.
JSON retains the full runtime, binding and supervisor provenance. Both views derive from one result.

## Installed binding and execution

Embed the reviewed checker and fixed structured wrapper as resources in the Rust binary, with
reported SHA-256 identity and upstream revision. An installed binary must work away from this
checkout, without reading a same-named workspace script. Python receives fixed source and literal
value arguments; caller input is never Python or shell source. Retain the upstream script unchanged
and its MIT notice; implement the structured seam in a separate wrapper.

Select Python from fixed system installation paths; no PATH, PYTHONPATH, environment or request
override can replace it. Disclose the selected executable. Python is an optional dependency,
minimum 3.11; missing runtime returns missing_dependency and unsupported versions are explicit.
Metadata inspection may report an executable present without claiming its version was probed.
Linux execution is the first supported lane; other platforms remain unavailable until tested.

Run Python with isolated mode, disabled site initialization and bytecode writing (`-I -S -B`),
closed stdin and the existing minimal environment through the verified supervisor. Native API
references: [Python command-line isolation](https://docs.python.org/3/using/cmdline.html#cmdoption-I)
and [site initialization](https://docs.python.org/3/using/cmdline.html#cmdoption-S).
Preserve the process deadline, output/cancellation bounds and cleanup semantics. File reads and
the check must be inside the supervised operation. This remains an operator-local tool with
ordinary OS authority; fixed code binding is not an account or network sandbox.

## Input and results

Read at most 2 MiB of model bytes from a regular file. Reject FIFOs/directories, invalid UTF-8/JSON,
duplicate keys, invalid field types, excessive nesting and unsupported V2 models. Limit model
nesting to 32, leaf panels to 1,000 and query text to 16,000 characters. Validate the shapes consumed
by the checker before calling it. Empty/row-only models with zero checked panels are uncheckable.
Keep the original file unchanged. Report its actual observed digest and checked panel count.

Preserve the current checker rules and exceptions: datasource-specific PromQL heuristics, quoted
literal masking, target datasource overrides and `increase(...[$__range])` totals. Unknown plugin
panels can receive common textual rules without claiming plugin-specific or query validation.
Findings retain rule, location and detail, alongside explicit coverage/limitations and source
identity. The result does not establish Grafana rendering, query correctness or live health.

Validate the structured child response and its task/version before accepting it. Reserve a bounded
structured-data budget within the 2 MiB final receipt ceiling; do not duplicate a large JSON payload
as both stdout and parsed data. Finding/field truncation or capture loss must be explicit and
partial. Malformed or truncated task output fails safely. Known static error messages must not
echo arbitrary exception bodies, input file contents or environment values.

## Acceptance

Exercise clean input, findings with both exit policies, legacy/app-platform wrappers, nested rows,
mixed backends, quoted query content and selected-range totals through actual CLI/core behavior.
Check equivalent structured calls and validate task metadata/results against schemas. Reuse
upstream fixture expectations and add independent malformed/empty/V2/large input cases.

Prove hostile cwd scripts/modules and parent Python environment cannot replace the binding; missing
runtime and timeout/cancellation cannot become successful checks; input bytes do not change;
isolated installation works without the source checkout; and source/encoded output limits hold.
Run the existing PoC regression suite for shared-core/CLI changes. Use Cally's bounded offline job
with a pinned Python-capable image for positive task tests and its minimal image for missing-runtime
behavior. Native Windows, live Grafana and agent-protected permissions remain separate work.
