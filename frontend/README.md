# SQLolla evidence review frontend

A local SQLolla review workspace: **Build → Check → Review examples → Refine**.
The explicit fixture mode works offline. The Rust `sqlolla-host` connects the browser to the
existing deterministic Pipeline and optional OpenAI assistance using exact `gpt-6.1-sol`.
Submitted SQL is modeled; it is never executed against an application database.

The interface is a three-column workspace:

- **Cases & query studio** — scenario library and a typed relation builder. Drafts are local,
  unvalidated state, separate from canonical IR. Editing a draft sets stored evidence aside.
- **Evidence card** — Graphviz/table views, exact before/after/output occurrences, typed NULLs,
  duplicates, statement errors/effects, graph node fact inspection and explicit Next.
- **Explain & refine** — labelled fixture summaries or requested live summaries with fact citations,
  assumptions, bounded reanalysis of SQL proposals, memory-only BYOT and the review ledger.

## Run

Requires Node.js 22.12+ and npm.

```sh
npm ci
npm run dev
```

Build static assets and inspect the offline fixture demo:

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
installation root. The process binds IPv4 loopback only. Vite preview remains a fixture demo;
it does not proxy provider credentials or the canonical analysis API.

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
It verifies no external requests, Graphviz SVG rendering, keyboard accept/reject/undo, graph-drag
non-swipe, explicit Next, draft invalidation, the compiler gap, large-graph partial mode, reduced
motion, WCAG A/AA axe checks, 390×844 layout and screenshots in [`e2e/screenshots/`](e2e/screenshots/).
Set `SQLLOLLA_URL` to check an already-running deployment instead.

`test:host` checks browser start/open/next, Graphviz and lossless Int64/NULL rendering against the
running loopback host. It creates synthetic stored instances and makes no AI request. The same
[`e2e/host.mjs`](e2e/host.mjs) exports `verifyHost({url, chromePath, apiKey})` for an explicitly
authorized live provider check with a credential supplied only in memory. It checks a cited summary,
SQL proposal reanalysis, explicit draft adoption, browser storage and Disconnect; its returned
report contains no key or generated prose. Do not embed credentials in commands or test files.

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

## Optional AI

Save an OpenAI key in the tab's provider settings, then explicitly request an explanation or SQL
proposal. Saving settings sends nothing. The same-origin Rust host reloads the selected stored
instance; the browser cannot replace its facts, model revision or analysis basis. The host calls
only OpenAI Responses with `gpt-6.1-sol`, `store:false`, no tools, redirects or model fallback.
Anthropic/Gemini adapters remain unconnected under this production model policy.

Redacted explanations omit SQL, IR, Alloy, parameters/domain values and fact payloads. Schema
shape and fact identifiers remain visible. Unredacted context includes the retained triple;
fact truncation is explicit. SQL proposals require unredacted context and preserve the original
catalog, parameters, property, bounds and domain during deterministic reanalysis. A candidate
result is not proof that accepted/rejected business cases have all been preserved.

Explanation references and response identity are checked. This proves neither prose accuracy nor
business intent. Provider failures leave deterministic evidence and feedback usable.

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

The original raster logo source is [`sqlolla_logo_assets.jpg`](sqlolla_logo_assets.jpg); the
transparent app mark is [`src/assets/sqlolla-mark.png`](src/assets/sqlolla-mark.png).
