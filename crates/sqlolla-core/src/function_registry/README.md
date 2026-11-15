Function resolution and metadata purity
======================================

`builtin` combines scalar NULL/body capability with volatility and effect family.
`immutable_call` also checks signature-sensitive temporal behavior. A known result
type alone does not make a function executable. Planner folding, index predicates
and expanded immutable SQL bodies use the same classification.

The SQL-function resolver inserts implicit `pg_catalog` only when it is absent from
the supplied path, hides identical signatures by schema order, and selects exact
types before allowed implicit conversions. Unknown literals remain unresolved
until a candidate is selected. Multiple coercible candidates, unsupported builtin
collision signatures, non-inert arguments and recursive definitions fail closed.
This is not the complete PostgreSQL overload-ranking algorithm.

The sorted bootstrap-name inventory is extracted from PostgreSQL 16.16's official
`pg_proc.dat`; its source URL and SHA-256 are in `pg16_builtin_names.json`. It stops
an unimplemented system builtin from being mistaken for an absent function. It
does not describe extensions, later system-catalog mutation, or user definitions
omitted from the supplied closed declaration context. Refresh the pinned inventory
and differential regressions together when changing the PostgreSQL profile.

The source specification is `contracts/m2/SQLollaM2/FunctionResolution.lean`.
Its checked abstract contracts do not prove Rust/PostgreSQL equivalence. Maintained
PostgreSQL regressions are `validation/sql_functions.py` and
`validation/unique_indexes.py`. Index traversal order comes from the same database
snapshot, not names or constraint categories. Missing order cannot justify choosing
between competing failure diagnostics.
