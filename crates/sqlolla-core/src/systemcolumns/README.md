# Heap-header observation profile

`TupleContext` describes an exclusive, top-level writer with no savepoints,
concurrent actors, MultiXact state, vacuum rewrite, triggers, or row-security
semantics. Its actual write xid and occurrence-aligned headers must be supplied
before the analyzed statement. A nonzero raw `xmax` alone does not establish its
transaction/lock interpretation.

The current executable adapter supports INSERT and ON CONFLICT UPDATE. Explicit
`RETURNING xmin` and `RETURNING xmax` use nominal `xid` values; wildcards retain
only user columns. Conflict handling first locks the old version, including when
UPDATE WHERE is false. A successful update preserves that local lock in its new
header. This is a version transition, not an `upsert = true` marker. Ordinary
UPDATE/DELETE with a tuple context and nested system-column RETURNING are refused
until their adapters preserve the same obligations.

Each `TupleVersionEvent` contains a pure header transition and original/before/
after row witnesses. Occurrences name stable candidate slots before table bags
are sorted; duplicates retain distinct slots. Newly appended slots follow input
slots. Journals admit at most 4096 events and 8 MiB summed serialized event bytes
per branch, including inherited witnesses. Crossing either bound refuses the
whole analysis. Header journals are replayed before another step is emitted.

On SQL failure, user-row changes are rolled back. The abort projection removes
newly inserted visible versions and retains raw-header residue on original rows.
The reported `aborted_residual` is the physical observation after abort; it is not
a surviving transactional row lock. Existing generic statement/savepoint lock
semantics must not be used to infer a write subtransaction xid.

`TupleVersion.lean` and `XidScalars.lean` specify restricted transition and scalar
invariants. They are not a proof of PostgreSQL MVCC or Rust refinement. Maintained
PostgreSQL tests are `validation/system_columns.py`,
`validation/system_columns_river.py`, and `validation/xid_scalars.py`.
Their reports distinguish observed fixture coverage from general guarantees.
