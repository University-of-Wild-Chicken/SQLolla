# SQLolla parser adapter

The native adapter uses SQLeanParser's uniform advanced AST and certified grammar at commit
`ad15c585155ff7fc2c5fc4c9746905e4959ff5eb`. Rust owns catalog binding, SQL types,
relational algebra and evaluation. This process only parses and exports its
explicit syntax profile. It does not execute SQL or establish database safety.

Send one UTF-8 SQL statement to stdin and close the pipe. Do not pass SQL as a
command argument. Stdout contains one versioned JSON response followed by a
newline. Previously admitted queries retain `sqlolla.parser.v1` and its exact
envelope value; `legacy_export_envelope_unchanged` kernel-checks this compatibility
condition. Extended syntax returns `sqlolla.parser.v2`. The schemas are
[parser-ast.json](../protocol/parser-ast.json) and
[parser-ast-v2.json](../protocol/parser-ast-v2.json).
Only `parse_status: parsed` plus `export_status: exported` includes an AST.
A zero exit code means a report was emitted, including refusals. Command-line
arguments produce a structured refusal and exit code 2. Missing output, process
failure, malformed JSON or a version mismatch must never become success.
Syntax diagnostic `offset` counts Unicode source characters from zero; `line`
and `column` are one-based upstream positions, not UTF-8 byte offsets.

The v1 profile exports SELECT, VALUES, DISTINCT, WHERE, column/wildcard
projections, literal NULL/Boolean/text/integer values, numbered parameters,
comparisons, Boolean operators, IS NULL, integer-literal signs, Cartesian
products, INNER/LEFT/RIGHT/FULL ON joins, explicitly aliased non-LATERAL derived
sources, set operations with their ALL flag, and ordered nonrecursive CTEs.
The Rust binder must resolve CTE references and reject unsupported semantics.
Qualified names, aliases, column aliases, duplicate VALUES rows and tuple order
are retained. Integer magnitudes are strings; v1 parameter indexes are 1–65535.

V2 exports every field of the current upstream typed AST, including DML/DDL,
ordering, pagination, grouping, aggregates, locks, calls, casts and subqueries.
Its `query` wrapper always carries all modifiers explicitly. Structural export
also retains anonymous/out-of-range parameter syntax; Rust decides admission.
SQL NULL has an explicit node, while JSON null represents absent optional syntax.

For syntax outside the upstream typed AST, the adapter invokes SQLeanParser's
certified PostgreSQL16.15 grammar at PostgreSQL commit
`f0595c3d839debdb140903621c1b44a4f4d86256`. Selected complete productions lower to
explicit nodes for views, DEFAULT cells, statement-valued CTEs, UPDATE FROM,
DELETE USING, named/BIT casts, window calls and ARRAY subqueries. Partial-index
conflict predicates remain separate from conflict-action predicates. Exact
UTF-8 subtree spans are reused by the upstream typed parser; SQL is never
rewritten and parameter numbers are retained. There is no raw-SQL or raw-CST
successful fallback. Multiple top-level statements remain rejected.

`CstContract.lean` pins 270 complete production shapes and checks them before
lowering. The census verifies that its production IDs cover the current dispatch
and verifies the upstream grammar source SHA256. Unknown modifiers, explicit
window frames, unsupported source forms and other unmapped grammar fail closed.
The grammar certificate establishes a derivation and source preservation, not
correctness of repository-owned lowering, binding, effects or execution. Every
envelope therefore retains `assurance: syntax-only` and
`semantic_preservation: false`; Rust must reject unimplemented semantics.

The transport cap is 65,536 bytes. Invalid UTF-8 and NUL are refused. Before
parsing, a conservative lexical workload guard limits nesting and nested block
comments to 64 and token-like units to 1,024; quoted text and comment contents
do not count as syntax internally. It does not replace the SQL lexer. AST
query/source traversal and expression traversal each have a 64-level budget.
Certified grammar parsing has a 200,000-transition budget; CST traversal and
lowering have 512-level budgets. Exhaustion cannot become a successful AST.
The JSON response cap is 1 MiB; exceeding it returns a refusal, never a partial
AST. The supervising Rust process must also enforce read/total deadlines,
capture limits and process termination: an input pipe that remains open can
otherwise wait indefinitely. Cache by exact SQL bytes plus parser build/profile.

Build on the supplied Windows workspace with:

```powershell
& .\parser\build.ps1
python -m pip install --target parser/.tools/schema -r parser/schema-requirements.txt
& .\parser\test.ps1
& .\parser\test.ps1 -Corpus
python parser/census.py
```

`build.ps1` accepts `-LeanBin` for another already installed Lean 4.34.0 Windows
toolchain. Its default reuses the verified portable M0 toolchain. On Linux or
macOS, supply the native toolchain directory explicitly:

```sh
LEAN_BIN=/absolute/path/to/lean-4.34.0/bin sh parser/build.sh
```

Both wrappers verify the dependency commit and tracked-source cleanliness.
Lake's local path dependency expects SQLeanParser beside the SQLolla checkout
under `../dependencies/SQLeanParser`. The binary is
`parser/.lake/build/bin/sqlolla_parser.exe` on Windows and `sqlolla_parser` on
Unix. Linux/macOS builds require their native Lean toolchain; these builds have
not yet been exercised locally. No global installation is performed here.

The default Lake build includes `TrustAudit`. It inspects every compiled adapter
declaration, including private and generated names, refuses custom foundations
or unsafe declarations, and permits only `propext`, `Classical.choice` and
`Quot.sound` transitively. This is separate from trusting the compiler/runtime
and from proving SQL semantics. The focused test runner requires PowerShell 7
and the isolated, pinned Python schema validator above; its cases use real native stdin/stdout, schema validation,
independent AST expectations and explicit resource/refusal cases. It makes no
database, provider or external service requests.
The test-only dependency versions are in `schema-requirements.txt`; tested wheel
URLs, hashes and licenses are in `schema-tools.json`. No global package is installed.
Schemas use guarded tag alternatives to avoid repeatedly traversing unrelated
recursive node families. Validation runs once over the response batch with
`jsonschema 4.25.1`; neither that package nor Python is a production parser dependency.

`python parser/census.py` replays all 276 pinned BenchBase, River and Temporal
records through the production adapter. Its local `census-results.json` separates
exported, parsed-but-unsupported, syntax-rejected and preflight-refused inputs.
The report preserves record IDs, URLs, transformations, exact SQL hashes and
binary/corpus hashes without copying the queries. It is export-readiness evidence;
it neither supplies missing application catalogs nor makes PostgreSQL claims.
The current pinned corpus has 276 records and 260 distinct SQL texts; all 276
export structurally. River entries remain unexpanded upstream template constants.
`test.ps1 -Corpus` additionally validates every emitted corpus AST against its
protocol schema. Neither result is an execution-coverage count.

For maintenance, update the pinned dependency identity, build checks, Rust
consumer and protocol schema together. Every newly admitted AST constructor or
modifier needs an explicit field mapping, a positive fixture and a refusal or
boundary fixture. Preserve stable diagnostic codes; bump the protocol for
incompatible changes. Every authored Lean definition has a purpose comment and
is at most 30 lines. No upstream tracked files are modified. Confirm the parser
dependency's currently unresolved root redistribution license before shipping it.
