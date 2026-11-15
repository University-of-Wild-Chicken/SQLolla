<# Focused native protocol tests. SQL is supplied as exact bytes through stdin.
   No database or provider is contacted. Node/field expectations are independent fixtures. #>
[CmdletBinding()]
param([string]$ParserExe = "$PSScriptRoot\.lake\build\bin\sqlolla_parser.exe", [switch]$Corpus)
$ErrorActionPreference = 'Stop'

function Initialize-SchemaChecks {
    # Retain bounded response frames for one independently pinned batch schema validator.
    $script:SchemaResponses = [Collections.Generic.List[string]]::new()
}

function Start-ParserProcess([string[]]$Arguments) {
    # Create hidden redirected pipes, never a shell command containing SQL.
    $start = [Diagnostics.ProcessStartInfo]::new($ParserExe)
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardInput = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.StandardOutputEncoding = [Text.Encoding]::UTF8
    $start.StandardErrorEncoding = [Text.Encoding]::UTF8
    foreach ($argument in $Arguments) { $start.ArgumentList.Add($argument) }
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $start
    [void]$process.Start()
    return $process
}

function Invoke-ParserBytes([byte[]]$Bytes, [string[]]$Arguments = @()) {
    # Close stdin to finish framing; enforce a supervisor deadline independently of byte limits.
    $process = Start-ParserProcess $Arguments
    $outTask = $process.StandardOutput.ReadToEndAsync()
    $errTask = $process.StandardError.ReadToEndAsync()
    if ($Bytes.Length) { $process.StandardInput.BaseStream.Write($Bytes, 0, $Bytes.Length) }
    $process.StandardInput.Close()
    if (-not $process.WaitForExit(10000)) {
        $process.Kill($true)
        throw 'Parser exceeded ten-second test deadline'
    }
    $result = @{exit_code=$process.ExitCode; stdout=$outTask.Result; stderr=$errTask.Result}
    $process.Dispose()
    return $result
}

function Get-AstPath($Value, [string]$Path) {
    # Follow fixture-owned paths through objects and ordered tuple/column arrays.
    foreach ($segment in $Path.Split('.')) {
        if ($Value -is [System.Collections.IDictionary]) { $Value = $Value[$segment] }
        else { $Value = $Value[[int]$segment] }
    }
    return $Value
}

function Assert-Envelope($Run) {
    # Reject malformed framing now and queue complete contract validation before the suite can pass.
    if ($Run.stderr) { throw 'Unexpected native stderr' }
    $decoded = $Run.stdout | ConvertFrom-Json -AsHashtable
    if ($decoded.protocol -notin @('sqlolla.parser.v1', 'sqlolla.parser.v2')) { throw 'Unknown parser protocol' }
    $script:SchemaResponses.Add($Run.stdout.TrimEnd())
    return $decoded
}

function Test-SqlCase($Case) {
    # Check expected success/refusal shape and deterministic bytes on repeated native runs.
    Write-Verbose "Checking $($Case.id)"
    $bytes = [Text.Encoding]::UTF8.GetBytes($Case.sql)
    $first = Invoke-ParserBytes $bytes
    $second = Invoke-ParserBytes $bytes
    try { $decoded = Assert-Envelope $first }
    catch { throw "$($Case.id): $($_.Exception.Message) response=$($first.stdout)" }
    if ($first.exit_code -ne 0 -or $first.stdout -cne $second.stdout) { throw "$($Case.id): process/repeat mismatch" }
    if ($decoded.result.parse_status -ne $Case.parse) { throw "$($Case.id): parse mismatch" }
    if ($decoded.result.export_status -ne $Case.export) { throw "$($Case.id): export mismatch" }
    if ($decoded.protocol -cne ($Case.protocol ?? 'sqlolla.parser.v1')) { throw "$($Case.id): protocol mismatch" }
    if ($Case.diagnostic -and $decoded.result.diagnostic -ne $Case.diagnostic) { throw "$($Case.id): diagnostic mismatch" }
    foreach ($entry in $Case.checks.GetEnumerator()) {
        $actual = Get-AstPath $decoded.result $entry.Key
        if (($actual | ConvertTo-Json -Compress) -cne ($entry.Value | ConvertTo-Json -Compress)) {
            throw "$($Case.id): AST field mismatch: $($entry.Key)"
        }
    }
    return @{id=$Case.id; passed=$true; parse=$decoded.result.parse_status; export=$decoded.result.export_status}
}

function Test-Refusal([string]$Id, [byte[]]$Bytes, [string]$Code, [string[]]$Arguments = @(), [int]$ExitCode = 0) {
    # Exercise transport and resource boundaries independently of authored SQL fixtures.
    $run = Invoke-ParserBytes $Bytes $Arguments
    $decoded = Assert-Envelope $run
    if ($run.exit_code -ne $ExitCode -or $decoded.result.diagnostic -ne $Code) { throw "$Id failed" }
    if ($decoded.result.ContainsKey('ast')) { throw "$Id leaked a partial AST" }
    return @{id=$Id; passed=$true; diagnostic=$Code}
}

