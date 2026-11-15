<#! Non-production M0 checks; writes only below this script's directory. #>
param([string]$JdkPath = 'C:\HackUTA26\SQLolla\m0\alloy\.tools\jdk-17.0.21+9')
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$probeRoot = $PSScriptRoot
$utf8 = [System.Text.UTF8Encoding]::new($false, $true)

# Constructs a hidden process without shell interpolation or ambient PATH lookup.
function New-ProbeStartInfo([string]$executable, [string[]]$arguments, [string]$directory) {
    $info = [System.Diagnostics.ProcessStartInfo]::new($executable)
    foreach ($argument in $arguments) { $info.ArgumentList.Add($argument) }
    $info.WorkingDirectory = $directory
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.StandardInputEncoding = $utf8
    $info.StandardOutputEncoding = $utf8
    $info.StandardErrorEncoding = $utf8
    return $info
}

# Drains both output pipes concurrently and imposes a five-second watchdog.
function Invoke-ProbeProcess($info, [AllowNull()][string]$inputLine) {
    $process = [System.Diagnostics.Process]::Start($info)
    $stdoutTask = $process.StandardOutput.ReadToEndAsync()
    $stderrTask = $process.StandardError.ReadToEndAsync()
    if ($null -ne $inputLine) { $process.StandardInput.WriteLine($inputLine) }
    $process.StandardInput.Close()
    if (-not $process.WaitForExit(5000)) { $process.Kill($true); throw 'Probe timeout' }
    $result = [ordered]@{ exit_code = $process.ExitCode; stdout = $stdoutTask.Result; stderr = $stderrTask.Result }
    $process.Dispose()
    return $result
}

# Confirms a started Java process can be terminated without opening a window.
function Test-DirectCancellation($info) {
    $process = [System.Diagnostics.Process]::Start($info)
    $ready = $process.StandardOutput.ReadLineAsync()
    if (-not $ready.Wait(5000)) { $process.Kill($true); throw 'Readiness timeout' }
    $timer = [System.Diagnostics.Stopwatch]::StartNew()
    $process.Kill($true)
    $stopped = $process.WaitForExit(3000)
    $timer.Stop()
    $result = [ordered]@{ ready = $ready.Result; exited = $stopped; elapsed_ms = $timer.ElapsedMilliseconds; exit_code = $process.ExitCode; descendants_tested = $false }
    $process.Dispose()
    return $result
}

$scratch = Join-Path $probeRoot 'space ü 数据'
[System.IO.Directory]::CreateDirectory($scratch) | Out-Null
$payload = [ordered]@{ protocol = 'm0-probe/1'; text = "tenant_ü_数据_😀`nescaped newline"; path = $scratch }
$line = $payload | ConvertTo-Json -Compress
$fixture = Join-Path $scratch 'fixture.json'
[System.IO.File]::WriteAllText($fixture, $line, $utf8)
$bytes = [System.IO.File]::ReadAllBytes($fixture)
$roundtrip = [System.IO.File]::ReadAllText($fixture, $utf8)
$javac = Join-Path $JdkPath 'bin\javac.exe'
$java = Join-Path $JdkPath 'bin\java.exe'
$source = Join-Path $probeRoot 'ProtocolProbe.java'
$compileInfo = New-ProbeStartInfo $javac @('-encoding', 'UTF-8', '-d', $scratch, $source) $scratch
$compile = Invoke-ProbeProcess $compileInfo $null
if ($compile.exit_code -ne 0) { throw $compile.stderr }
$marker = 'argument with spaces ü 数据'
$echoInfo = New-ProbeStartInfo $java @('-Djava.awt.headless=true', '-cp', $scratch, 'ProtocolProbe', 'echo', $marker) $scratch
$echo = Invoke-ProbeProcess $echoInfo $line
$cancelInfo = New-ProbeStartInfo $java @('-Djava.awt.headless=true', '-cp', $scratch, 'ProtocolProbe', 'sleep') $scratch
$cancellation = Test-DirectCancellation $cancelInfo
$results = [ordered]@{
    schema_version = 1
    non_production = $true
    platform = [System.Runtime.InteropServices.RuntimeInformation]::OSDescription
    jdk_path = $JdkPath
    utf8_file_roundtrip = ($roundtrip -ceq $line)
    utf8_file_has_bom = ($bytes.Length -ge 3 -and $bytes[0] -eq 239 -and $bytes[1] -eq 187 -and $bytes[2] -eq 191)
    javac = $compile
    echo = $echo
    stdout_json_frame_roundtrip = ($echo.stdout.TrimEnd("`r", "`n") -ceq $line)
    stderr_separate_and_argument_intact = ($echo.stderr.TrimEnd("`r", "`n") -ceq $marker)
    direct_cancellation = $cancellation
    not_tested = @('descendant process cleanup', 'large-pipe backpressure', 'resource limits', 'Linux execution', 'macOS execution', 'read-only install', 'long paths', 'Alloy protocol')
}
[System.IO.File]::WriteAllText((Join-Path $probeRoot 'native-probes.json'), ($results | ConvertTo-Json -Depth 8), $utf8)
$results | ConvertTo-Json -Depth 8
