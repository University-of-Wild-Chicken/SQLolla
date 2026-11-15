# SQLolla evidence review frontend

The workbench starts with an editable tenant-filter query and its finite rule. Choose
**Analyze query** for a real case from the same deterministic Rust and Alloy pipeline.
The SQL editor and typed query builder share this pipeline. Use Graph or Table to inspect
the result, or open **SQL / IR / Alloy** to see the source triple. Yes, No and Unsure save
reviews at the bottom of the page for later reopening, relabeling or withdrawal.

When the host opens a case with its complete SQL/IR/Alloy bundle, **Explain this result**
offers cited OpenAI assistance. The workbench offers optional AI setup on the first visit.
Save a key in **AI settings**, then
choose **Explain this case**. Saving the key does not test account access or send data.

## Run

Requires Node.js 22.12+ and npm.

```sh
npm ci
npm run dev
```

Build static assets and preview the editor without an analysis host:

```sh
npm run build
npm run preview
```

For the connected workspace, build the Rust/parser/Java components from the repository root,
then serve these assets from the same Rust host:

```powershell
. .\scripts\env.ps1
.\scripts\build.ps1
Set-Location frontend
npm ci
npm run build
Set-Location ..
.\target\debug\sqlolla-host.exe
```

Open `http://127.0.0.1:8787`. `SQLolla_HOST_PORT` changes the port; `SQLolla_ROOT` selects the
installation root. The process binds IPv4 loopback only. Vite preview can render the static
editor. The solver and AI require the same-origin Rust host.

Dependencies are pinned in `package.json` and `package-lock.json`. Fonts, the pixel logo and the
Graphviz worker are self-hosted; there is no CDN request.

## Verify

```sh
npm run typecheck
npm test                 # Vitest: adapter, session races, builder, journal, graph, AI, App
npm run check:functions  # 30-physical-line production-function gate
npm run format:check
npm run check            # all of the above, plus a production build
npm run test:e2e         # builds, starts Vite preview, drives installed Chrome
npm run test:host        # requires the running Rust host; performs real local solver work
```

`test:e2e` uses `playwright-core` with installed Chrome. Set `CHROME_PATH` if Chrome is elsewhere.
The preview test checks the editor, BYOT form and axe at desktop and mobile widths. Set
`SQLLOLLA_SCREENSHOTS` to save screenshots outside the source tree.

`test:host` runs real analysis, Graphviz, Next, exact case reopening, the review bins, refinement,
SQL copying, exact Int64/NULL rendering and the query builder's compile and stage flow. Use a fresh
test-only instance store. The default AI
test supplies a synthetic cited response, then checks that an Access-style sign-in redirect
leaves the solver case visible without following the redirect.
[`verifyHost({ url, chromePath, apiKey })`](e2e/host.mjs) supports an authorized live explanation
check with an in-memory key. Test output omits the key and generated prose.

## Local host boundary

`GET /api/status` returns the exact model and a memory-only CSRF capability. `POST /api/command`
passes existing strict commands to one serialized Pipeline, preserving the parser and Java Alloy
worker. Both the browser Origin and `X-SQLolla-CSRF` must match. Requests are capped at two MiB,
responses at sixteen MiB, and admission is bounded. No browser command selects executable paths.

`instances.start` returns the first record; the transport then calls `instances.open` to load the
matching canonical SQL/IR/Alloy bundle. Next is explicit and serialized. Reopening checks tool
fingerprints and replay; rebuilding a native binary can require a new analysis revision.
Browser cancellation discards stale results; it does not promise to terminate active solver work.

The lossless JSON adapter retains exact numeric lexemes and original response text. Imported
evidence remains separate from host-replayed evidence: local hashes and pointer checks do not
authenticate imports. Presentation DOT is derived from typed evidence and is not a second SQL
interpreter. Signed imports and authenticated multi-user hosting remain separate work.

## Refine a query

In a local bounded analysis, swipe right to label a saved observation Yes, swipe left for No,
or choose Mark unsure. Yes requires the exact observed behavior to remain possible on its saved
input. No requires that behavior to be absent on its saved input. Unsure provides context only.
The review bins retain compatible observations across query revisions. If a legacy label has no
bundle ID, reopen its original case from the library or analysis and save that label again to bind
its exact identity. The host checks compatibility and replays every saved case before refinement.

Regeneration and local candidate checks require an active Yes or No. Enter the original prompt
to regenerate with AI, or paste SQL into Candidate SQL to check locally without a key. The local
checker keeps the catalog, parameters, property, bounds, domain and execution settings and checks
the exact labeled cases. Use checked SQL as new draft appears enabled only after every decisive
case and the original bounded property pass. The editor stages the checked request for a fresh
analysis. The native editor, evidence inspector and candidate result expose Copy SQL and a manual
selection fallback.

Save an OpenAI key in the tab to request explanations or use Regenerate SQL. Generation reuses
the original prompt, requires unredacted context and sends the canonical SQL, cases and notes only
on that explicit action. The host uses OpenAI Responses with `gpt-6.1-sol` and `store:false`.
The browser invalidates pending results and adoption when the prompt, source or feedback changes.
Redacted explanations share the verified rule outcome and decisive fact when available.
SQL, IR, Alloy, scalar values and fact payloads stay local.

## Security and truthfulness boundaries

- Evidence is immutable. Feedback is a separate append-only browser journal with JSON
  export/import, idempotency keys, supersession and stale-revision/conflict detection.
- Accepted/rejected labels express review intent only. They are never evidence of safety; NULL is
  typed; duplicate occurrences are not collapsed.
- Status language distinguishes counterexample, example, exhausted, limit, unsupported, missing
  context, SQL error, replay mismatch, timeout, stale and archive failure. SAT is not “bad” and
  UNSAT is not “safe”.
- Imported evidence cannot inject HTML, images, links or executable DOT. Presentation DOT is
  derived locally from typed evidence, Graphviz runs in a pinned worker, and rendered SVG is
  allowlist-sanitized before insertion.
- Provider API keys are memory-only. They are never written to `localStorage`, URLs, exports, logs
  or evidence; Disconnect clears the tab's key. Host credentials live only for a request.
- Fixture and live summaries are labelled separately and rendered as untrusted text. Citations prove a supplied fact
  exists, not that prose is true, and the summary cannot change evidence, status or feedback.

## Graphviz provenance

`@viz-js/viz` 3.31.0 bundles Graphviz 16.1.0 as WASM. Rendering runs in
[`src/graph/graphviz.worker.ts`](src/graph/graphviz.worker.ts); the SVG passes through
[`src/graph/sanitizer.ts`](src/graph/sanitizer.ts) before insertion. The renderer reports Graphviz
16.1.0 in the graph provenance line. No CDN or remote renderer is used.

## Fixtures

Fixtures live in [`src/analysis/fixtures.ts`](src/analysis/fixtures.ts) and cover a tenant
counterexample, a valid example, a mutation with effects, and terminal/error states (large graph,
exhausted, limit, unsupported, missing context, SQL error, replay mismatch, timeout, stale archive,
and transport gap). They intentionally include duplicate occurrences, typed NULLs, Unicode values
and malicious-looking label text so the UI's escaping and review behavior can be exercised.

The fixture transport is injected by tests. The published application does not ship the fixture
scenario library or substitute fixture output for a failed host call.

The original raster logo source is [`sqlolla_logo_assets.jpg`](sqlolla_logo_assets.jpg); the
transparent app mark is [`src/assets/sqlolla-mark.png`](src/assets/sqlolla-mark.png).
