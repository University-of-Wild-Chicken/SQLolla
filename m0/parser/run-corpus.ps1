<# Replay a small, predetermined source sample without executing SQL.
   The full upstream corpus is not rerun, and its historical counts are not reused. #>
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\run-probe.ps1" -LibraryOnly
$dependency = 'C:\HackUTA26\dependencies\SQLeanParser'

function Get-TextDigest([string]$Text) {
    # Bind each exact UTF-8 SQL specimen to its recorded source without copying source code.
    return [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData(
        [Text.Encoding]::UTF8.GetBytes($Text))).ToLower()
}

function Observe-CorpusRecord($InputRecord) {
    # Record observational parse/export results rather than relabel them static validity.
    $first = Invoke-ParserSpecimen $InputRecord.sql
    $second = Invoke-ParserSpecimen $InputRecord.sql
    if ($first.exit_code -ne 0 -or $first.stderr) { throw 'Corpus process failure' }
    $report = $first.stdout | ConvertFrom-Json -AsHashtable
    return [ordered]@{
        id=$InputRecord.id; file=$InputRecord.file; line=$InputRecord.line;
        source_url=$InputRecord.source_url; license=$InputRecord.license;
        sql_sha256=(Get-TextDigest $InputRecord.sql); parse_status=$report.result.parse_status;
        export_status=$report.result.export_status; diagnostic=$report.result.diagnostic;
        canonical_ast_roundtrip=$report.result.canonical_ast_roundtrip;
        deterministic_repeat=($first.stdout -ceq $second.stdout)
    }
}

$inputs = [Collections.Generic.List[object]]::new()
foreach ($category in @('select','insert','update','delete')) {
    $file = "fixtures/crud/$category.sql"
    $lines = @(Get-Content -LiteralPath "$dependency/$file" -TotalCount 5)
    for ($index = 0; $index -lt $lines.Count; $index++) {
        $inputs.Add(@{id="synthetic-$category-$($index+1)"; sql=$lines[$index]; file=$file;
            line=($index+1); source_url=$null; license='project license not found at checkout root'})
    }
}
foreach ($source in @('river','temporal')) {
    $file = "fixtures/external_postgresql/$source.jsonl"
    $rows = @(Get-Content -LiteralPath "$dependency/$file" -TotalCount 3)
    for ($index = 0; $index -lt $rows.Count; $index++) {
        $row = $rows[$index] | ConvertFrom-Json -AsHashtable
        $inputs.Add(@{id=$row.id; sql=$row.sql; file=$file; line=($index+1);
            source_url=$row.source_url; license=$row.license})
    }
}
$observations = @($inputs | ForEach-Object { Observe-CorpusRecord $_ })
$result = [ordered]@{
    kind='m0-small-upstream-fixture-replay'; semantic_proof=$false; sql_executed=$false;
    parser_commit='ad15c585155ff7fc2c5fc4c9746905e4959ff5eb';
    sampling='First five records of each CRUD .sql file; first three JSONL records each of River and Temporal';
    total=$observations.Count; parsed=@($observations | Where-Object parse_status -eq 'parsed').Count;
    exported=@($observations | Where-Object export_status -eq 'exported').Count;
    deterministic=@($observations | Where-Object deterministic_repeat).Count;
    observations=$observations
}
$result | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath "$PSScriptRoot\corpus-results.json" -Encoding utf8
Write-Output "Upstream sample: $($result.parsed)/$($result.total) parsed; $($result.exported) exported; $($result.deterministic) deterministic"
if ($result.deterministic -ne $result.total) { exit 1 }
