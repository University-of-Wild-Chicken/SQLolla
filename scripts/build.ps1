<# .SYNOPSIS Builds the pinned parser, Rust compiler and private Java17 Alloy worker. #>
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'env.ps1')
Push-Location $env:SQLolla_ROOT
try {
    & .\parser\build.ps1
    if ($LASTEXITCODE) { throw 'Parser build failed' }
    $jarHash = (Get-FileHash -LiteralPath $env:SQLolla_ALLOY_JAR -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($jarHash -ne '6b8c1cb5bc93bedfc7c61435c4e1ab6e688a242dc702a394628d9a9801edb78d') {
        throw 'Alloy artifact hash differs from the validated M0 pin'
    }
    [IO.Directory]::CreateDirectory((Join-Path $env:SQLolla_ROOT 'bridge/classes')) | Out-Null
    & $env:SQLolla_JAVAC --release 17 -Xlint:all -cp $env:SQLolla_ALLOY_JAR -d bridge/classes bridge/AlloyBridge.java
    if ($LASTEXITCODE) { throw 'Java bridge compilation failed' }
    cargo build --workspace --locked
    if ($LASTEXITCODE) { throw 'Rust build failed' }
} finally { Pop-Location }
