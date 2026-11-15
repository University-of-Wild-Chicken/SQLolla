# SQLolla architecture

M0 established component boundaries and Lean contracts. M1 implements the offline
relational core. The maintained delivery surface is [capabilities.json](capabilities.json),
with operation and validation instructions in [README.md](README.md). Historical
M0 decisions remain in [m0/architecture.json](m0/architecture.json) and the original
planning snapshot in [SQLolla.gameplan.json](SQLolla.gameplan.json).

The MVP target remains the pinned SQLeanParser typed-AST surface, including
mutations and business-query features. M1 is its foundation, not the complete MVP.
Current unsupported features fail closed and stay in the delivery ledger.

## Components and authority

```text
SQL / low-code graph
    -> compiled SQLeanParser adapter (Lean dependency)
    -> Rust binder + capability gate
    -> typed bag RA + effect/transaction IR
    -> Rust deterministic Alloy encoder
    -> Java 17 / Alloy 6.2 / SAT4J worker
    -> Rust decoded-instance validation
    -> deterministic instance tables / graph / transaction trace
    -> optional BYOT AI explanation linked to immutable evidence
```

Rust owns algorithms, semantics, authorization, orchestration and result status.
Java is limited to Alloy API loading, solving, enumeration and tuple extraction.
Lean remains the pinned parser dependency plus the separate specification. The
planned frontend is a TypeScript low-code client; it does not duplicate compiler logic.

All three native tool families now execute on Windows x64 through isolated,
checksum-verified tools. Those tools were not found as usable global installs.
WSL has no registered distribution in this user context; Linux and macOS were
not executed. Portable-tool smoke tests are not a clean-machine release test.

## Formal foundation

[formal/SQLolla.lean](formal/SQLolla.lean) imports the M0 contract library:

| Module | Established specification boundary |
| --- | --- |
| Truth | SQL TRUE/FALSE/UNKNOWN; filtering differs from CHECK acceptance |
| Relation | Typed rows, occurrence-preserving bags, projection/product laws |
| IR | Typed relational and mutation shapes; not a full interpreter |
| Transaction | Failure stops later writes; rollback, frames and explicit outcomes |
| Analysis | Nonvacuity, bounded claims, unknown and incomplete-coverage handling |
| Encoding | Separate conditional SAT soundness and UNSAT completeness obligations |
| Examples | Synthetic tenant, simultaneous-assignment and rollback witnesses |
| Instance | Decoded-fact authenticity, counterexample labels and AI references |

The proof library uses mathematical integers and an abstract execution relation.
It does not establish PostgreSQL execution equivalence, a working SQL compiler,
numeric bitwidth correctness, or correctness of AI prose. In particular, the
empty-encoding example proves that witness replay alone cannot justify UNSAT.
Every future admitted operation needs independent all-outcome validation.

The maintained [M2 contracts](contracts/m2/SQLollaM2.lean) extend this foundation
with SQL errors, checked arithmetic, rich values, mutations, transactions,
subqueries, ordering and complete finite transition selection. The same compiled
trust audit applies. These remain abstract contracts and conditional lemmas,
not a preservation theorem for the Rust implementation.

`formal/verify.ps1` rejects prohibited source shortcuts, checks that every formal
source module is imported, builds the library, and audits compiled declarations
by owning module. The only allowed transitive axioms are Lean's `propext`,
`Classical.choice`, and `Quot.sound`. No custom axiom or unsafe declaration is
allowed. Generated theorem counts are not counts of independent semantic claims.
This gate trusts the pinned Lean toolchain; it is not a malicious-code sandbox.

## Semantics and safety claims

M1 models pure reads over int32, Boolean and exact text values, with bag algebra,
SQL NULL/3VL, derived sources and nonrecursive CTEs. Concrete mode compares Rust
and decoded Alloy bags. Symbolic mode runs the named semantics predicate to check
reachable assumptions, then executes `check QueryProperty` on a separate assertion.
The assertion is `QuerySemantics implies ResultProperty`; the policy is never
assumed as a fact. The Java bridge reports and Rust validates command kind and
assertion identity. SAT check results are decoded and replayed counterexamples.
Catalog, parameters, inputs and artifacts
have separate hashes. The CLI does not execute SQL against live databases.

The extended profile enumerates complete Rust statement outcomes over a concrete
input or an explicitly bounded finite input domain. Its deterministic Alloy
model supplies an `ExecutionTable` predicate and checks whether it implies the
requested result property. Responses
identify this as `RUST_EVALUATOR_BACKED_FINITE_TABLE`, with
`ALLOY_TABLE_ROUNDTRIP` validation, because this encoding shares the evaluator's
semantic assumptions. PostgreSQL provides the independent comparison. Retained
native M1 operators still use the independent relational encoding. An enumeration
that exceeds capacity returns an error and cannot yield a bounded safety claim.

The planned first transaction profile is sequential with no concurrent
transactions. Statements have local parameter scopes and explicit catalog,
session and actor context. Transaction boundaries are structured input because
the typed parser does not accept BEGIN/COMMIT. Arbitrary SQL is not split on
semicolons. Client branches, savepoints, concurrent schedules, triggers, RLS and
other unmodeled effects receive explicit coverage diagnostics.

Counterexample search requires satisfiable assumptions, reachable executions,
adequate scopes and a completed failure query. Enforced constraints produce SQL
errors and aborts; they must not remove all failing executions from the model.
The business property under test is never assumed as a poststate fact.

The numeric decision for future arithmetic is a finite-input profile with adequate intermediate range
and explicit SQL range-error rules. The M0 Alloy probe demonstrated both wrapping
and exclusion of overflowing instances. Setting `noOverflow=true` alone is not
a correct SQL arithmetic model. M1 represents int32 values as exact scalar atoms
with a finite comparison relation. Alloy integers only count occurrences, with
bitwidth covering complete capacity. SQL arithmetic remains explicitly refused.

