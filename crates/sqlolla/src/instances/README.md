# Durable solver instances

The offline CLI accepts three additional JSON-lines actions. No SQL is executed
against a live server, and no AI or network service participates in solving.

```json
{"action":"instances.start","request":{"sql":"SELECT 1","catalog":{"tables":{},"search_path":["public"]}},"kind":"example","max_instances":16}
{"action":"instances.next","cursor":"<opaque cursor returned by start or next>"}
{"action":"instances.open","bundle_id":"<immutable bundle identity>"}
```

`request` is the ordinary strict SQLolla request, including explicit catalog,
typed parameters, database or finite bounds/domain, and execution context.
`kind: "counterexample"` requires `mode: "analyze"` and an explicit property.
Examples execute the generated `run QuerySemantics`; counterexamples execute
`check QueryProperty`. A separate satisfiable assumptions run precedes either.
Every yielded instance is decoded and replayed through Rust. Counterexamples
must also violate the archived property when replayed.

Responses distinguish `instance`, `exhausted`, and `limit`. Only a real UNSAT
solver step sets `enumeration_exhausted: true`. Limits, damaged archives, changed
compiler provenance, unavailable tools, timeouts, and failed replay never imply
exhaustion or safety. Errors return `status: "error"` with a stable diagnostic
code. A limit of 1 through 256 is explicit; the default is 64.

The archive defaults to `artifacts/instances` beneath `SQLolla_ROOT`.
Administrators can explicitly choose a private directory with
`SQLolla_INSTANCE_STORE`. This directory contains the supplied SQL and data;
apply deployment filesystem access controls accordingly. Cursors are validated
content identities, never paths. A standard exclusive file lock serializes
writers. Complete JSON entries are flushed and published by same-directory
rename. Unix also flushes the parent directory. Abrupt power loss and particular
filesystem implementations remain operating-system assumptions; this is not a
replicated durable storage service. Interrupted temporary files are ignored.

Bundles retain the exact SQL/typed IR/generated Alloy triple, bounds, property,
parameters, context, output schema, and hashes of the actual compiler, parser,
Alloy JAR and bridge. SHA256 provides integrity and revision binding, not an
authenticated signature or solver certificate. Caller correlation IDs are
excluded from semantic revision identity. The parser and compiler regenerate
artifacts on reopening; archived plans/models are never trusted as executable
input. A compiler or tool change requires starting a new bundle.

Each next/open action reconstructs the Alloy enumeration and checks its entire
saved prefix. Changed solver ordering or atom evidence fails with
`resume.prefix_changed`; the implementation never silently skips to a different
solution. This makes restart portable without serializing a JVM. It costs work
proportional to the retained prefix and applies a replay budget in addition to
worker timeouts. `open` returns only already committed records. Repeating a
cursor is idempotent when the regenerated prefix agrees. Concurrent commands
that cannot acquire the store lease return `store.busy` and can be retried.

The enumeration unit is an Alloy solution. Different atom assignments can have
the same SQL behavior; `semantic_sha256` is separate from
`alloy_relations_sha256`. Native RA encodes relational semantics independently
and compares decoded outputs with Rust. The extended profile encodes a complete
Rust-evaluated finite transition table and checks table selection with Alloy;
its label explicitly says `evaluator_backed_table_with_rust_replay`. Its Alloy
selection is not an independent implementation of PostgreSQL semantics.
Exhaustion concerns this exact generated finite model only. In the native
profile, unreferenced symbolic catalog tables are represented by an empty bag
in the displayed example and are not independently enumerated.

Typed instance JSON retains full ordered n-ary Alloy tuples, input/output and
poststate row occurrences, SQL errors, catalog transitions and retained effects.
Stable JSON Pointer facts ground Graphviz/AI references without confusing
duplicate row occurrences. The completeness flags describe the encoded instance,
its transport, and the full retained observation view; they do not claim coverage
of all PostgreSQL behavior. Any later filtered or truncated view must recompute
its own view-completeness flag. Render SQL, identifiers and values as untrusted
text. Feedback should bind both immutable instance and model-revision identities;
it must not alter solver evidence or turn one accepted example into a proof.

Run `python validation/instances.py` after loading `scripts/env.ps1`. The harness
uses a fresh CLI/JVM for every command, checks native and finite-table examples
and counterexamples, actual exhaustion, configured limits, restart/idempotence,
grounded references, archive corruption and a validly rehashed but semantically
altered record. Its isolated temporary archive is removed after the run. The
checked structural obligations are in `SQLollaM2/Enumeration.lean`; they do not
verify the Rust filesystem implementation or assume cryptographic collision
resistance as a Lean axiom.
