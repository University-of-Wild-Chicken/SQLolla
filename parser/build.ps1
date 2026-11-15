<# Build the pinned native adapter and its mandatory compiled trust audit.
   No toolchain installation or tracked dependency edits are performed. #>
param([string]$LeanBin = "$PSScriptRoot\..\m0\parser\.tools\lean-4.34.0-windows\bin")
$ErrorActionPreference = 'Stop'
$dependency = [IO.Path]::GetFullPath("$PSScriptRoot\..\..\dependencies\SQLeanParser")
$expectedCommit = 'ad15c585155ff7fc2c5fc4c9746905e4959ff5eb'
$actualCommit = (& git -C $dependency rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $actualCommit -ne $expectedCommit) { throw 'SQLeanParser revision mismatch' }
& git -C $dependency diff --quiet HEAD --
if ($LASTEXITCODE -ne 0) { throw 'SQLeanParser has tracked source changes' }
$version = (& "$LeanBin\lean.exe" --version) -join ''
if ($LASTEXITCODE -ne 0 -or $version -notmatch 'version 4\.34\.0,') { throw 'Lean 4.34.0 required' }
$sourceFiles = @(Get-ChildItem -LiteralPath $PSScriptRoot -Filter '*.lean' -File)
$sourceFiles += Get-ChildItem -LiteralPath "$PSScriptRoot\SQLollaParser" -Filter '*.lean' -File
$forbidden = '\b(sorry|sorryAx|admit|axiom|native_decide|unsafe)\b'
foreach ($file in $sourceFiles) {
    if (Select-String -LiteralPath $file.FullName -Pattern $forbidden -CaseSensitive -Quiet) {
        throw "Prohibited trust shortcut in $($file.Name)"
    }
}
$savedPath = $env:PATH
Push-Location $PSScriptRoot
try {
    $env:PATH = "$LeanBin;" + $savedPath
    & "$LeanBin\lake.exe" build
    if ($LASTEXITCODE -ne 0) { throw 'Parser build or compiled trust audit failed' }
} finally {
    $env:PATH = $savedPath
    Pop-Location
}
