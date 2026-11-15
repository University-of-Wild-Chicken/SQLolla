# SQLolla

SQLolla compiles an explicit PostgreSQL 16 profile through SQLeanParser into
typed, bag-preserving relational algebra and deterministic Alloy models. M1
provides an offline Rust CLI, a native Lean parser adapter and a narrow Java 17
Alloy worker. It can compare concrete executions or find bounded counterexamples
to read-result properties. It never executes submitted SQL against a live app.

The **MVP target is the full pinned SQLeanParser typed-AST surface**, including
business queries and mutations. M1 is its tested relational foundation, not that
complete MVP. [capabilities.json](capabilities.json) tracks the remaining families
and extension gates. Parser acceptance, static typing and runtime equivalence
are separate claims; the separate upstream full PostgreSQL grammar API does not
provide typed execution semantics.

The current parser census exports all 276 pinned industrial SQL records
(260 distinct texts) as structural ASTs. The adapter uses the pinned typed parser
and certified grammar CST lowering for its remaining syntax gaps. This is
syntax coverage, not complete MVP semantics. Run `python parser/census.py` to
track the same population. The [corpus ledger](validation/corpus-manifest.json)
separates syntax, authentic schema context, actual PostgreSQL outcomes and current
Rust/Alloy checks. Expected SQL errors count only when their semantics match;
unsupported or resource refusals remain uncovered.
All 276 records now match under both declared PostgreSQL fixtures (552 comparisons),
including expected SQL errors and independently observed effect obligations.
This completes the pinned corpus target; arbitrary inputs and the broader
SQLeanParser typed-AST surface remain separate MVP obligations.
Eight unchanged corpus queries also pass 24 PostgreSQL/Rust/Alloy comparisons
under explicitly synthetic catalogs. Their provenance and the remaining coverage
gaps are retained in [industrial.json](validation/results/industrial.json).

## Run locally

Install Git LFS before cloning, then run `git lfs pull`. The full canonical
`validation/corpus-manifest.json` is retained through LFS; validation needs its
materialized JSON rather than an LFS pointer. Source, tests, dependency pins and
smaller verification reports remain ordinary Git files. Generated runtimes and
build caches are excluded. Provision the pinned tools for a fresh installation;
the existing local workspace's extracted toolchains are not part of the repository.

The supplied Windows workspace has pinned portable Rust 1.100.0, Lean 4.34.0,
Java 17, Alloy 6.2 and PostgreSQL 16.16 runtimes. PowerShell 7 is required for the
parser's schema tests. No global installation or database service is needed.

```powershell
. .\scripts\env.ps1
.\scripts\build.ps1
Get-Content -Raw .\examples\tenant-counterexample.jsonl | .\target\debug\sqlolla.exe
.\scripts\verify.ps1
```

The example intentionally omits a tenant filter. Its decoded counterexample
contains a row belonging to another tenant. Add `WHERE tenant_id = $1` and
`"parameters":{"1":{"type":"int32","value":1}}` to check the corrected
query under the same finite domain. `NO_COUNTEREXAMPLE_WITHIN_BOUNDS` describes
only the reported profile, property, domain and capacities.

The CLI accepts one UTF-8 JSON object per line and returns one response per line.
`id` is echoed. `catalog` contains `search_path` and a `tables` map keyed by stable
IDs. Each table declares `schema`, `name` and ordered columns with `name`, `ty`
(`int32`, `bool`, `text`) and `nullable`. Cells use `{"type":"int32","value":1}`
or typed NULL such as `{"type":"null","value":"int32"}`. Parameters are
explicit numbered values; no SQL text is interpolated into a shell command.

| Mode | Required input | Result |
| --- | --- | --- |
| `compare` (default) | Complete concrete `database`, including empty unused tables | Rust and decoded Alloy result bags must agree |
| `compile` | Same concrete database | Canonical typed `ir` and Alloy `model` |
| `analyze` | `property`, per-scanned-table `bounds`, finite typed `domain` | Replayed counterexample or bounded no-counterexample result |

Read-result properties are `{"kind":"only_value","column":0,"value":...}`,
`no_value` with the same fields, and `{"kind":"max_rows","max":2}`. Value
properties use exact typed value equality, including NULL; they are policy
checks, not SQL `=` expressions. `include_model:true` exports artifacts in other
modes. Unknown fields, wrong-mode inputs, unsupported operations, incomplete
data, resource limits and engine failures produce errors, never safety claims.

Analysis generates a named query predicate and a separate `ResultProperty`
predicate. Alloy executes `check QueryProperty` on the assertion that the query
semantics imply that property. A separate `run` first establishes satisfiable
assumptions, preventing an inconsistent model from being reported as safe.
The Java bridge confirms the command is an actual assertion check; every SAT
counterexample is decoded and replayed. `python validation/assertion_examples.py`
regenerates the inspectable `.als` models and SQL/IR evidence under `examples`.

