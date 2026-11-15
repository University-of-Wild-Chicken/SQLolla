<# M0 evidence runner: invokes the native specimen with argument-list transport.
   It does not execute SQL, contact a database, or validate PostgreSQL semantics. #>
param([string]$ProbeExe = "$PSScriptRoot\.lake\build\bin\parser_probe.exe", [switch]$LibraryOnly)
$ErrorActionPreference = 'Stop'
$env:PATH = "$PSScriptRoot\.tools\lean-4.34.0-windows\bin;" + $env:PATH

function Invoke-ParserSpecimen([string]$Sql) {
    # Run one bounded native process without shell interpolation or visible windows.
    $start = [Diagnostics.ProcessStartInfo]::new($ProbeExe)
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.StandardOutputEncoding = [Text.Encoding]::UTF8
    $start.StandardErrorEncoding = [Text.Encoding]::UTF8
    $start.ArgumentList.Add($Sql)
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $start
    [void]$process.Start()
    $outTask = $process.StandardOutput.ReadToEndAsync()
    $errTask = $process.StandardError.ReadToEndAsync()
    if (-not $process.WaitForExit(30000)) {
        $process.Kill($true)
        throw 'Parser specimen exceeded 30-second timeout'
    }
    $result = @{exit_code=$process.ExitCode; stdout=$outTask.Result; stderr=$errTask.Result}
    $process.Dispose()
    return $result
}

function Get-SpecimenPath($Value, [string]$Path) {
    # Traverse a fixture-owned dot path through JSON objects and arrays.
    foreach ($segment in $Path.Split('.')) {
        if ($Value -is [System.Collections.IDictionary]) { $Value = $Value[$segment] }
        else { $Value = $Value[[int]$segment] }
    }
    return $Value
}

function Test-SpecimenCase($Case) {
    # Distinguish parser rejection, restricted export and finite syntax round trips.
    $first = Invoke-ParserSpecimen $Case.sql
    $second = Invoke-ParserSpecimen $Case.sql
    $errors = [Collections.Generic.List[string]]::new()
    if ($first.exit_code -ne 0 -or $first.stderr) { $errors.Add('process failure') }
    if ($first.stdout -cne $second.stdout) { $errors.Add('repeated output differs') }
    $decoded = $first.stdout | ConvertFrom-Json -AsHashtable
    if ($decoded.result.parse_status -ne $Case.parse) { $errors.Add('parse expectation') }
    if ($Case.export -and $decoded.result.export_status -ne $Case.export) { $errors.Add('export expectation') }
    if ($Case.parse -eq 'parsed' -and -not $decoded.result.canonical_ast_roundtrip) { $errors.Add('AST round trip') }
    foreach ($entry in $Case.checks.GetEnumerator()) {
        $actual = Get-SpecimenPath $decoded.result $entry.Key
        if (($actual | ConvertTo-Json -Compress) -cne ($entry.Value | ConvertTo-Json -Compress)) {
            $errors.Add("field mismatch: $($entry.Key)")
        }
    }
    return @{id=$Case.id; sql=$Case.sql; passed=($errors.Count -eq 0); errors=@($errors);
        exit_code=$first.exit_code; deterministic_repeat=($first.stdout -ceq $second.stdout); report=$decoded}
}

if ($LibraryOnly) { return }
$cases = Get-Content -LiteralPath "$PSScriptRoot\cases.json" -Raw | ConvertFrom-Json -AsHashtable
$results = foreach ($case in $cases) {
    if (-not $case.ContainsKey('checks')) { $case.checks = @{} }
    Test-SpecimenCase $case
}
$evidence = [ordered]@{
    kind='m0-native-parser-feasibility'; semantic_proof=$false; cases=@($results);
    total=$results.Count; passed=@($results | Where-Object passed).Count;
    parser_commit='ad15c585155ff7fc2c5fc4c9746905e4959ff5eb';
    executable_sha256=(Get-FileHash -LiteralPath $ProbeExe -Algorithm SHA256).Hash.ToLower();
    probe_source_sha256=(Get-FileHash -LiteralPath "$PSScriptRoot\ParserProbe.lean" -Algorithm SHA256).Hash.ToLower();
    cases_sha256=(Get-FileHash -LiteralPath "$PSScriptRoot\cases.json" -Algorithm SHA256).Hash.ToLower()
}
$evidence | ConvertTo-Json -Depth 80 | Set-Content -LiteralPath "$PSScriptRoot\results.json" -Encoding utf8
Write-Output "Native parser specimens: $($evidence.passed)/$($evidence.total) passed"
$results | Where-Object { -not $_.passed } | ConvertTo-Json -Depth 30
if ($evidence.passed -ne $evidence.total) { exit 1 }
