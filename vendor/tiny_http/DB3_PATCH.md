# Local tiny_http patch

This directory vendors the source, normalized package manifest, README and both
license texts from the published `tiny_http` 0.12.0 crate. Upstream:
https://github.com/tiny-http/tiny-http. It is dual-licensed MIT OR Apache-2.0;
the original license files and source notices are retained. Examples (including
example private keys), integration tests and package-cache metadata are omitted.

The listener behavior change is in `src/lib.rs`, the accept loop: retry
`WouldBlock`, `TimedOut` and `Interrupted` rather than terminating the accept
thread. The application configures two-second socket I/O idle timeouts, inherited
by accepted Unix sockets; those timeouts also affect accept on the listener. The
unpatched crate exits after an idle two-second accept and leaves the UI offline.
Ignoring the error only in the application's receive loop cannot restart the
already-terminated accept thread.

The application regression
`idle_listener_survives_socket_timeouts_and_accepts_new_requests` waits 4.5
seconds without requests, then verifies process liveness and the authenticated
health endpoint. `incomplete_http_headers_expire` separately verifies the
inherited client idle bound. This is an idle timeout, not a wall-clock deadline
against continuous trickle headers. Replace this vendor patch with an upstream
release containing equivalent behavior when available, retaining both regressions.

Two scoped lint annotations also retain the upstream unused internal re-export
and compile-time Send/Sync assertion without warnings on newer compilers; they
do not change runtime behavior.

No registry cache or global dependency installation is modified. Normal locked
Cargo source builds/installations use this path dependency and require no Node.
Do not publish the crate to a registry without reviewing how Cargo rewrites path
dependencies, since an unpatched registry dependency would reintroduce the bug.

## Opt-in db3 bodyless protocol

`Cargo.toml` adds the non-default `db3-read-only` feature; the root application's
path dependency explicitly enables it. `src/client.rs` rejects any
Transfer-Encoding and any positive or malformed Content-Length with HTTP 400
and closes the connection **before** creating a Request or body/data reader.
Only digit-only zero Content-Length is accepted. This avoids EqualReader's
unread-body drop/drain and declared-length allocation path, including slow
bodies, absent body bytes, huge lengths and chunked framing. Merely ignoring
outer errors cannot avoid a body-reader destructor blocking shutdown.

With the same opt-in feature, request/header lines are bounded to 8 KiB,
header fields to 64 and total header-section bytes (including CRLF) to 32 KiB.
The request line has its separate 8 KiB bound. Oversized input returns 400 and
closes. These are memory/input bounds, not an absolute trickle deadline; the
application's two-second socket **idle** timeout remains in effect.

Without this feature, upstream body/header handling remains unchanged.
`client::test::upstream_body_handling_without_db3_feature` exercises a real
body-bearing request with the feature disabled. Application real-HTTP
regressions cover rejection, bodyless GET/HEAD, header budgets, and bounded
SIGINT/SIGTERM shutdown with a still-open client attempting writes every 200ms.
The prior idle-listener retry and incomplete-header idle regressions remain.

Complete locally changed upstream source list: `src/lib.rs` (accept retry and
scoped Send/Sync lint allowance), `src/util/mod.rs` (scoped re-export allowance),
`src/client.rs` (feature-gated body/header policy and compatibility test).
The normalized vendor and root Cargo manifests enable the feature explicitly;
this provenance document is local documentation, not upstream source.
