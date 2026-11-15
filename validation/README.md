# PostgreSQL differential validation

This suite runs real PostgreSQL16, the Rust interpreter and the Java17/Alloy SAT bridge. It never connects to a business database. The PostgreSQL cluster uses a random password, a free IPv4 loopback port, UTF-8 and C collation. Every run stops its cluster and removes its owned data directory.

From the repository root, after building the parser, CLI and Java bridge:

```powershell
python validation/bootstrap_postgres.py
python validation/run.py
python validation/rejections.py
python validation/pg_rejections.py
python validation/symbolic.py
python validation/mutations.py
python validation/industrial.py
python validation/comparison_tests.py
python validation/ordering.py
python validation/extended_symbolic.py
python validation/grouping.py
```

`bootstrap_postgres.py` pins PostgreSQL16.16 for Windows x64 from EDB, linked by the official PostgreSQL Windows download page. It verifies the published SHA256 and extracts only an isolated runtime under `tools/.tools/`. It does not run an installer or create a Windows service. On Linux/macOS, supply an existing PostgreSQL16 binary directory through `SQLolla_PG_BIN`; the harness checks the actual major version. Those platforms have not been executed in this validation session.

The native M1 suite contains 3,511 cases. Its exhaustive portion covers every unordered bag with zero, one or two rows over three-value nullable integer, Boolean and text domains. Every pair is exercised through six set operators, four equijoins and a Cartesian product. Additional cases cover three-valued logic, integer bounds, complete-row duplicate equivalence, unknown literal coercion, aliases, derived relations and nonrecursive CTEs. This is exhaustive over the stated small domains and operations, not over PostgreSQL or arbitrary query trees.

The oracle creates independent typed tables and obtains positional JSON row arrays from PostgreSQL. Python compares exact typed row multiplicities with both engine outputs; it does not implement RA to invent expected answers. `run.py --reuse-oracle` reuses an executed oracle only when both the complete fixture digest and output digest match. `--oracle-only` is explicitly reported as incomplete validation.

`symbolic.py` compares 110 bounded property searches with complete PostgreSQL enumeration of ten input bags for each of eleven query/catalog configurations. One configuration includes an unused catalog table with bound zero and requires that table to appear as an empty bag in a complete witness. Every returned counterexample is checked against its declared domain/bounds and replayed in PostgreSQL. `mutations.py` requires an original model to rule out a wrong result and a deliberately changed model to expose it. It also rejects a contradictory input-capacity model as an encoding failure. Neither an inconsistent model nor a timeout counts as a clean query.

`industrial.py` preserves the eight-query M1 regression: seven BenchBase queries and one Temporal query. Each runs against three synthetic fixtures (empty input, matching duplicates plus a miss, and a typed NULL parameter), giving 24 PostgreSQL/Rust/Alloy bag comparisons. SQL bytes remain unchanged from the corpus; earlier JDBC extraction conversions remain visible in provenance. PostgreSQL binds numbered parameters with typed `PREPARE`/`EXECUTE`. `industrial-contexts.json` declares synthetic columns, nullability and rows, not authoritative upstream schemas or production constraints. This regression supplements the full 276-record ledger below; its historical coverage inventory is not the current MVP census.

Keep `results/summary.json`, the hashed PostgreSQL oracle/metadata, rejection results, symbolic witness evidence, mutation summary and `results/industrial.json` as the canonical evidence. Bulk CLI request/response files and generated mutant models are reproducible diagnostics and may be removed after a successful run. Failures preserve their case IDs and observed outputs for reproduction.

The harness uses process argument arrays and explicit UTF-8. Windows startup deliberately avoids captured `pg_ctl` output pipes, which otherwise remain inherited by the server. Windows children are hidden; Unix child sessions have a process-group cancellation path. Cross-platform code paths require execution on their target runners before support is claimed.

## Full industrial coverage ledger

`corpus-manifest.json` tracks all 276 source records separately from the eight-query M1 replay. Maintain it with `python validation/corpus.py` (unchanged grammar/analysis probes), `python validation/corpus_context.py` (explicit network acquisition of pinned DDL and driver context), `python validation/corpus_schema.py` (authoritative schemas and inferred parameter types), and `python validation/corpus_runtime.py` (actual PostgreSQL execution). Context acquisition verifies raw bytes against the pinned Git tree blob ID and records SHA256. Downloaded Java/Go application code is never executed.

The copied context files retain their upstream terms independently of this project's root license. Exact pinned upstream license files accompany each subtree:

