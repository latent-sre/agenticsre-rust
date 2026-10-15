# Remote runner transport decisions

These decisions support [WORKBENCH-023](roadmap.md#workbench-023--authenticated-remote-runner-experiment).
They define implementation and verification targets, not evidence of deployed remote access.

## Sources checked on 2026-10-11

- The current [TLS 1.3 specification, RFC 9846](https://www.rfc-editor.org/info/rfc9846/),
  particularly sections 2.3 and 8, describes early-data replay risk and the application's continuing
  responsibility for duplicate requests. The runner will use completed handshakes and no early data.
- [Rustls client certificate verification](https://docs.rs/rustls/latest/rustls/server/struct.WebPkiClientVerifier.html)
  documents mandatory CA-verified client certificates through the verifier builder. Anonymous
  authentication is a separate opt-in and is not part of this profile.
- [Rustls peer certificate access](https://docs.rs/rustls/latest/rustls/struct.CommonState.html)
  provides the peer certificate chain. The profile maps the verified leaf certificate's SHA-256
  to an explicitly configured principal, after the handshake completes.
- The repository locks Rustls **0.23.45** in [Cargo.lock](../Cargo.lock), checksum
  `0d41d731c7d2f962d1ccc364cec258de3c0e93b38c2fb3ba97ac74513048d634`.
  Its local crate source confirms the relevant `WebPkiClientVerifier`, `peer_certificates`,
  `enable_early_data`, resumption and server ticket configuration APIs. Online `latest` pages can
  display other point versions; implementation must compile against the locked local source.

## Selected profile

Use TLS 1.3 with mandatory mutual certificate authentication and ALPN `save-runner/1`. Both peers
use explicitly configured CA certificates; the controller also verifies the configured DNS server
name and exact server leaf certificate digest. The server independently requires its authenticated
client's leaf digest to appear in the principal configuration. A CA-valid but unlisted client gets
no application authority. Leaf pins supplement normal certificate validation, never replace it.

Disable early data, resumption, session tickets and server half-round-trip application data. Each
connection therefore establishes fresh authentication before any operation frame. This is the
project's chosen initial profile, not a claim that every alternative is unsafe. Certificate rotation
requires explicit configuration changes while preserving the configured principal's durable identity.

Endpoints are explicit numeric socket addresses. The separate TLS server name supplies certificate
identity checking; no ambient resolver, proxy, platform trust store or controller credentials are
forwarded. Private keys are bounded private PKCS#8 DER files selected by startup configuration.
Certificates use bounded DER files. No request field can select those paths or replace a verifier.

Each connection carries one request and one response, each prefixed by a four-byte network-order
length. Request bodies are at most 65,536 bytes and responses at most 2,097,152 bytes. JSON decoding
rejects duplicate keys, invalid UTF-8, excessive depth, unknown fields and positional arrays.
The client must verify the server leaf pin before writing a request. The server maps the verified
client pin to a principal before considering any operation. Extra frames are never dispatched.

One absolute deadline covers connect, TLS, frame reads and frame writes. Partial reads cannot reset
that deadline. The server separately bounds concurrent connections and active workers. A completed
transport write does not prove durable admission; lost replies yield uncertainty and read-only
lookup of the same principal/request ID. TLS alone does not provide admission deduplication.

## Required evidence

Exercise real mutually authenticated controller/runner exchanges in the existing isolated test
network. Include missing/untrusted/expired certificates, wrong server name and pin, unknown client
pin, protocol/ALPN mismatch, slow/truncated/oversized frames, disconnects and bounded shutdown.
Prove that authentication refusal produces no admission or child effects. Then test durable
principal isolation, admission deduplication, lost acknowledgment, cancellation and restart through
the same transport. Live host deployment and native Windows conformance remain separate requirements.
