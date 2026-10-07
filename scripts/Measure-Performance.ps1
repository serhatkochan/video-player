[CmdletBinding()]
param(
    [string]$Executable,
    [string]$RuntimeDirectory,
    [string]$MediaPath,
    [string]$OutputDirectory,
    [ValidateRange(3,20)][int]$Runs = 7
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
if (!$Executable) { $Executable = Join-Path $repo 'target/release/video-player.exe' }
if (!$RuntimeDirectory) { $RuntimeDirectory = Join-Path $repo 'runtime' }
if (!$MediaPath) { $MediaPath = Join-Path $repo 'test-media/performance/h264-1080p30.mp4' }
if (!$OutputDirectory) { $OutputDirectory = Join-Path $repo 'test-media/performance/results' }
$Executable = [IO.Path]::GetFullPath($Executable)
$RuntimeDirectory = [IO.Path]::GetFullPath($RuntimeDirectory)
$MediaPath = [IO.Path]::GetFullPath($MediaPath)
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
if (!(Test-Path -LiteralPath $Executable -PathType Leaf)) { throw 'Build the release player before measuring' }
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
if (!(Test-Path -LiteralPath $MediaPath -PathType Leaf)) {
    New-Item -ItemType Directory -Force -Path (Split-Path $MediaPath -Parent) | Out-Null
    $fixtureUrl = 'https://github.com/serhatkochan/video-player/releases/download/v0.1.0/benchmark-h264-1080p30.mp4'
    $fixtureHash = 'cfb288446f67e89279ed49156df893d667ca4b3099c403e18cf6ecd37912ef8a'
    $download = $MediaPath + '.download'
    Invoke-WebRequest -UseBasicParsing -Uri $fixtureUrl -OutFile $download -TimeoutSec 180
    if ((Get-FileHash -LiteralPath $download -Algorithm SHA256).Hash.ToLowerInvariant() -ne $fixtureHash) { throw 'Benchmark fixture checksum mismatch' }
    Move-Item -LiteralPath $download -Destination $MediaPath
}

function Get-Summary([double[]]$Values) {
    $ordered = @($Values | Sort-Object)
    if (!$ordered.Count) { throw 'No samples to summarize' }
    $middle = [int][math]::Floor($ordered.Count / 2)
    $median = if ($ordered.Count % 2) { $ordered[$middle] } else { ($ordered[$middle - 1] + $ordered[$middle]) / 2 }
    [ordered]@{ count=$ordered.Count; median=[math]::Round($median,4); min=[math]::Round($ordered[0],4); max=[math]::Round($ordered[-1],4) }
}

function Measure-Player([string]$Scenario, [int]$Trial) {
    $smokeFile = Join-Path $OutputDirectory ('smoke-{0}-{1}-{2}.json' -f $Scenario,$Trial,[Guid]::NewGuid().ToString('N'))
    $arguments = @('"--smoke-report=' + $smokeFile + '"')
    if ($Scenario -eq 'playback') {
        $freshMedia = Join-Path (Split-Path $MediaPath -Parent) ('performance-' + [Guid]::NewGuid().ToString('N') + [IO.Path]::GetExtension($MediaPath))
        New-Item -ItemType HardLink -Path $freshMedia -Target $MediaPath | Out-Null
        $arguments += '"' + $freshMedia + '"'
    }
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $process = Start-Process -FilePath $Executable -ArgumentList $arguments -PassThru -WindowStyle Normal
    $windowMs = $null
    $cpuStart = $null
    $sampleStart = 0.0
    $cpuPercent = $null
    $workingSet = New-Object 'System.Collections.Generic.List[double]'
    $privateBytes = New-Object 'System.Collections.Generic.List[double]'
    try {
        while (!$process.HasExited -and $clock.Elapsed.TotalSeconds -lt 15) {
            $process.Refresh()
            $elapsed = $clock.Elapsed.TotalSeconds
            if ($null -eq $windowMs -and $process.MainWindowHandle -ne [IntPtr]::Zero) { $windowMs = $clock.Elapsed.TotalMilliseconds }
            if ($elapsed -ge 2 -and $null -eq $cpuStart) {
                $cpuStart = $process.TotalProcessorTime.TotalSeconds
                $sampleStart = $elapsed
            }
            if ($null -ne $cpuStart -and $elapsed -le 5) {
                $workingSet.Add($process.WorkingSet64 / 1MB)
                $privateBytes.Add($process.PrivateMemorySize64 / 1MB)
            }
            if ($elapsed -ge 5 -and $null -eq $cpuPercent -and $null -ne $cpuStart) {
                $cpuPercent = 100 * ($process.TotalProcessorTime.TotalSeconds - $cpuStart) / ($elapsed - $sampleStart) / [Environment]::ProcessorCount
            }
            Start-Sleep -Milliseconds $(if ($null -eq $windowMs) { 5 } else { 50 })
        }
        if (!$process.HasExited) { throw ("The visible player smoke mode did not self-close: scenario={0}, trial={1}, elapsed_seconds={2:N3}, window_created_ms={3}, cpu_machine_percent={4}, memory_samples={5}" -f $Scenario,$Trial,$clock.Elapsed.TotalSeconds,$windowMs,$cpuPercent,$workingSet.Count) }
        if ($process.ExitCode -ne 0 -or !(Test-Path -LiteralPath $smokeFile)) { throw 'Player smoke mode failed' }
        $smoke = Get-Content -LiteralPath $smokeFile -Raw | ConvertFrom-Json
        if (!$smoke.engine_started -or !$smoke.native_surface -or $smoke.error) { throw 'Player engine or native surface failed' }
        if ($Scenario -eq 'playback' -and ($smoke.position -lt 1 -or !$smoke.codec)) { throw 'No real playback progress in resource measurement' }
        if ($null -eq $windowMs -or $null -eq $cpuPercent -or $workingSet.Count -eq 0) { throw 'Incomplete window/resource measurement' }
        [ordered]@{
            trial=$Trial; window_created_ms=[math]::Round($windowMs,4)
            cpu_machine_percent=[math]::Round($cpuPercent,4)
            working_set_mib=[math]::Round(($workingSet | Measure-Object -Average).Average,4)
            private_memory_mib=[math]::Round(($privateBytes | Measure-Object -Average).Average,4)
            sample_count=$workingSet.Count; codec=$smoke.codec; hwdec=$smoke.hwdec
        }
    } finally {
        if (!$process.HasExited) { $process.Kill(); $process.WaitForExit() }
        $process.Dispose()
    }
}

$scenarios = [ordered]@{}
foreach ($scenario in @('idle','playback')) {
    Write-Host ('Measuring {0}: one discarded warmup and {1} trials' -f $scenario,$Runs)
    $warmup = Measure-Player $scenario 0
    $trials = @(1..$Runs | ForEach-Object { Measure-Player $scenario $_ })
    $scenarios[$scenario] = [ordered]@{
        warmup=$warmup; trials=$trials
        summary=[ordered]@{
            window_created_ms=Get-Summary @($trials | ForEach-Object { $_.window_created_ms })
            cpu_machine_percent=Get-Summary @($trials | ForEach-Object { $_.cpu_machine_percent })
            working_set_mib=Get-Summary @($trials | ForEach-Object { $_.working_set_mib })
            private_memory_mib=Get-Summary @($trials | ForEach-Object { $_.private_memory_mib })
        }
    }
}

Push-Location $repo
try {
    $nativeOutput = Join-Path $OutputDirectory 'native.json'
    & cargo run -p video-player --example performance_probe --release --locked -- $MediaPath $RuntimeDirectory $nativeOutput $Runs
    if ($LASTEXITCODE -ne 0) { throw 'Native engine performance probe failed' }
    $native = Get-Content -LiteralPath $nativeOutput -Raw | ConvertFrom-Json
} finally { Pop-Location }
$os = Get-CimInstance Win32_OperatingSystem
$report = [ordered]@{
    schema_version=1; measured_at_utc=[DateTime]::UtcNow.ToString('o')
    player_sha256=(Get-FileHash -LiteralPath $Executable -Algorithm SHA256).Hash.ToLowerInvariant()
    runtime_sha256=[ordered]@{
        libmpv=(Get-FileHash -LiteralPath (Join-Path $RuntimeDirectory 'libmpv-2.dll') -Algorithm SHA256).Hash.ToLowerInvariant()
        ffmpeg=(Get-FileHash -LiteralPath (Join-Path $RuntimeDirectory 'ffmpeg.exe') -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    system=[ordered]@{
        os=$os.Caption; build=$os.BuildNumber; logical_processors=[Environment]::ProcessorCount
        cpu=@(Get-CimInstance Win32_Processor | Select-Object Name,NumberOfCores,NumberOfLogicalProcessors)
        gpu=@(Get-CimInstance Win32_VideoController | Select-Object Name,DriverVersion)
        physical_memory_gib=[math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory / 1GB,2)
    }
    media=[ordered]@{
        file=(Split-Path $MediaPath -Leaf); sha256=(Get-FileHash -LiteralPath $MediaPath -Algorithm SHA256).Hash.ToLowerInvariant()
        bytes=(Get-Item -LiteralPath $MediaPath).Length
    }
    conditions=[ordered]@{
        warmup_discarded_per_scenario=1; measured_trials=$Runs; cache='warm OS and file cache'
        resource_window_seconds=@(2,5); cpu_normalization='process CPU seconds / elapsed seconds / logical processors * 100'
        memory='process working set and private bytes; excludes GPU VRAM'
        window_metric='process launch until MainWindowHandle becomes nonzero; not first painted frame'
        gui_launch='Normal visible GUI; Hidden can throttle redraw/event delivery and delay the smoke timer beyond its expected six seconds'
    }
    gui=$scenarios; native=$native
}
$report | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'performance.json') -Encoding UTF8
$scenarios | ConvertTo-Json -Depth 8
Write-Host 'Detailed benchmark evidence saved to performance.json.'
