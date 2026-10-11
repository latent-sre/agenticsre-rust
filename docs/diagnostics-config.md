# Diagnostics configuration and receive profile

The startup-only `--diagnostics-config` file implements
[the connectivity contract](diagnostics.md). A request selects an exact alias, fixed pack and
typed input. Configuration contents do not become UI/MCP grants or protected operator identities.

```json
{
  "schema_version": 1,
  "targets": [{
    "id": "fixture-web",
    "host": "diagnostics.workbench.test",
    "port": 8443,
    "path": "/probe",
    "allowed_ips": ["127.0.0.1"],
    "resolver": {"kind":"explicit","address":"127.0.0.1","port":5353,"transport":"udp"},
    "tls_ca_file": "/tmp/fixture/ca.pem",
    "process": {"backend":"linux-process-v1","pid":17,"start_time_ticks":"123456","uid":1000}
  }]
}
```

This is a synthetic example, not a usable machine mapping. Configuration is capped at 64 KiB,
depth 16, 16 targets and 16 addresses per target. Fields are strict: objects cannot be positional
arrays; null, unknown fields, duplicate JSON keys and duplicate aliases are refused. The
[configuration schema](sre-workbench/schemas/diagnostics-config.schema.json) defines exact types.
All targets' declarations are validated; only the selected target's CA file is opened. The exact
configuration and additional CA bytes are hashed. Linux admission pins a regular inode with
`openat2` before creating its readable descriptor, refuses all symlink components, and has no
weaker fallback. Replacing a pathname during a run does not retarget its parsed snapshot.

`host` is an ASCII DNS name with ordinary letter/digit/hyphen labels, or an IP literal. DNS names
are normalized to lowercase without a trailing dot; queries append the absolute DNS root dot.
Special-use localhost/invalid/onion names remain outside this profile. Admitted names, including
reverse zones and CNAME aliases into them, are never synthesized locally: they require an answer
from the configured resolver. IP spellings normalize through Rust's IP parser. IPv4-mapped IPv6, scoped addresses, unspecified
and multicast destination/resolver addresses are refused. Allowlist duplicates after
normalization are refused. Every address associated with the service's matching A/AAAA query and
CNAME chain is checked before connecting: one denied address refuses the answer. Unrelated-owner,
other-class, authority and additional records cannot select a service peer. A wrong-family address
at a service-chain owner is an explicit `dns_answer_type_mismatch`, rather than an accepted answer.
The lowest sorted authorized address
is selected once and pinned across the TCP, TLS and HTTP connections. There is no alternate-peer
retry or fresh service-host lookup.

Paths must start with one slash, contain only the declared ASCII path characters, and be at most
1024 bytes. Dot segments, percent escapes, query/fragment syntax, whitespace and controls are
unsupported. Port zero, credentials and arbitrary schemes/headers are unsupported.

