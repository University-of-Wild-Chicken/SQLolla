# Deterministic semantic core

This crate binds pinned SQLeanParser envelopes into typed plans and evaluates
finite outcomes without database I/O, solver calls, SQL text parsing, or AI.
`bind`/`eval` retain the original relational API. Extended callers use
`bind_statement_with_source`, `merged_catalog_context`, initial-state validation,
and `evaluate_with_catalog`; stored views require parser preparation by the
outer pipeline. No caller-supplied bound expression is a substitute for parsing.

`binding/` owns lexical scope, type resolution, AST admission, and expansion
budgets. `planning/` folds admitted immutable constants and preserves known
failure phases. `scalar/` owns PostgreSQL scalar families and their explicit
profiles. `execution/` lifts relational alternatives into statement outcomes;
`context.rs`, `metadata.rs`, and `constraints.rs` keep initial-state assumptions,
write constraints, defaults, and SQL errors separate.

`cte.rs` and `execution/with_statement.rs` preserve original base snapshots.
Writable CTEs communicate changed rows only through their RETURNING relations;
their disjoint table deltas publish atomically. Unreferenced writes execute,
while unreferenced reads are removed after type binding. Compiler-reserved CTE
relation identifiers never become external input requirements or published
tables. Overlapping writes, cross-written foreign-key trigger dependencies,
uncertain read-CTE error demand, and partial consumption of locking CTEs remain
explicit coverage refusals.

`binding/initialization.rs` retains successfully resolved ROW SHARE relation
identities before unused CTE removal and scalar planning. A top-level
`StatementPlan::Initialized` acquires that fixed-snapshot footprint once, even
when no tuples are requested. `execution/initialization.rs` checks unresolved
relation conflicts before a retained planning error; SQL failure releases new
transactional locks. It does not invent row locks. Failed-binding opening order,
DDL initialization, AccessShare/RowExclusive observations, and concurrent wait
completion remain outside this profile. The maintained
`validation/lock_initialization.py --engine` compares every admitted outcome
with PostgreSQL and records unsupported demand shapes separately.

Conflict handling checks proposed rows before skipping unique conflicts. It
retains EXCLUDED scope, skipped-row counts, and same-command cardinality errors.
Unordered multirow conflict inputs require source-order alternatives before
admission. Joined mutations currently require matching source scopes to be
identical; distinct possible representatives remain an explicit coverage gap.
Explicit DEFAULT positions remain distinct from SQL NULL and ordinary supplied
values. Sequence allocations require their own declared state/effect model.

Add semantics only after extending and checking the corresponding Lean contract
in `contracts/m2`. Then add Rust boundary tests and actual PostgreSQL differential
observations; use `validation/cte.py` for CTE, conflict, default, and joined-write
changes. The Lean contracts do not establish PostgreSQL refinement by this Rust
implementation. Complete outcomes mean complete within an admitted finite
profile; resource exhaustion must never return a truncated success set.
