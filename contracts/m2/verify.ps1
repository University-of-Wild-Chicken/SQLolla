<#
.SYNOPSIS
Kernel-check the isolated M2 contracts and audit their logical dependencies.
.DESCRIPTION
Checks source admission, module ownership, declaration length and inherited source
hashes. Only the final JSON evidence is retained; compiler diagnostics stay on stdout.
#>
param([string]$LakePath = '')
$ErrorActionPreference = 'Stop'

function Get-SourceInventory([string]$Directory) {
    <# .SYNOPSIS Record canonical paths and hashes without entering build caches. #>
    Get-ChildItem -LiteralPath $Directory -Recurse -Filter '*.lean' |
        Where-Object { $_.FullName -notmatch '[\\/]\.lake[\\/]' } |
        Sort-Object FullName | ForEach-Object {
            [ordered]@{ path = [IO.Path]::GetRelativePath($Directory, $_.FullName)
                sha256 = (Get-FileHash -LiteralPath $_.FullName).Hash.ToLowerInvariant() }
        }
}

function Test-Sources([object[]]$Inventory) {
    <# .SYNOPSIS Reject proof shortcuts, omitted modules and oversized declarations. #>
    $forbidden = '(?<![\p{L}\p{N}_])(sorry|sorryAx|admit|native_decide|unsafe|unsafeCast|ofReduceBool|ofReduceNat|implemented_by|extern|skipKernelTC)(?![\p{L}\p{N}_])|(?m)^\s*(?:private\s+|protected\s+)?(?:axiom|opaque|partial)\b'
    $aggregate = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot 'SQLollaM2.lean')
    foreach ($entry in $Inventory) {
        $source = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot $entry.path)
        if ([regex]::IsMatch($source, $forbidden)) { throw "Prohibited construct: $($entry.path)" }
        if ($entry.path -match '^SQLollaM2[\\/]') {
            $module = ($entry.path -replace '\.lean$', '') -replace '[\\/]', '.'
            if ($aggregate -notmatch ('(?m)^import ' + [regex]::Escape($module) + '\s*$')) {
                throw "Module omitted from aggregate: $module"
            }
        }
        $blocks = [regex]::Matches($source, '(?m)^(?:def|theorem|structure|inductive|abbrev|instance|run_cmd)\b[^\r\n]*(?:\r?\n(?!\r?$)[^\r\n]*)*')
        foreach ($block in $blocks) {
            if (($block.Value -split '\r?\n').Count -gt 30) { throw "Declaration exceeds 30 lines: $($entry.path)" }
        }
    }
}

if (-not $LakePath) {
    $LakePath = Join-Path $PSScriptRoot '../../m0/parser/.tools/lean-4.34.0-windows/bin/lake.exe'
}
$LakePath = (Resolve-Path -LiteralPath $LakePath).Path
$leanPath = Join-Path (Split-Path $LakePath) $(if ($IsWindows) { 'lean.exe' } else { 'lean' })
$version = (& $leanPath --version | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or $version -notmatch 'version 4\.34\.0') { throw 'Require Lean 4.34.0' }
$inheritedDirectory = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../../formal')).Path
$inheritedBefore = @(Get-SourceInventory $inheritedDirectory)
$inventory = @(Get-SourceInventory $PSScriptRoot)
Test-Sources $inventory
Push-Location $PSScriptRoot
try {
    $build = & $LakePath build 2>&1
    if ($LASTEXITCODE -ne 0) { $build | Write-Output; throw 'M2 Lean build failed' }
    $audit = & $LakePath env lean M2TrustAudit.lean 2>&1
    if ($LASTEXITCODE -ne 0) { $audit | Write-Output; throw 'M2 compiled trust audit failed' }
} finally { Pop-Location }
$inheritedAfter = @(Get-SourceInventory $inheritedDirectory)
if (($inheritedBefore | ConvertTo-Json -Depth 5) -cne ($inheritedAfter | ConvertTo-Json -Depth 5)) {
    throw 'Inherited M0 source changed during verification'
}
$summary = [regex]::Match(($audit | Out-String), 'M2_TRUST_PASS declarations=(\d+) theorems=(\d+) inherited=(\d+) foundations=([^\r\n]+)')
if (-not $summary.Success) { throw 'Compiled trust gate emitted no acceptance record' }
$authoredTheorems = (Get-ChildItem -LiteralPath (Join-Path $PSScriptRoot 'SQLollaM2') -Filter '*.lean' |
    Select-String -Pattern '^theorem\s').Count
$report = [ordered]@{
    status = 'PASS'; milestone = 'M2_MAINTAINED_SPECIFICATION'; compiler = $version
    compiler_sha256 = (Get-FileHash -LiteralPath $leanPath).Hash.ToLowerInvariant()
    source_shortcut_gate = 'PASS'; maximum_authored_declaration_lines = 30
    authored_theorem_statements = $authoredTheorems
    compiled_draft_declarations = [int]$summary.Groups[1].Value
    compiled_draft_theorems_including_generated = [int]$summary.Groups[2].Value
    compiled_inherited_declarations = [int]$summary.Groups[3].Value
    observed_foundations = $summary.Groups[4].Value
    custom_or_unapproved_axioms = 0; inherited_sources_unchanged = $true
    draft_sources = $inventory; inherited_sources = $inheritedBefore
    claim = 'Kernel-checked abstract contracts and conditional lemmas; no PostgreSQL/Rust/Alloy preservation proof.'
    audit_boundary = 'Module-owned SQLollaM2 and SQLolla declarations, including private/generated declarations and transitive axiom dependencies.'
    trust_boundary = 'Pinned Lean kernel/toolchain and audit metaprogram; not a hostile-code sandbox.'
}
$report | ConvertTo-Json -Depth 8 | Set-Content -Encoding utf8NoBOM -LiteralPath (Join-Path $PSScriptRoot 'verification.json')
Write-Output $summary.Value
Write-Output "Source, module inventory and 30-line gates passed; $authoredTheorems authored theorem statements."
