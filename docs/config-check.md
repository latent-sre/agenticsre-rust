# Declared offline configuration checks

`config.check` checks one explicitly declared configuration kind using an installed, versioned
validator. `grafana-dashboard` uses `dashboard-hygiene` version 1;
[`prometheus-rules`](alert-rules.md) adds the explicitly bound native `promtool` adapter.
The dashboard behavior described below is specific to its declared kind.
This reuses the [fixed dashboard task](tasks.md), its ten textual rules, supervised Python runtime,
input bounds and pinned upstream provenance. It never establishes deployment or live-health
correctness. Additional formats and validator versions remain separate implementation work.

```sh
save --json config check --input check.json
save config check --input check.json --fail-on-findings
```

`check.json` contains the [typed input](sre-workbench/schemas/config-check-input.schema.json):

```json
{
  "kind": "grafana-dashboard",
  "validator": {"id": "dashboard-hygiene", "version": 1},
  "file": "/absolute/path/dashboard.json"
}
```

The command accepts `--timeout-ms` (default 30000) and `--max-output-bytes` (default 1048576).
The normalized `config.check` operation uses version 1, local workstation target and `record=never`.
Use the same inputs in a standard structured request through `save --json call --request request.json`.
Supported content is a raw legacy dashboard, a legacy `{dashboard,meta}` response or the
fixture-backed app envelope `kind: Dashboard`, `apiVersion: dashboard.grafana.app/v1beta1`.
Unknown app envelope versions/kinds and ambiguous mixed envelopes are unsupported. Other
legacy dashboard properties outside the ten textual rules remain explicitly unchecked.
Unknown fields in the invocation, explicit null, arrays in place of objects, unsupported kinds/validator versions and
relative model paths are refused. Caller-supplied programs, scripts or validator registration are
not part of the interface. Linux is the supported execution lane; hosted grants remain unavailable.

## Identity and results

Optional `validator.expected_binding_sha256` pins the embedded validator binding. Its mismatch
is refused before any model read or interpreter launch. Optional `expected_input_sha256` pins
the configuration bytes. The fixed wrapper reads the file once under the supervised deadline,
computes its digest and compares the pin before decoding or applying rules. It validates the same
bytes that it hashed; a pathname is not treated as immutable identity.

The result's [data contract](sre-workbench/schemas/config-check-data.schema.json) reports:

- Declared kind, validator ID/version, upstream revision, checker and binding SHA-256 identities.
- Observed input path, SHA-256 and byte count. Bounded complete reads retain their identity even
  for malformed or unsupported content. Unread or oversized inputs have null digest/byte count.
- Implemented rule IDs, checked panel count, findings and truncation flags.
- `validation_state`: complete, partial, not_established or unsupported; runtime and process
  supervisor provenance where available.
- Explicit unsupported scopes: live datasource/query checks, Grafana rendering, plugin-specific
  options and dashboard properties outside the implemented rules. Encountered V2 models or unknown declared envelope schemas add their
  unsupported section explicitly.

An empty finding list is meaningful only with complete validation and at least one checked panel.
An empty or row-only dashboard fails as uncheckable. Unsupported V2 models, malformed JSON,
duplicate keys, invalid UTF-8, invalid field types and exceeded limits cannot pass by omission.
Common textual rules can inspect unknown plugin panels without claiming plugin-specific coverage.

A complete check with findings has execution `succeeded` and assessment `not_assessed`.
`--fail-on-findings` changes only its CLI exit to 1, preserving those result fields. Failure,
cancellation, timeout and incomplete output keep their own status and take precedence. Human text
and JSON use the same findings and identities. Raw model contents and exception bodies are never
echoed as errors.

## Bounds and authority

The existing task limits apply: regular files up to 2 MiB, nesting at most 32 containers, at most
1000 leaf panels, query strings at most 16000 characters and bounded finding counts/fields. FIFOs
and directories are rejected without blocking. Oversized or truncated child output is explicit;
partial results cannot claim complete validation. The original model is never written.

Python is selected through the fixed system binding and runs with isolated flags and the existing
minimal environment. The upstream checker remains unchanged and retains its license. This is an
operator-local fixed program with the invoking account's filesystem authority, not an arbitrary
validator runner or an account/network sandbox. No Grafana server, credentials or network query
are required. See [the roadmap](roadmap.md) for acceptance status and remaining kinds/platforms.
