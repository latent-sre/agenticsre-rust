# Browser evidence collection

WORKBENCH-033 adds three independent grants to the local browser workbench:

| Startup grant | Browser check | Effects |
|---|---|---|
| `evidence.packs.list` | Evidence pack catalog | Installed metadata only |
| `evidence.packs.describe` | Inspect evidence pack | Installed metadata only |
| `evidence.collect` | Collect evidence | Two Grafana observations and private store writes |

Catalog checks need no connection, context, provider or store. The installed pack is
`service-queries` version 1. Collection requires every launcher prerequisite below:

```sh
save --config /absolute/grafana.json \
  --context-source /absolute/context.json --context-max-age-days 30 \
  --state-dir /absolute/existing-private-store \
  ui serve --allow evidence.packs.list --allow evidence.packs.describe \
  --allow evidence.collect --collection-target reviewed-target
```

`--collection-target` is independent of query and diagnostic target grants. Startup refuses
missing, uninitialized or unsafe stores without creating them. Use the existing operator
collection workflow to initialize the store. Granting collection authorizes fixed `record=always`
for that operation, with the existing one-GiB/seven-day store policy. The adapter's global record
option remains `never`; per-request recording, retention and quota overrides are refused.
No store management operation is granted.

The form selects a granted alias and takes exact service/environment selectors, optional team
and deployment, separate metrics/logs datasource UIDs and literal jobs, a common window,
continue/stop behavior and an optional bare 64-character pack SHA-256. It cannot provide paths,
credentials, endpoints, expressions or arbitrary steps. Relative windows resolve once at actual
collection start. Both child observations share that window and the parent deadline.

The browser defaults to 60 seconds and 2,097,152 output bytes for collection, with lower explicit
limits permitted (at least 1 ms and 1,024 bytes). Catalog checks use the standard 300-second ceiling.
Concurrency and item limits are not accepted. Requests use the existing authenticated same-origin
run API, cancellation, bounded session history and submission identity. Repeating an identical
submission ID returns its existing handle; changing inputs under that ID returns a conflict.
A malformed or mismatched pack pin is refused before admission without network or store effects.

The server constructs the existing opaque core collection grant. It pins exact config/CA and
context-source bytes, the context age policy, selected aliases and the initialized store identity.
Each invocation checks the consumed files and actual store descriptor before retention or HTTP.
Changed resources require a new session. Credentials independently authorize `grafana.query`;
context metadata grants no network authority. Generic hosted execution still refuses collection.

Results show the capture state, per-step outcomes and omissions, resolved window, provenance and
sealed bundle ID/digest when available. Partial and cancelled runs can retain evidence. Complete
receipt retention does not establish telemetry completeness or service health. Returned content
stays inert, untrusted text. Download result preserves the exact API receipt bytes; it does not
export the entire bundle. Session history is temporary; sealed bundles remain in the private store.

Reading a saved bundle requires a separate `--allow evidence.open` grant, either in the same
session or a new offline session. Collection does not imply list, read, compare or report access.
The existing [offline evidence views](investigation-ui.md) verify manifests and artifact digests.
See [acceptance evidence](evidence-ui-verification.md). Live targets, native Windows, additional
packs, other recording policies and remote/team access remain separate work.
