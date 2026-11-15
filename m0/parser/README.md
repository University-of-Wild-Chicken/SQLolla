# SQLolla M0 parser specimen

This directory demonstrates native Windows integration with the pinned local
SQLeanParser dependency. It is a feasibility specimen, not production code.
No SQL is executed, and syntax acceptance is not static validity or security.

Read `report.json` for decisions, caveats, toolchain provenance and exact commands.
`results.json` records 25 authored cases, including expected rejections and
restricted-export failures. `corpus-results.json` records a small predetermined
26-record upstream sample with source provenance and SQL hashes. Both runs used
two executions per specimen; output equality is observed finite evidence only.

Use the portable Lean 4.34.0 executables under `.tools/lean-4.34.0-windows/bin`.
Prefix that directory to PATH for the current shell, then run:

```powershell
& .\.tools\lean-4.34.0-windows\bin\lake.exe build parser_probe
& .\run-probe.ps1
& .\run-corpus.ps1
& .\.tools\lean-4.34.0-windows\bin\lake.exe env lean TrustCheck.lean
```

The local path dependency is pinned by the report to commit
`ad15c585155ff7fc2c5fc4c9746905e4959ff5eb`; check its Git HEAD and tracked diff
before replay. Lake uses only the imported parser/syntax/pretty-printer modules,
not the full upstream test suite, and writes upstream `.lake/` build artifacts.
No upstream tracked source is changed. The local dependency path in
`lakefile.toml` assumes the supplied `C:/HackUTA26` layout.

Every authored Lean definition has a purpose comment and is at most 30 lines.
`TrustCheck.lean` inspects transitive axioms of the entry/export/round-trip paths;
the recorded result contains only standard Lean foundations. This does not prove
the compiler/runtime or SQL execution semantics correct.

The export deliberately excludes nodes outside its specimen fragment. Extending
it requires an explicit tagged schema and fixtures that preserve every field,
including bag flags, parameter identity, exact numbers, qualification and NULL.
The current CLI accepts a SQL argument: it must not become a secret-bearing
production transport. A production adapter needs bounded private IPC, exhaustive
coverage, version negotiation, cancellation, source maps and a separate security
review. The parser's root redistribution license remains unresolved.

`.tools/` and `.lake/` are ignored local build material. The portable toolchain is
shared with other M0 checks; do not remove it while those checks are running.
