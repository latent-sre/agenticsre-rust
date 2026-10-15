# Prometheus rule-file validation

`config.check` supports `kind: prometheus-rules` with the installed `promtool` adapter version 1.
The adapter invokes exactly the official Prometheus **3.15.0 Linux amd64** `promtool` binary.
It checks alerting and recording rules without a Prometheus server. The upstream
[rule checker](https://github.com/prometheus/prometheus/blob/v3.15.0/cmd/promtool/main.go) and
[rule format validator](https://github.com/prometheus/prometheus/blob/v3.15.0/model/rulefmt/rulefmt.go)
define the implemented behavior; the selected release is [3.15.0](https://github.com/prometheus/prometheus/releases/tag/v3.15.0).

## Invocation and dependency

Supply the executable as trusted operator startup configuration, separately from the request:

```sh
save --json --promtool /absolute/path/promtool config check --input check.json
save --promtool /absolute/path/promtool config check --input check.json --fail-on-findings
```

```json
{
  "kind": "prometheus-rules",
  "validator": {"id": "promtool", "version": 1},
  "file": "/absolute/path/rules.yml"
}
```

The existing timeout/output limits, structured `call` envelope, local workstation target and
`record=never` apply. Optional `expected_input_sha256` and
`validator.expected_binding_sha256` pin the consumed input and installed adapter binding.
`--promtool` cannot be supplied through wire input or inherited by UI/MCP startup. There is no
PATH lookup, arbitrary executable/argument override, automatic download or post-install hook.
Other builds, versions and platforms are refused. Python is not required for this kind.

Dependency identity for an operator-provided executable:

| Item | Identity |
|---|---|
| Upstream revision | `5241a27fe3c6983549fccc32f6e65917408c63cd` |
| Release archive | `prometheus-3.15.0.linux-amd64.tar.gz` |
| Archive SHA-256 | `2a542df32eac02ee17b9d844fb2aa1de00dafa5476579ba8a3ba862e9d572ea0` |
| Extracted promtool SHA-256 | `c736d55d3ccd959fe48329965fb5cb671d45585a9483954766448035100ff75c` |
| Extracted executable bytes | `119146763` |

The local acceptance fixture was downloaded from the release above, checked against the release
checksum file and archive digest, and extracted into `target/alert-validator/upstream`. No host
package or service was installed. Release/dependency clearance for broader distribution remains
separate; the product does not bundle this executable.

## What is checked

The fixed invocation is `check rules --lint=duplicate-rules --lint-fatal` against one immutable
snapshot. Unknown YAML fields remain errors. The pinned upstream validator checks rule structure,
PromQL syntax, labels and recording-rule field restrictions, and parses alert templates. Its
duplicate-rule lint is included. It does not evaluate expressions against time series, establish
metric existence, execute templates, test alert delivery/routing or prove live configuration.

Each result identifies the adapter and executable digest/revision, the actual consumed input
digest/size, implemented rules, unsupported scopes and native child exit. Diagnostics are bounded,
marked untrusted and may contain fragments of the supplied rules. They are not commands, authority,
or service-health facts. Typed findings summarize the adapter outcome; `findings_total` counts
those summaries rather than pretending to count every upstream diagnostic.

| Native result | Adapter behavior |
|---|---|
| Exit 0, exact recognized success output, positive rule count, no stderr | Complete validation, outcome `valid`, CLI exit 0 |
| Exit 1 with recognized rejection | Failed check, outcome `invalid`, CLI exit 1; original diagnostics retained |
| Exit 3, recognized duplicate lint, no incomplete-document warning | Complete lint findings, successful operation with native child exit 3 retained; `--fail-on-findings` gives CLI exit 1 |
| Zero rules | Failed/uncheckable; no clean pass by omission |
| Multiple-document warning | Partial/unknown, even when native exit is 0 or 3 |
| Truncation, timeout, cancellation or unknown protocol | Explicit incomplete/failure outcome; no complete-valid claim |

Upstream processes only the first document in a multi-document YAML file and emits a warning.
This adapter refuses to call that complete. Keep one rule document per invocation. Rule files are
not server configuration files, Alertmanager configuration, Kubernetes objects or arbitrary YAML.
Use the declared kind and explicit supported version rather than assuming future formats work.

## Input and execution integrity

The executable is opened and copied into a private, unlinked file while hashing. Only the expected
ELF bytes may execute; all writable snapshot descriptors are closed before launch, and a pathname
replacement of the original cannot change the copied bytes. The rule
file is read once, bounded to 2 MiB, hashed and copied to another sealed in-memory file. The optional
input pin is checked before child launch. The validator receives only that inherited file descriptor,
not the original path. Neither original is written; no snapshot files remain on disk.

Final symlinks are refused for both inputs; supply the actual regular executable and rule file.
The executable snapshot uses normal file execution supported by the host profile; it does not
require executable memory files or changes to AppArmor. Private temporary names are removed by
the adapter; an abrupt crash during initial creation can require local cleanup. Same-account
process inspection/tampering is outside this operator-local boundary.

The native process uses the existing supervisor, fixed environment, closed stdin and process-group
cleanup. Input/binary preparation and execution share a deadline. Native diagnostics are bounded
and final receipts remain within the common encoded limit. This binding uses ordinary account
authority; it is not an account/filesystem sandbox. Fixed rule validation requires no network
request, and acceptance runs inside the existing private network namespace.

The common [configuration contract](config-check.md) and
[Prometheus data schema](sre-workbench/schemas/prometheus-config-data.schema.json) describe the
wire boundary. See [the roadmap](roadmap.md) for current acceptance and remaining product scope.
