# Local browser host

The Rust host serves the built frontend on `http://127.0.0.1:8787` and forwards
bounded UTF-8 commands to one owner of the existing `sqlolla::pipeline::Pipeline`.
It contains no second SQL parser or compiler. Java remains the Alloy API bridge.
Run `scripts/env.ps1`, build the frontend, then `cargo run -p sqlolla-host`.
`SQLolla_HOST_PORT` may select another local port; zero selects an ephemeral port.

`GET /api/status` returns `sqlolla.host.v1`, the fixed `gpt-6.1-sol` model identifier,
and a memory-only CSRF capability. JSON POST requests require the exact same Origin
and `X-SQLolla-CSRF`. `POST /api/command` accepts unchanged analysis requests or
`instances.start`, `instances.next`, and `instances.open` actions. Start yields the
first record; open its returned bundle ID to load the complete context. Rust emits
integer values directly; the browser must use its lossless JSON transport.

Input is limited to two MiB, canonical output to sixteen MiB, and at most four commands wait
behind the serialized worker. At most eight API requests, including AI requests,
are active at once. Transport errors never imply solver exhaustion.
Abandoning a queued response skips that work. An active request keeps the existing
parser/solver/core limits and can still finish after browser cancellation or the
120-second response deadline. Graceful shutdown joins that owner and reaps its
private JVM. Forced process termination is outside this graceful-shutdown claim.

The host executable supplies compiler provenance for its instance archives;
archives created by another binary require a new analysis. The private store uses
the existing administrator-selected location. Browser commands cannot change
paths or executable configuration. Provider credentials belong only to AI request
memory; they must never enter the canonical command, archive, log or export.
The checked `AssistanceBoundary` contracts describe conditional admission, not a
proof of this HTTP implementation, provider prose, or operating-system secrecy.

Run `cargo test -p sqlolla-host --locked` after loading the portable tool paths.
The live HTTP regression launches its own child test process with a nonce-named
temporary store, then removes that store after graceful shutdown. It exercises
the real parser and Alloy API through `instances.start/open/next`, both encoding
profiles, exact int64 values, typed NULL, and actual UNSAT exhaustion. The ignored
child test is invoked by the ordinary parent test; it is not an omitted gate.
