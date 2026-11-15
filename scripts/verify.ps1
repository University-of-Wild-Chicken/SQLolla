<# .SYNOPSIS Runs the maintained M1 build, proof-hygiene and semantic validation gates. #>
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'env.ps1')

function Invoke-Check {
    <# .SYNOPSIS Stops at the first failing native validation command. #>
    param([string]$Program, [string[]]$Arguments)
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "Validation failed: $Program $Arguments" }
}

Push-Location $env:SQLolla_ROOT
try {
    & .\scripts\build.ps1
    Invoke-Check cargo -Arguments @('fmt', '--all', '--', '--check')
    Invoke-Check cargo -Arguments @('test', '--workspace', '--locked')
    Invoke-Check cargo -Arguments @('clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings')
    Invoke-Check cargo -Arguments @('rustdoc', '-p', 'sqlolla-core', '--lib', '--', '-D', 'missing-docs')
    Invoke-Check cargo -Arguments @('rustdoc', '-p', 'sqlolla', '--lib', '--', '-D', 'missing-docs')
    Invoke-Check cargo -Arguments @('rustdoc', '-p', 'sqlolla-host', '--lib', '--', '-D', 'missing-docs')
    $javadoc = Join-Path (Split-Path $env:SQLolla_JAVAC) 'javadoc.exe'
    Invoke-Check $javadoc -Arguments @('-private', '-quiet', '-Xdoclint:all', '-Werror', '-classpath', $env:SQLolla_ALLOY_JAR, '-d', 'target/javadoc', 'bridge/AlloyBridge.java')
    Invoke-Check python -Arguments @('scripts/check_style.py')
    & .\formal\verify.ps1 -LakePath $env:SQLolla_LAKE
    & .\contracts\m2\verify.ps1 -LakePath $env:SQLolla_LAKE
    & .\parser\test.ps1
    Invoke-Check python -Arguments @('parser/census.py')
    Invoke-Check python -Arguments @('validation/run.py')
    Invoke-Check python -Arguments @('validation/industrial.py')
    Invoke-Check python -Arguments @('validation/ordering.py')
    Invoke-Check python -Arguments @('validation/distinct_on.py')
    Invoke-Check python -Arguments @('validation/set_returning.py')
    Invoke-Check python -Arguments @('validation/windows.py')
    Invoke-Check python -Arguments @('validation/expression_outcomes.py')
    Invoke-Check python -Arguments @('validation/extended_symbolic.py')
    Invoke-Check python -Arguments @('validation/grouping.py')
    Invoke-Check python -Arguments @('validation/subqueries.py')
    Invoke-Check python -Arguments @('validation/intervals.py')
    Invoke-Check python -Arguments @('validation/cte.py')
    Invoke-Check python -Arguments @('validation/ddl.py')
    Invoke-Check python -Arguments @('validation/catalog_context.py')
    Invoke-Check python -Arguments @('validation/collection_scalars.py')
    Invoke-Check python -Arguments @('validation/comparability.py')
    Invoke-Check python -Arguments @('validation/sql_functions.py')
    Invoke-Check python -Arguments @('validation/regex_subset.py')
    Invoke-Check python -Arguments @('validation/session_effects.py')
    Invoke-Check python -Arguments @('validation/sequence_effects.py')
    Invoke-Check python -Arguments @('validation/xid_scalars.py')
    Invoke-Check python -Arguments @('validation/backend_effects.py')
    Invoke-Check python -Arguments @('validation/system_columns.py')
    Invoke-Check python -Arguments @('validation/system_columns_river.py')
    Invoke-Check python -Arguments @('validation/unique_indexes.py')
    Invoke-Check python -Arguments @('validation/instances.py')
    Invoke-Check python -Arguments @('validation/refinement.py')
    Invoke-Check python -Arguments @('validation/locking.py')
    Invoke-Check python -Arguments @('validation/locking_join.py')
    Invoke-Check python -Arguments @('validation/cte_locks.py')
    Invoke-Check python -Arguments @('validation/lock_initialization.py', '--engine')
    Invoke-Check python -Arguments @('validation/assertion_examples.py')
    Invoke-Check python -Arguments @('validation/comparison_tests.py')
    Invoke-Check python -Arguments @('validation/rejections.py')
    Invoke-Check python -Arguments @('validation/pg_rejections.py')
    Invoke-Check python -Arguments @('validation/symbolic.py')
    Invoke-Check python -Arguments @('validation/mutations.py')
    Invoke-Check python -Arguments @('validation/corpus_wire.py')
    Invoke-Check python -Arguments @('validation/corpus_runtime.py')
    Invoke-Check python -Arguments @('validation/corpus_runtime.py', '--fixture', 'nonempty')
    Invoke-Check python -Arguments @('validation/corpus_semantics.py')
} finally { Pop-Location }