The resolver is either the explicit numeric endpoint above or
`{"kind":"system","transport":"udp"}` (also `tcp`). The system profile reads one bounded,
strict regular `/etc/resolv.conf` snapshot and selects its first nameserver on port 53; it does
not fall back to another server or to public DNS. It supports numeric `nameserver`, bounded
`search`/`domain`, and the documented ordinary resolver options; unsupported/malformed directives
are refused. Search/options do not override this pack's fixed lookup strategy. Hickory 0.26.3
uses its low-level `DnsExchange` to perform one A and one AAAA question, each with at most one
transmission and a maximum three-second endpoint timeout within the check's remaining time.
UDP retransmissions and resolver-level retries are disabled. TCP uses one connection for both
families and any alias questions; a closed connection is terminal, without reconnect or resend.
Hickory correlates response IDs and UDP source endpoints; this profile also admits only one
matching question, response role/opcode, NOERROR and an untruncated message. Truncation, error
responses or a failed family refuse the combined lookup; an empty NOERROR family is allowed.
Message and question fields follow
[RFC 1035 §4.1](https://www.rfc-editor.org/rfc/rfc1035.html#section-4.1).

Ordinary CNAME chains follow the owner/type association in
[RFC 1034 §3.6.2](https://www.rfc-editor.org/rfc/rfc1034.html#section-3.6.2).
Answer order and DNS label case do not affect association. Both transports allow at most eight
CNAME hops per family. A missing terminal answer starts one new question for that alias at the
same endpoint/transport, so there are at most nine questions per family (18 total), within the
existing DNS step and parent deadlines. Inline answers need no additional question. Repeated
names, conflicting CNAME targets or CNAME/address data at the same owner fail explicitly, before
a repeated question or peer selection. Aliases obey the same ASCII hostname profile.

Completed DNS outcomes disclose `max_cname_hops: 8`, `max_queries_per_family: 9`, no synthetic answers or
connection reconnect, and a family-grouped `questions` list of name/type requests handed to the
transport. A closed stream can reject a listed request before it is transmitted. A cancelled or
timed-out DNS future returns its terminal state without a partial question trace; some questions
may already have been sent. Successful DNS
observations also report `cname_chains.a` and `.aaaa` (original name plus any aliases). Hosts
files, shared caches, search domains, cross-transport fallback, opportunistic encryption and
DNSSEC validation are not enabled. Resolver provenance is separate from the service peer.
Literal IPs skip resolver work and report `literal_ip`, null resolver and null CNAME chains;
they still require the service IP allowlist.

TLS uses the original DNS name or IP and Rustls 0.23.45 certificate/hostname validation, bundled
`webpki-root-certs` 1.0.9 and optional additional PEM certificate roots (64 KiB, at most 16
certificates). CA PEM text outside certificate blocks is refused. Ambient CA/keylog variables,
client credentials and unknown-CA fallback are not used. Each TLS exchange records actual peer,
local socket, leaf SHA-256, negotiated TLS version/cipher and ALPN. No TCP result substitutes for
TLS validation; HTTP performs its own validated TLS exchange to the same peer.

The HTTP profile sends one literal GET with Host authority, Accept `*/*`, Accept-Encoding
`identity` and Connection `close`. It is an invocation-owned direct socket, with no proxy,
redirect follow, cookie jar, authentication or retry. Response framing follows the relevant
[RFC 9112 sections 5–8](https://www.rfc-editor.org/rfc/rfc9112.html#section-5), with deliberate
strict subsets: HTTP/1.0 or HTTP/1.1 only; no informational/protocol-switch responses, obsolete
folding, duplicate framing headers, combined Content-Length values, conflicting length/transfer
encoding, chunk extensions or nonempty trailers. Unsupported syntax produces an explicit failed
check. Chunked and Content-Length bodies are supported. Close-delimited bodies require a clean
TLS close-notify; premature transport EOF fails. Headers plus chunk framing/trailers total at
most 8192 bytes, including every chunk-size line, trailing chunk CRLF and final empty-trailer
line; body bytes have their separate limit. Each line is at most 1024 bytes, with 64 fields and
256 chunks. Exactly 8192 framing/header bytes is supported; a crossing fails before another
framing read. Only content-type and
content-length (each at most 256 bytes) are returned; error headers/bodies are not copied into
messages. HTTP non-2xx status is a failed observation, not a health assessment.

Bodies are read with a 16 KiB ceiling, limited further by the caller's output allowance, including
when capture is disabled, to detect incomplete framing within that bound. Beyond the limit the
socket closes and the HTTP check is partial with explicit truncation. Capture defaults to false;
no body bytes are returned then. Opt-in bytes use lossless base64, remain untrusted and are never
automatically redacted or decompressed. Identity/gzip/deflate/br/zstd content codings are recognized
as opaque bytes; unknown/combined encodings are refused. Encoded expansion cannot exceed the
64 KiB data ceiling or become a complete successful receipt after required observations are lost.

`process` is optional. The Linux backend pins one numeric proc directory, confirms kernel procfs,
reads bounded stat/status metadata twice and checks PID/start ticks and real UID. Ticks are a
decimal string to avoid loss in JSON consumers. The real UID is the configured identity; other
kernel UID fields are reported separately. Process name, command line and environment are not
read or returned. Running/sleeping/stopped/zombie/gone are observations only. Missing, inaccessible,
changed/reused or unsupported instances cannot substitute another PID or execute a command.
An absent binding is `unconfigured`; a future named backend is an explicit unsupported check.

The fixed graph has DNS → TCP → TLS → HTTP and independent process observation. Concurrency is
one or two, default two. Stop mode stops admission after an observed failure; an already active
check can finish within its bound. Continue mode retains independent process results and marks
dependents blocked. An omitted optional process binding is disclosed as unconfigured and does
not invent a failure that stops the configured network branch. One monotonic parent deadline includes config/CA admission and is at most
60 seconds; each admitted step is at most 10 seconds or the remaining parent time. Cancellation
and consumer closure drop sockets/futures and then shut down the invocation-owned reactor.
No child or blocking resolver worker is created. Coverage names omitted checks and never implies
healthy service state. The data schema and public receipt examples live with the other packaged
[machine contracts](sre-workbench/schemas/README.md).
The [illustrative data](sre-workbench/examples/diagnostics-data.json) shows the complete five-check
shape. Its zero identities are synthetic examples, not actual observations or verified artifacts.
The public mission runs through [the private fixture client](../tools/verify-diagnostics.py);
[the focused review client](../tools/verify-diagnostics-review.py) adds wire-count, CNAME ownership,
reverse-zone and exact HTTP framing witnesses. Run the clients only in the declared private sandbox:

```sh
tools/test-sandbox.sh .venv/bin/python -I -B tools/verify-diagnostics.py target/debug/save --json-results
tools/test-sandbox.sh .venv/bin/python -I -B tools/verify-diagnostics-review.py target/debug/save --validate --json-results
```

The review client's `--findings-only` retains the eight original red/green cases; `--validate`
uses the candidate's current data schema and should be omitted for a preserved R0 executable.

Regular configuration/CA/proc reads use bounded reads and monotonic checks, with Linux descriptor
admission. They cannot hard-preempt an uninterruptible kernel/filesystem call. Fixture success
establishes the tested local and Cally filesystems and kernel views; it does not prove a universal
kernel-stall deadline or native Windows behavior.
