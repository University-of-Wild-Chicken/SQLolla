<# .SYNOPSIS Sets process-local tool defaults without replacing explicit installation paths. #>

# Keep explicitly provisioned tool paths; otherwise select the pinned local runtime.
function Set-SQLollaToolDefault([string]$Name, [string]$Value) {
    if (-not [Environment]::GetEnvironmentVariable($Name, 'Process')) {
        [Environment]::SetEnvironmentVariable($Name, $Value, 'Process')
    }
}

$taskRoot = if ($env:SQLolla_ROOT) {
    [IO.Path]::GetFullPath($env:SQLolla_ROOT)
} else { Split-Path -Parent $PSScriptRoot }
$rustPrefix = Join-Path $taskRoot 'm0/portability/.tools/rust-1.100.0-x86_64-pc-windows-msvc'
Set-SQLollaToolDefault 'RUSTC' (Join-Path $rustPrefix 'bin/rustc.exe')
Set-SQLollaToolDefault 'RUSTDOC' (Join-Path $rustPrefix 'bin/rustdoc.exe')
$rustBin = Join-Path $rustPrefix 'bin'
if (($env:PATH -split [IO.Path]::PathSeparator) -notcontains $rustBin) {
    $env:PATH = $rustBin + [IO.Path]::PathSeparator + $env:PATH
}
Set-SQLollaToolDefault 'SQLolla_ROOT' $taskRoot
Set-SQLollaToolDefault 'SQLolla_JAVA' (Join-Path $taskRoot 'm0/alloy/.tools/jdk-17.0.21+9/bin/java.exe')
Set-SQLollaToolDefault 'SQLolla_JAVAC' (Join-Path $taskRoot 'm0/alloy/.tools/jdk-17.0.21+9/bin/javac.exe')
Set-SQLollaToolDefault 'SQLolla_LAKE' (Join-Path $taskRoot 'm0/parser/.tools/lean-4.34.0-windows/bin/lake.exe')
$alloyDefault = [IO.Path]::GetFullPath((Join-Path $taskRoot '../dependencies/org.alloytools.alloy.dist.jar'))
Set-SQLollaToolDefault 'SQLolla_ALLOY_JAR' $alloyDefault