The required final frontend is specified in
[the maintained gameplan](contracts/m2/gameplan.json) under `instance_review_frontend`.
It pairs Graphviz instance cards with AI summaries grounded in the SQL/IR/Alloy
triple. Right swipe accepts a use case; left swipe labels unwanted behavior for
the next agentic revision. Stored instance JSON, on-demand enumeration and
revision-bound feedback keep solver evidence separate from user intent and AI
text. The [frontend package](frontend/README.md) retains an explicit offline fixture
demo and connects through the loopback Rust `sqlolla-host` to the same Pipeline.
Build its assets with `npm ci` and `npm run build` inside `frontend`, then start
`target/debug/sqlolla-host.exe` from the initialized Windows environment above.
Open `http://127.0.0.1:8787`; use the Analyze tab for a new stored solver instance.
Optional summaries and SQL proposals use exact `gpt-6.1-sol`; keys stay in tab
memory and request-lifetime host state. The host reloads canonical evidence before
provider egress and deterministically reanalyzes proposed SQL without changing
the retained property, context or finite domain. Other provider adapters remain
future work under the current model policy.
The redacted [browser/AI verification report](validation/results/host-browser.json)
records the tested host and frontend build, actual instance navigation, numeric
precision, live cited explanation and deterministically reanalyzed SQL proposal.
The [frontend-only agent prompt](contracts/frontend-agent.txt) defines its iteration
scope, fixture transport, evidence handling, Graphviz review and acceptance tests.
The offline [instance API](crates/sqlolla/src/instances/README.md) stores
versioned JSON bundles and supports `instances.start`, `instances.next` and
`instances.open`, with replay before exposing examples or counterexamples.

The extended `sqlolla.explicit_outcomes.v2` profile adds complete finite outcome
sets, including order ties and pagination alternatives. Its Alloy model selects
from a Rust-evaluated transition table and labels its validation
`ALLOY_TABLE_ROUNDTRIP`. This checks the encoding and solver selection; independent
SQL semantics come from PostgreSQL differential tests. The native M1 profile
retains its independent RA encoding. Extended requests may supply an `execution`
context with explicit session values and complete table semantics for writes.
CHECK and default expressions are reparsed from SQL; supplied ASTs are never trusted.

Scalar extensions include checked integer/decimal arithmetic, SQL errors, casts,
conditional expressions, supported dates/intervals, arrays and JSONB. Each family
still has explicit admission limits; the capability ledger and PostgreSQL tests
define the supported fragments. Catalog/session values are explicit inputs.
Duplicate rows and output column positions are preserved.

## Validate and extend

`scripts/verify.ps1` builds all components; runs Rust tests, formatting, Clippy,
Rustdoc and the 30-line method gate; audits Lean declarations; and executes the
native parser, industrial census/replay, actual PostgreSQL differential, symbolic
and mutation suites.
It stops on a failed command. The PostgreSQL harness creates a private ephemeral
loopback cluster and removes it after stopping it. Canonical evidence lives in
[validation/results](validation/results); corpus generators remain in
[validation](validation). `python validation/run.py --reuse-oracle` reuses only
hash-matching, previously executed PostgreSQL results.

The industrial expansion uses `validation/corpus-manifest.json` as its single
stage ledger. `python validation/corpus_runtime.py` observes the initial schemas;
`python validation/corpus_runtime.py --fixture nonempty` adds populated authentic
schemas. `python validation/corpus_wire.py` inspects their real types, defaults and
constraints. `python validation/corpus_semantics.py` checks every declared fixture
against the current CLI and exits unsuccessfully until all 276 records match.
The original 24 synthetic industrial comparisons remain a separate regression.

For each extension, use the small vertical sequence in `capabilities.json`:
preserve syntax fields → bind exact types/effects → interpret → encode complete
outcomes → decode/replay → compare with PostgreSQL → adversarially review.
Update the same ledger and maintained docs, then remove temporary experiments
and redundant artifacts. Do not add a new report directory for every pass.

Rustdoc is generated by `cargo doc --workspace --no-deps`; Java API documentation
by `javac`/`javadoc` using the pinned Alloy classpath. Authored source functions
and methods have purpose comments and at most 30 physical lines after formatting.
The core is pure; process I/O is confined to the orchestration and bridge modules.

The Lean library has no placeholders or custom axioms. The compiled trust gate
permits only Lean's `propext`, `Classical.choice` and `Quot.sound`. Its current
theorems establish specification contracts, **not an end-to-end correctness
proof of the Rust compiler**. Differential and mutation tests are evidence, not
a universal equivalence theorem.

## Portability and release gates

Rust process paths use platform separators and explicit arguments. Set
`SQLolla_ROOT`, `SQLolla_PARSER`, `SQLolla_JAVA` and `SQLolla_ALLOY_JAR` to provisioned
native tools in a private installation. `SQLolla_PG_BIN` selects a PostgreSQL 16
test installation. Lean expects SQLeanParser at `../dependencies/SQLeanParser`.
The build wrappers check the pinned parser revision and Alloy artifact hash.

Windows x64 has been executed. Linux, macOS and WSL validation and clean-machine
packaging remain release gates; copying the Windows tools is not a portable
installation. On Unix, provision native Rust, Lean 4.34.0 and Java 17, set
`LEAN_BIN` and `SQLolla_ALLOY_JAR`, and use `sh scripts/build.sh`. This wrapper is
provided for the platform-validation increment and has not been executed here.

The local graph/table review UI and optional OpenAI assistance are implemented.
The full feedback-preserving revision agent, additional provider adapters,
multi-statement transaction schedules and deployment at `sqlolla.us` remain planned.
Current extensions observe individual mutations, SQL failures and fixed-snapshot
row locks; these do not establish correctness under concurrent database changes.
The CLI emits exact instance data; the browser provides its review interface.
See [ARCHITECTURE.md](ARCHITECTURE.md) for those boundaries and the private
deployment design. SQLeanParser redistribution licensing, dependency notices,
hosted worker isolation and platform validation still block a public release.
