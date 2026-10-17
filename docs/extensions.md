# Reviewed external capability packages

`extension.inspect`, `extension.install` and `extension.list` manage offline package bytes on
Linux. This is the package-management foundation of CAP-17. Installation does not enable execution.
The separate [execution contract](extension-runtime.md) requires an explicit operator policy and has
[Linux x86-64 acceptance](extension-runtime-verification.md). An accepted package alone is not proof
its executable implements the protocol.

```sh
save --json extension inspect /absolute/path/capacity.save-package.json
save --json --extension-state-dir /absolute/private/extensions extension install \
  /absolute/path/capacity.save-package.json --expected-package-sha256 sha256:REVIEWED_PACKAGE_DIGEST
save --json --extension-state-dir /absolute/private/extensions extension list
```

Each command accepts `--timeout-ms` and `--max-output-bytes`. Structured callers use the same
operation names, local workstation target and `record=never`. Package installation persists
package data; it does not record an evidence receipt. The store path is explicit operator startup
configuration. Hosted UI/MCP callers cannot inherit it or use these operations.

## Package format and identity

A package is one UTF-8 JSON file, at most 12 MiB and 16 nested containers. There is no archive
extraction, filename mapping, dependency installation, download, hook or embedded command-line
configuration. Unknown fields, duplicate keys and unsupported format/protocol versions refuse.
The [package schema](sre-workbench/schemas/extension-package.schema.json) contains:

- `package_format: 1`;
- a [manifest](sre-workbench/schemas/extension-manifest.schema.json), at most 64 KiB serialized;
- `executable_base64`, canonical standard Base64 for at most 8 MiB of executable bytes.

The manifest identifies the package/version, `save-extension` protocol version 1, exact envelope
compatibility range `0.1` through `0.1`, Linux x86-64 ELF header identity, executable length/digest,
one to eight namespaced capability declarations and supplied source/revision/license provenance.
The executable digest covers decoded bytes; the package digest covers the exact consumed JSON
bytes, including whitespace. Installation requires the latter as an explicit pin. Inspection may
also supply it. A pin establishes identity, not trust, signing, provenance verification or permission.
Paths and provenance strings also have UTF-8 byte budgets and reject Unicode control characters;
schema string-length limits are upper bounds, not an exemption from those runtime checks.

Capability names must start with `ext.<package-id>.`; each has version 1, declared effect `observe`
and an empty requested-access list. These declarations are untrusted metadata. The host does not
register or dispatch them solely because they were installed. Without a policy, `enabled` and
`execution_available` remain false. Inventory can report effective authorization from an explicitly
attached policy, with its digest; standalone inspection always reports inert package metadata.
`provenance_verified` remains false. The ELF check verifies header identity only, without loading code.

The initial schema subset is a closed object of at most 16 required properties. Each property is
a bounded string, a bounded safe integer or a boolean. References, remote schemas, arrays, optional
properties, recursive schemas, formats and unknown keywords are unsupported in this version.
Runtime also checks relationships that the published schemas cannot express, including matching
required/property sets, ordered bounds, namespaced capability names and matching payload digests.
This finite subset is intended for the next capacity-projection consumer; extending it needs a
concrete consumer and compatibility checks, not implicit acceptance of arbitrary schema behavior.

## Private installation and recovery

Package source paths are absolute. Reads refuse symlinks in any path component, nonregular files,
hardlinks, excessive size and expired/cancelled invocations. The package is read once; the digest
and installed bytes refer to that same bounded buffer. Validation and pin checks precede store
creation. Inspection never creates or opens a package store.

The explicit store uses owner-only directories and nonexecutable owner-only files, a format marker,
a nonblocking lock and content-addressed package files. It permits at most 32 packages and 64 MiB
total package/staging bytes. A completed installation syncs bytes before atomic publication and
syncs the directory afterward. Reinstalling the same bytes is idempotent; different package bytes
with the same ID/version refuse replacement. No implicit upgrade, eviction or enablement occurs.

Listing and installation revalidate installed package bytes and their identities. Unknown files,
incomplete initialization, abandoned staging or corrupt packages produce an explicit recovery-needed
failure and remain untouched. Automatic repair/removal is not implemented in this foundation.
Preserve the directory for inspection, identify the failed installation and restore a known verified
store or choose a new empty private store. Do not delete unexplained entries to force acceptance.
Errors after local writes preserve uncertainty about those effects; they never imply clean rollback.

Results omit executable payloads. Descriptions and counts are bounded; omitted descriptions produce
an explicit partial result with `omitted_packages`. Provenance and metadata remain untrusted text.
The store is an operator-local integrity boundary, not protection from the same account or root.

## Execution has separate authority and acceptance

The [external process contract](extension-runtime.md) uses versioned JSON Lines, a handshake before work, correlated
request/run IDs and sequence numbers, bounded progress/diagnostics, exactly one terminal result,
cancellation and hard-stop behavior. Installation supplies none of that authority. A future execution
binding must verify the same pinned package under independent operator policy and prove malformed
frames, incompatible peers, replacement and old-consumer refusal before being enabled. The fourth
distinct capability experiment and remaining CAP-17 requirements stay on [the roadmap](roadmap.md).
