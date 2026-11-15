<#
.SYNOPSIS
Build the M0 specification and reject prohibited proof shortcuts.
.DESCRIPTION
Uses an explicit portable Lake path, inventories source modules, runs the
compiled trust gate and writes reproducible evidence without global changes.
#>
param([string]$LakePath = '')

$ErrorActionPreference = 'Stop'
if (-not $LakePath) {
    $LakePath = Join-Path $PSScriptRoot '../m0/parser/.tools/lean-4.34.0-windows/bin/lake.exe'
}
$LakePath = (Resolve-Path -LiteralPath $LakePath).Path
$sourceFiles = @(Get-ChildItem -LiteralPath $PSScriptRoot -Filter '*.lean' -Recurse |
    Where-Object { $_.FullName -notmatch '[\\/]\.lake[\\/]' })
$forbidden = '(?<![\p{L}\p{N}_])(sorry|sorryAx|admit|native_decide|unsafe|unsafeCast|ofReduceBool|ofReduceNat|implemented_by|extern|skipKernelTC)(?![\p{L}\p{N}_])|(?m)^\s*(?:private\s+|protected\s+)?(?:axiom|opaque|partial)\b'
$rootSource = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot 'SQLolla.lean')
$inventory = foreach ($sourceFile in $sourceFiles) {
    $source = Get-Content -Raw -LiteralPath $sourceFile.FullName
    if ([regex]::IsMatch($source, $forbidden)) {
        throw "Prohibited source construct in $($sourceFile.FullName)"
    }
    $relative = [IO.Path]::GetRelativePath($PSScriptRoot, $sourceFile.FullName)
    if ($relative.StartsWith('SQLolla\') -or $relative.StartsWith('SQLolla/')) {
        $module = ($relative -replace '\.lean$', '') -replace '[\\/]', '.'
        if ($rootSource -notmatch ('(?m)^import ' + [regex]::Escape($module) + '\s*$')) {
            throw "Module omitted from audited aggregate: $module"
        }
    }
    [ordered]@{ path = $relative; sha256 = (Get-FileHash -LiteralPath $sourceFile.FullName).Hash }
}
$evidenceDirectory = Join-Path $PSScriptRoot '../m0/formal'
[IO.Directory]::CreateDirectory($evidenceDirectory) | Out-Null
Push-Location $PSScriptRoot
try {
    & $LakePath build *> (Join-Path $evidenceDirectory 'build.log')
    if ($LASTEXITCODE -ne 0) { throw 'Lean build failed; see m0/formal/build.log' }
    & $LakePath env lean TrustAudit.lean *> (Join-Path $evidenceDirectory 'trust.log')
    if ($LASTEXITCODE -ne 0) { throw 'Compiled trust audit failed; see m0/formal/trust.log' }
} finally { Pop-Location }
$trustLog = Get-Content -Raw -LiteralPath (Join-Path $evidenceDirectory 'trust.log')
$summary = [regex]::Match($trustLog, 'TRUST_PASS declarations=(\d+) theorems=(\d+) foundations=([^\r\n]+)')
if (-not $summary.Success) { throw 'Trust audit emitted no acceptance record' }
$report = [ordered]@{
    status = 'PASS'
    scope = 'M0 Lean specification; no SQL/PostgreSQL/Alloy equivalence proof'
    compiler = 'Lean 4.34.0 portable Windows x64'
    source_files = $inventory
    source_shortcut_gate = 'PASS'
    compiled_declarations = [int]$summary.Groups[1].Value
    compiled_theorems_including_generated = [int]$summary.Groups[2].Value
    observed_foundations = $summary.Groups[3].Value
    custom_or_unapproved_axioms = 0
    namespace_boundary = 'All declarations from imported SQLolla modules, including private declarations'
    caveat = 'Trusts the pinned Lean kernel/toolchain; not a sandbox for hostile metaprograms'
}
$report | ConvertTo-Json -Depth 8 | Set-Content -Encoding utf8NoBOM -LiteralPath (Join-Path $evidenceDirectory 'report.json')
Write-Output $summary.Value
Write-Output "Source gate passed for $($sourceFiles.Count) files."
