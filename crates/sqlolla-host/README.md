# Local browser host

The Rust host serves the built frontend on `http://127.0.0.1:8787` and forwards
bounded UTF-8 commands to one owner of the existing `sqlolla::pipeline::Pipeline`.
It contains no second SQL parser or compiler. Java remains the Alloy API bridge.
Run `scripts/env.ps1`, build the frontend, then `cargo run -p sqlolla-host`.
`SQLolla_HOST_PORT` may select another local port; zero selects an ephemeral port.

For private IIS publication, set `SQLolla_PUBLIC_ORIGIN` to the canonical HTTPS
origin without a path or trailing slash. The Rust host still listens on IPv4
loopback. IIS must preserve the actual `Host` and `Origin` headers. The host
uses those headers for admission and ignores forwarding metadata. The site's
private perimeter controls who can access its shared evidence store.
[Private IIS package](../../deployment/iis/README.md)

`GET /api/status` returns `sqlolla.host.v1`, the legacy OpenAI model field and a
memory-only CSRF capability. JSON POST requests require the configured `Host`
and `Origin` plus `X-SQLolla-CSRF`. `POST /api/command` accepts analysis requests
and `instances.start`, `instances.next` and `instances.open`. Start yields the
first record. Open its bundle ID to load the complete context. Use the browser's
lossless JSON transport to preserve exact integers.

`refinement.context` validates a saved base and the submitted active review
snapshot, returning its canonical digest. `refinement.check` takes the same
snapshot, digest and candidate SQL. Both require at least one Yes or No. A Yes
requires exact occurrence of the archived behavior on the same input. A No
requires absence from the complete outcome set on that input. Unsure is
advisory. The candidate is ready only after those checks and the unchanged
bounded property pass with the original output schema and execution basis.
The browser compares the result with its current review and prompt before
staging the checked request as a draft. [Refinement contract](../../contracts/m2/README.md)

`POST /api/ai/byot/explain` summarizes a saved instance. `POST /api/ai/byot/refine`
regenerates SQL with the retained prompt and canonical reviews after explicit
unredacted disclosure. The `draft` and `model-draft` routes under the same prefix
compile first drafts. These routes require `X-SQLolla-Provider` and a request-only
`Authorization: Bearer` credential. The provider is one of `openai` (`gpt-6.1-sol`),
`anthropic` (`claude-sonnet-5-5`), or `gemini` (`gemini-4-hydrogen`). Each must
serve its exact pinned model. AI refinements still go through `refinement.check`.
Older requests on `/api/ai/*` without a provider header retain the OpenAI default.
The retired `/api/ai/propose` endpoint returns `ai.feedback_required`.

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