function Test-CorpusExport($Record) {
    # Validate complete exported schemas on exact pinned corpus SQL; this is not execution evidence.
    Write-Verbose "Checking $($Record.id)"
    $run = Invoke-ParserBytes ([Text.Encoding]::UTF8.GetBytes($Record.sql))
    $decoded = Assert-Envelope $run
    if ($run.exit_code -ne 0 -or $decoded.result.export_status -cne 'exported') {
        throw "$($Record.id): corpus syntax export failed: $($run.stdout)"
    }
    return @{id=$Record.id; passed=$true; protocol=$decoded.protocol; scope='syntax_export_only'}
}

Initialize-SchemaChecks
$cases = Get-Content -LiteralPath "$PSScriptRoot\tests.json" -Raw | ConvertFrom-Json -AsHashtable
$results = [Collections.Generic.List[object]]::new()
foreach ($case in $cases) {
    if (-not $case.ContainsKey('checks')) { $case.checks = @{} }
    $results.Add((Test-SqlCase $case))
}
$results.Add((Test-Refusal 'invalid_utf8' ([byte[]]@(0xC3,0x28)) 'input.invalid_utf8'))
$results.Add((Test-Refusal 'nul_byte' ([byte[]]@(0)) 'input.nul'))
$results.Add((Test-Refusal 'oversize' ([byte[]]::new(65537)) 'resource.source_bytes'))
$results.Add((Test-Refusal 'arguments_forbidden' ([byte[]]@()) 'input.arguments_forbidden' @('SELECT 1') 2))
$deepExpression = [Text.Encoding]::UTF8.GetBytes('SELECT ' + ('NOT ' * 70) + 'true;')
$results.Add((Test-Refusal 'expression_depth' $deepExpression 'resource.expression_depth'))
$deepSource = [Text.Encoding]::UTF8.GetBytes('SELECT ' + ('(' * 65) + '1' + (')' * 65) + ';')
$results.Add((Test-Refusal 'source_depth' $deepSource 'resource.source_depth'))
$manyUnits = [Text.Encoding]::UTF8.GetBytes('SELECT ' + ('NOT ' * 1025) + 'true;')
$results.Add((Test-Refusal 'source_units' $manyUnits 'resource.source_units'))
$quotedDepth = @{id='quoted_depth_not_syntax'; sql=("SELECT '" + ('(' * 100) + "';"); parse='parsed'; export='exported'; checks=@{}}
$results.Add((Test-SqlCase $quotedDepth))
$commentDepth = @{id='comment_depth_not_syntax'; sql=('SELECT /* ' + ('(' * 100) + ' */ 1;'); parse='parsed'; export='exported'; checks=@{}}
$results.Add((Test-SqlCase $commentDepth))
$escapedQuotes = @{id='escaped_quotes'; sql="SELECT 'one''two(((' AS label;"; parse='parsed'; export='exported'; checks=@{'ast.items.0.value.value'="one'two((("}}
$results.Add((Test-SqlCase $escapedQuotes))
$quotedName = @{id='quoted_identifier_depth'; sql=('SELECT "' + ('(' * 100) + '" FROM accounts;'); parse='parsed'; export='exported'; checks=@{}}
$results.Add((Test-SqlCase $quotedName))
$lineComment = @{id='line_comment_units'; sql=('-- ' + ('NOT ' * 1025) + "`nSELECT 1;"); parse='parsed'; export='exported'; checks=@{}}
$results.Add((Test-SqlCase $lineComment))
$nestedComments = [Text.Encoding]::UTF8.GetBytes(('/* ' * 65) + (' */' * 65) + ' SELECT 1;')
$results.Add((Test-Refusal 'nested_comment_depth' $nestedComments 'resource.source_depth'))
$emptyInput = @{id='empty_input'; sql=''; parse='rejected'; export='unsupported'; diagnostic='syntax.rejected'; checks=@{}}
$results.Add((Test-SqlCase $emptyInput))
$boundary = @{id='exact_byte_limit'; sql=('SELECT 1;' + (' ' * (65536 - 9))); parse='parsed'; export='exported'; checks=@{}}
$results.Add((Test-SqlCase $boundary))
if ($Corpus) {
    foreach ($population in @('benchbase', 'river', 'temporal')) {
        $file = Join-Path $PSScriptRoot "../../dependencies/SQLeanParser/fixtures/external_postgresql/$population.jsonl"
        foreach ($line in Get-Content -LiteralPath $file) {
            $results.Add((Test-CorpusExport ($line | ConvertFrom-Json -AsHashtable)))
        }
    }
}
$script:SchemaResponses | & python "$PSScriptRoot\schema_check.py"
if ($LASTEXITCODE -ne 0) { throw 'Independent response schema validation failed' }
$summary = [ordered]@{protocols=@('sqlolla.parser.v1','sqlolla.parser.v2'); total=$results.Count; passed=$results.Count;
    executable_sha256=(Get-FileHash -LiteralPath $ParserExe -Algorithm SHA256).Hash.ToLower(); cases=@($results)}
$summary | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath "$PSScriptRoot\test-results.json" -Encoding utf8
Write-Output "Parser protocol: $($summary.passed)/$($summary.total) checks passed"