Output occurrences use structural capacities: projection retains input slots,
products/joins account for candidate pairs, UNION ALL tags both input domains,
and outer joins reserve unmatched rows. M1 implements these capacities, capped
at 256 total slots, 256 encoder nodes, 16,384 cells and a two-MiB model. Binder
budgets bound expanded plans and repeated inference, including CTE expansion.
PostgreSQL bag comparisons, independent finite symbolic searches and semantic
mutation tests exercise this encoding. A universal preservation proof remains
open. Excessive scope is a resource limit, not evidence of safety.

Compilation and canonical artifacts are deterministic. Solver-selected witnesses
may differ between versions and platforms. Reverse translation applies only to
recognized SQLolla-generated packages with preserved IR/provenance.

## Instance exploration and explanations

The requested reference is [AlloyStudio](https://github.com/AnonymousAlloyLab/AlloyStudio).
Its solver-derived visualization and separate explanation layers inform this
design; see [the pinned review](m0/instance-review.json). SQLolla keeps its own
BYOT security design and does not copy a provider key configuration into the UI.

Users choose either **Explore a satisfying example** or **Find a counterexample
to this rule**. The instance viewer has exact tables, a deterministic relation
graph, before/after rows, a statement-step selector, and an explanation panel.
An example is not a proof of security. A counterexample label identifies a
specific violated property; unvalidated decoded models remain candidates.

The canonical instance carries its analysis/instance IDs, model/IR/catalog/policy
hashes, bounds, typed atoms, ordered n-ary tuples, duplicate occurrence identities,
NULL values, states, changes, termination and evidence fact IDs. Higher-arity
relations use tuple nodes or exact tables rather than misleading binary edges.
Filtering a view shows hidden counts and never alters the underlying instance.
Full-evidence completeness, transport completeness and view completeness are
separate fields. Missing data in a filtered/truncated view does not establish
absence from the instance. A retained complete witness is required for a claim
that the exported evidence can be replayed.

AI explains a selected instance and may propose a typed query patch. It does not
create the graph, overwrite evidence, change bounds or weaken policies. Every
reference must resolve to the exact instance hash. That check establishes the
existence of cited facts, not the truth of arbitrary natural-language assertions.
Provider failures leave the instance and deterministic summary available.

Tokens stay in tab memory and request-lifetime host state, never graph data, URLs,
localStorage, logs, checkpoints or exports. The current host fixes the destination
to OpenAI Responses and every production flow to exact `gpt-6.1-sol`; it provides
no endpoint editing or provider/model fallback. Sensitive context is described
before an explicit send. Redaction omits the complete SQL/IR/Alloy triple and
scalar payloads; proposals require explicit unredacted context. Anthropic and
Gemini adapters remain future compatibility work under this model policy.

## Interfaces, deployment and maintenance

M1 uses versioned, size-bounded UTF-8 JSON over private pipes, with request IDs,
pinned parser identity and explicit failure statuses. SQL and tokens
must not be command-line arguments; the M0 parser's command-line specimen is not
the production transport. Explicit argument APIs avoid shell interpolation.

`sqlolla-host` serves the built frontend and HTTP routes on IPv4 loopback. Exact
Host/Origin checks and a memory-only CSRF capability protect all POST actions.
One Pipeline thread owns the parser and Alloy worker; a bounded command queue
and API admission limit keep resource failures distinct from solver exhaustion.
Incoming command bytes preserve integer precision. Browser cancellation discards
stale responses; active computations retain their existing internal budgets.

The host reloads stored, replayed instances before an AI call. Strict provider
output schemas, model/revision/context identity and fact-reference checks cannot
prove prose accuracy. A SQL proposal changes only SQL in the retained request
and undergoes a new bounded analysis. It does not yet certify all user feedback
as preservation/exclusion obligations. Canonical evidence, browser feedback and
AI annotations remain separate. Loopback hosting is not authenticated public
multi-user deployment.

The proposed public deployment is static UI at `sqlolla.us`, authenticated ingress
at `api.sqlolla.us`, and a Rust/Lean/JVM origin behind Cloudflare. Ordinary Workers
are not the JVM/SAT host. Private installations use the same core, loopback defaults,
optional provider egress and no required Cloudflare connectivity. No DNS, account
configuration, hosting or deployment has been changed.

Authored methods stay within 30 lines and document purpose and effects. The Lean
spec and disposable probes follow that rule. Production work additionally requires
Rustdoc/Javadoc/TSDoc gates, pinned dependencies, capability-ledger updates, and
independent review of every semantic extension.

## Open gates

| Gate | Blocks | Required resolution |
| --- | --- | --- |
| Parser licensing and full SBOM | Redistribution | Establish explicit permission and notices |
| Linux/macOS/WSL validation | Platform support claim | Run native build, solver, IPC and cancellation matrix |
| Broad typed-AST coverage | Complete MVP | Extend M1 using the capability ledger and authentic corpus census |
| Preservation and future error/effect semantics | Stronger safety claims | Independent semantic checks and formal preservation arguments |
| PostgreSQL transaction oracle | Transaction compatibility | Extend tested statement errors/writes/effects to transaction commit and abort |
| Worker descendants, quotas and malformed IPC | Hosted untrusted jobs | Enforce resource/process isolation per OS |
| Feedback-preserving agent and public BYOT isolation | Product readiness | Replay every labelled case; validate authenticated deployment and provider boundaries |
| Clean-machine release reproducibility | Distribution | Cargo.lock is retained; run native CI and package pinned runtimes |

These gates do not invalidate the component split. They prevent a feasibility
probe or conditional theorem from being presented as a production guarantee.
