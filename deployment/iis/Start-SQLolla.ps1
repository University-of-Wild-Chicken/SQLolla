<# .SYNOPSIS Starts the private Rust host using only paths inside this extracted package. #>
param([string]$Configuration = (Join-Path $PSScriptRoot 'deployment.json'))
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Set-SQLollaRuntime([string]$Package, [object]$Settings) {
    <# .SYNOPSIS Resolve deployment paths without loading development tools or credentials. #>
    $env:SQLolla_ROOT = $Package
    $env:SQLolla_PARSER = Join-Path $Package 'parser/sqlolla_parser.exe'
    $env:SQLolla_JAVA = Join-Path $Package 'runtime/java/bin/java.exe'
    $env:SQLolla_ALLOY_JAR = Join-Path $Package 'runtime/alloy/org.alloytools.alloy.dist.jar'
    $env:SQLolla_PUBLIC_ORIGIN = [string]$Settings.public_origin
    $env:SQLolla_HOST_PORT = [string]$Settings.port
    $data = [IO.Path]::GetFullPath([string]$Settings.data_directory)
    if (-not [IO.Path]::IsPathRooted([string]$Settings.data_directory)) { throw 'Data directory must be absolute' }
    if ($data.StartsWith($Package + '\', [StringComparison]::OrdinalIgnoreCase) -or $data -eq $Package) {
        throw 'Use persistent private data outside the release directory'
    }
    $env:SQLolla_INSTANCE_STORE = Join-Path $data 'instances'
    $env:TEMP = Join-Path $data 'temp'
    $env:TMP = $env:TEMP
    [IO.Directory]::CreateDirectory($env:SQLolla_INSTANCE_STORE) | Out-Null
    [IO.Directory]::CreateDirectory($env:TEMP) | Out-Null
}

$packageRoot = [IO.Path]::GetFullPath($PSScriptRoot)
$settings = Get-Content -LiteralPath $Configuration -Raw | ConvertFrom-Json
if (-not [Environment]::Is64BitOperatingSystem) { throw 'Windows x64 is required' }
if ([int]$settings.port -lt 1024 -or [int]$settings.port -gt 65535) { throw 'Choose a port between 1024 and 65535' }
Set-SQLollaRuntime $packageRoot $settings
Push-Location $packageRoot
try {
    & (Join-Path $packageRoot 'bin/sqlolla-host.exe')
    if ($LASTEXITCODE -ne 0) { throw "SQLolla host exited with code $LASTEXITCODE" }
} finally { Pop-Location }