- BenchBase `33c00473807ebd49304d114a6d769d2d2b2bbb34`: [Apache 2.0 notice](upstream/benchbase/LICENSE), [pinned source](https://raw.githubusercontent.com/cmu-db/benchbase/33c00473807ebd49304d114a6d769d2d2b2bbb34/LICENSE).
- River `1354a059d7cbe5790acc7a755a2ab7ac6d2f1ed8`: [MPL 2.0](upstream/river/LICENSE), [pinned source](https://raw.githubusercontent.com/riverqueue/river/1354a059d7cbe5790acc7a755a2ab7ac6d2f1ed8/LICENSE).
- Temporal `b7bc978d1040afe2dcbc68ecfe8b228ae198e0b6`: [MIT and copyright notices](upstream/temporal/LICENSE), [pinned source](https://raw.githubusercontent.com/temporalio/temporal/b7bc978d1040afe2dcbc68ecfe8b228ae198e0b6/LICENSE).

The first full execution fixture uses authoritative schemas with their upstream seed rows, synthetic nonnull scalar parameters, and empty array parameters. It records 269 successful executions and seven actual constraint errors. Five driver-context cases are explicit: TPC-H Q1 binds a text parameter; Q15 instantiates the same date twice when creating its view, then makes that view available to SELECT/DROP; River expands its schema marker inside a sequence-name string. Original and instantiated SQL hashes remain distinct. These observations do not certify Rust/Alloy coverage, nonempty business behavior, notification delivery or backend termination.

Runtime evidence contains before/after/rollback state hashes. Resolve `observation_states` and then its relation hashes through `observation_relations`; these are deduplicated complete public relation observations, not summaries of selected rows. Results preserve PostgreSQL type names, SQLSTATE, constraint names, returned rows, affected counts, sequence state and advisory transaction locks. PostgreSQL sequence consumption can survive a failed statement and transaction rollback. Complete semantic coverage requires independent engine outcome membership checks and the declared side-effect obligations, not merely preparation, rejection, or one successful empty-input run.

The second fixture (`corpus_runtime.py --fixture nonempty`) seeds all 74 physical tables using authentic DDL, FK dependency order, checked types, NOT NULL/check constraints and source literal defaults. Row values are synthetic; array parameters remain empty except explicitly declared positive inputs, including repeated notification payloads. `corpus_wire.py` reconstructs typed engine catalogs and actual unique-index enforcement ordering. `corpus_semantics.py` requires every declared fixture to match before crediting a source record; unobserved effect families remain blockers. It verifies pooled hashes and complete row widths, derives outer query ordering from SQL source, and compares arbitrary-precision numeric values without rounding through Python's Decimal context. Replacing an oracle or semantic context invalidates its old comparisons; unreachable pooled states are pruned.

`corpus_effects.py` compares each independently observable family separately: held row/table locks, sequence poststate, advisory ownership, and notification commit delivery. Notification companions run identical SQL and parameters in an isolated dump clone, require equivalent relevant pre/poststate and context, and observe actual LISTEN delivery after commit; the original transaction separately proves rollback delivers nothing. Delivered notifications do not independently expose every internal queue invocation. Sequence fixtures require exclusive cache-one state; CTE jobs whose tuple interleavings are not represented are refused.

Focused regression entry points cover `sql_functions.py`, `unique_indexes.py`, `collection_scalars.py`, `comparability.py`, `regex_subset.py`, `session_effects.py`, `sequence_effects.py`, `xid_scalars.py`, `backend_effects.py`, `system_columns.py`, `system_columns_river.py`, `cte_locks.py` and `lock_initialization.py --engine`. They retain actual PostgreSQL outcomes and distinguish supported results from deliberately tested refusals. The River tuple suite uses unchanged industrial INSERT SQL with nonempty arrays, conflicts, repeated proposals and rollback header observations. Backend tests observe signal requests separately from the owned worker's eventual exit. `scripts/verify.ps1` includes these checks and refreshes the full corpus's contexts, both PostgreSQL fixtures and semantic comparisons. Read each maintained report's binary hash and scope before combining its evidence with another run.

Extended Alloy models select from a complete table produced by the Rust evaluator. Their table roundtrip is not an independent SQL semantics check. `extended_symbolic.py` supplies that independent check over 42 concrete int32 bags and all 53 physical permutations, requiring equality of the full observed outcome sets before validating five SAT property verdicts and bounded witnesses. `ordering.py` separately checks 15 queries over 90 PostgreSQL permutations. `grouping.py` checks 29 aggregate and GROUP BY boundaries, including actual SQL errors and observed result typmods. `comparison_tests.py` deliberately mutates ordering, large decimals, state, effects, response IDs, input shapes and witness bounds to check that validators reject false evidence.

`NoValue` and `OnlyValue` policies use exact typed transport identity, including typed NULL and observable scalar spelling. They do not apply SQL implicit coercions or SQL duplicate equality. SQL error planning/statement phase is not inferred from SQLSTATE in the PostgreSQL harness; reports identify this observation limit. Industrial fixture membership remains evidence for those concrete fixtures, not a universal proof for every possible input or PostgreSQL execution plan.
