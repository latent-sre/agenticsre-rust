# Contract examples

The files in this directory are synthetic specification fixtures. Nothing here is an installed
configuration, real service observation, approved task or valid execution credential.

[cases.json](cases.json) is the authoritative test manifest: it names each instance, target
schema, expected validity and reason. Positive examples illustrate commands, Grafana envelopes,
capability discovery, success/denial/partial/unknown-effect results, events, configuration,
workflow, task, query and bundle metadata. Negative examples demonstrate structural rejection of
role injection, unknown versions, contradictory denial/effect status, unexpected secret fields,
artifact traversal and invalid limits.

The imported package adds an invalid-timestamp fixture. The verifier must have RFC3339 date-time
checking installed and must reject that value; an unavailable format plugin is a validation failure.

The command request also validates its inputs against the example capability's input schema.
Grafana query inputs also validate against their shape schema and still require the eventual
adapter's semantic checks; structural validation does not
establish the target, query dialect, range order, permissions or supported API version.

The task example describes the intended structured adapter seam; the current Python helper does
not yet emit that proposed JSON shape. Query placeholders use double braces around a declared
parameter name; a string parameter becomes an escaped quoted literal in the selected dialect.
Definitions with other template syntax, undeclared parameters or unsupported dialect features
must be refused by the eventual renderer.

The bundle's artifact path/size/digest are illustrative and do not name a real bundled payload.
Its complete flag is false. A real importer verifies contained files and their bytes before
accepting a bundle. The unknown-effect example requires reconciliation and is never a retry plan.

Use [verify_specs.py](../verify_specs.py) to validate the structural examples without running any
product operation. Runtime cases remain in [the acceptance plan](../verification.md).
