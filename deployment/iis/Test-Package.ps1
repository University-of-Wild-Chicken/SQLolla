<# .SYNOPSIS Verifies immutable package hashes and the relocated runtime without a provider key. #>
param([switch]$SkipRuntime)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Test-PackageInventory([string]$Directory) {
    <# .SYNOPSIS Reject missing, changed or out-of-root inventory entries before executing binaries. #>
    $manifest = Get-Content -LiteralPath (Join-Path $Directory 'manifest.json') -Raw | ConvertFrom-Json
    foreach ($entry in $manifest.files) {
        $path = [IO.Path]::GetFullPath((Join-Path $Directory $entry.path))
        if (-not $path.StartsWith($Directory + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid inventory path' }
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing package file: $($entry.path)" }
        if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $entry.sha256) {
            throw "Package hash mismatch: $($entry.path)"
        }
    }
    Write-Output "Verified $($manifest.files.Count) immutable package files."
}

function Test-ParserRuntime([string]$Directory) {
    <# .SYNOPSIS Exercise the native parser outside any Lean installation. #>
    $result = 'SELECT 1' | & (Join-Path $Directory 'parser/sqlolla_parser.exe')
    if ($LASTEXITCODE -ne 0) { throw 'Parser failed to start' }
    $value = $result | ConvertFrom-Json
    if ($value.result.parse_status -ne 'parsed' -or $value.result.export_status -ne 'exported') { throw 'Parser smoke failed' }
}

function Test-AlloyRuntime([string]$Directory) {
    <# .SYNOPSIS Require a real SAT solution from the packaged Java17 bridge and Alloy JAR. #>
    $java = Join-Path $Directory 'runtime/java/bin/java.exe'
    $version = & $java --version
    if ($LASTEXITCODE -ne 0 -or ($version -join ' ') -notmatch 'openjdk 17\.') { throw 'Java17 runtime required' }
    $classpath = (Join-Path $Directory 'bridge/classes') + ';' + (Join-Path $Directory 'runtime/alloy/org.alloytools.alloy.dist.jar')
    $query = '{"model":"sig A {} run {some A} for 1","command":0}'
    $result = $query | & $java '-Djava.awt.headless=true' '-cp' $classpath AlloyBridge
    if ($LASTEXITCODE -ne 0) { throw 'Alloy bridge failed to start' }
    $value = $result | ConvertFrom-Json
    if ($value.protocol -ne 'sqlolla.alloy.v1' -or $value.status -ne 'SAT') { throw 'Alloy SAT smoke failed' }
}

$packageRoot = [IO.Path]::GetFullPath($PSScriptRoot)
Test-PackageInventory $packageRoot
if (-not $SkipRuntime) {
    Test-ParserRuntime $packageRoot
    Test-AlloyRuntime $packageRoot
}
Write-Output 'Package checks passed. IIS routing and target OS acceptance remain separate checks.'
