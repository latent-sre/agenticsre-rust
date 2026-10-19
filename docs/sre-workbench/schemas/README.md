# Draft machine contracts

These JSON Schema Draft 2020-12 files are proposed version 0.1 contracts. They are planning
artifacts, not an implemented or released API. Schema identifiers use local URNs; validation
registers them locally and never retrieves schema references over the network.

| Schema | Scope |
|---|---|
| [Common](common.schema.json) | Reusable target, limit, artifact and source definitions |
| [Request](request.schema.json) | Shared operation envelope; no caller identity or grant override |
| [Diagnostics pack input](diagnostics-pack-input.schema.json) | Typed body capture and stop/continue policy only |
| [Diagnostics run input](diagnostics-run-input.schema.json) | Fixed installed connectivity pack/version |
| [Diagnostics configuration](diagnostics-config.schema.json) | Trusted startup aliases, allowlisted destinations, resolver/CA/process bindings |
| [Diagnostics data](diagnostics-data.schema.json) | Five typed observations, prerequisite states, timings and provenance |
| [Process input](process-input.schema.json) | Explicit program, argument vector and absolute working directory |
| [Context input](context-resolve-input.schema.json) | Explicit service/environment selectors; no wire source or grant |
| [Context export](context-export.schema.json) | Bounded offline fixture declarations; separate dependency directions |
| [Context data](context-data.schema.json) | Selected metadata, observed source identity and usable/coverage state |
| [Grafana query input](grafana-query-input.schema.json) | Selected datasource/dialect and fixed timestamps |
| [Saved query input](query-run-input.schema.json) | Installed version, literal parameters, explicit datasource and absolute/relative window |
| [Saved query data](query-run-data.schema.json) | Definition digest, redacted parameters/resolved window and the original Grafana observation |
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
The verifier supports Draft 2020-12 at every embedded schema position, absolute references to
registered package IDs, and fragment-local pointers/anchors. Other dialect declarations and
unregistered resource references are rejected, including references in unused schema branches.

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

Evidence format 1 adds strict [collection input](evidence-collect-input.schema.json),
[reference input](evidence-reference-input.schema.json),
[stored payload](evidence-collection-payload.schema.json),
[terminal collection data](evidence-collection-data.schema.json) and
[manifest](evidence-manifest.schema.json). Runtime checks additionally enforce byte limits,
identity/count consistency, digest-to-path equality and artifact/receipt integrity.

`evidence-compare-input` selects two generated local references and an explicit normalization.
`evidence-comparison-data` describes bounded returned-observation differences and their
compatibility, omissions and source limitations; it does not establish full telemetry coverage.

The `runbook-*` schemas describe installed guided review inputs and bounded checkpoint summaries.
Recording and revision/source identity admission remain runtime checks.

The `change-{plan,status,approve,apply,reconcile}-input` schemas describe the named
Prometheus-rule file replacement consumer over the existing authenticated runner transport.
[Canonical plans](change-plan.schema.json) retain file and directory identities, exact original
and proposed bytes, validator receipts, expiry, executor identity and readback criteria.
[Change data](change-data.schema.json) reports the original attempt and verified or uncertain
effects. [Runner startup configuration](runner-server-config.schema.json) supplies private ledger,
target paths, validator binding and distinct requester/approver grants. Input selectors never
grant authority. Planning, approving, applying and reconciling require `record=always`;
status supports `record=never`. Digest equality, canonical base64 pad bits, decoded byte limits,
authorization, expiry and inode readback require runtime validation.
