# Draft machine contracts

These JSON Schema Draft 2020-12 files are proposed version 0.1 contracts. They are planning
artifacts, not an implemented or released API. Schema identifiers use local URNs; validation
registers them locally and never retrieves schema references over the network.

| Schema | Scope |
|---|---|
| [Common](common.schema.json) | Reusable target, limit, artifact and source definitions |
| [Request](request.schema.json) | Shared operation envelope; no caller identity or grant override |
| [Process input](process-input.schema.json) | Explicit program, argument vector and absolute working directory |
| [Context input](context-resolve-input.schema.json) | Explicit service/environment selectors; no wire source or grant |
| [Context export](context-export.schema.json) | Bounded offline fixture declarations; separate dependency directions |
| [Context data](context-data.schema.json) | Selected metadata, observed source identity and usable/coverage state |
| [Grafana query input](grafana-query-input.schema.json) | Selected datasource/dialect and fixed timestamps |
| [Grafana dashboard input](grafana-dashboard-input.schema.json) | One dashboard UID |
| [Result](result.schema.json) | Execution, assessment, coverage, effects, output and evidence |
| [Event](event.schema.json) | Sequenced progress/event framing |
| [Capability](capability.schema.json) | Discoverable operation contract and requested permissions |
| [Configuration](config.schema.json) | Non-secret settings and connection references |
| [Workflow](workflow.schema.json) | Bounded pack/runbook definition |
| [Task](task.schema.json) | Fixed installed script/executable adapter metadata |
| [Query](query.schema.json) | Typed saved-query definition and bounds |
| [Bundle](bundle.schema.json) | Offline evidence manifest and artifact references |

The selected capability's input_schema and output_schema add operation-specific validation.
Embedded schemas must be checked against the meta-schema and resolve only approved packaged
references. A manifest cannot request arbitrary remote schema retrieval.

Capability output_schema validates the final result envelope. Task output_schema validates the
helper's structured payload before the core inserts it into result.data. The core owns identities,
timings and effective policy; a helper cannot replace them by returning extra fields.

Schema validation does not establish authorization, DNS/origin safety, path containment after
symlink resolution, actual artifact bytes, execution effects, graph acyclicity, time order,
secret absence or healthy service state. Those are semantic/runtime checks in
[shared contracts](../contracts.md) and the [acceptance plan](../verification.md).

Examples use synthetic values and zero digests where no actual artifact exists. Passing structural
validation never certifies such a digest or approves that task for execution.

Run the repeatable planning checks from the repository root:

~~~powershell
.venv/Scripts/python.exe -B docs/sre-workbench/verify_specs.py
~~~

The verifier reads only this package and linked local documents. It does not invoke tasks,
contact Grafana, inspect credentials, install a dependency or run a model.
