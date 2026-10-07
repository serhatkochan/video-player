[CmdletBinding()]
param(
    [string]$Executable,
    [string]$VlcDirectory = 'C:\Program Files\VideoLAN\VLC',
    [string]$MpvExecutable,
    [string]$MediaPath,
    [string]$OutputDirectory,
    [ValidateRange(3,20)][int]$Runs = 7
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
if (!$Executable) { $Executable = Join-Path $repo 'dist/stage/video-player.exe' }
if (!$MediaPath) { $MediaPath = Join-Path $repo 'test-media/performance/h264-1080p30.mp4' }
if (!$OutputDirectory) { $OutputDirectory = Join-Path $repo 'test-media/performance/comparison' }
foreach ($name in @('Executable','VlcDirectory','MediaPath','OutputDirectory')) {
    Set-Variable -Name $name -Value ([IO.Path]::GetFullPath((Get-Variable -Name $name -ValueOnly)))
}
foreach ($file in @($Executable,$MediaPath,(Join-Path $VlcDirectory 'vlc.exe'))) {
    if (!(Test-Path -LiteralPath $file -PathType Leaf)) { throw 'A player executable or the synthetic media file is missing' }
}
if ($MpvExecutable) {
    $MpvExecutable = [IO.Path]::GetFullPath($MpvExecutable)
    if (!(Test-Path -LiteralPath $MpvExecutable -PathType Leaf)) { throw 'Optional mpv executable does not exist' }
}
[IO.Directory]::CreateDirectory($OutputDirectory) | Out-Null
$vlcCopy = Join-Path $OutputDirectory ('vlc-' + [Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($vlcCopy) | Out-Null
Get-ChildItem -LiteralPath $VlcDirectory -Force | Where-Object { $_.Name -ne 'portable' } | ForEach-Object {
    Copy-Item -LiteralPath $_.FullName -Destination $vlcCopy -Recurse
}
[IO.Directory]::CreateDirectory((Join-Path $vlcCopy 'portable')) | Out-Null
$vlcExe = Join-Path $vlcCopy 'vlc.exe'
$originalHash = (Get-FileHash -LiteralPath (Join-Path $VlcDirectory 'vlc.exe') -Algorithm SHA256).Hash
if ((Get-FileHash -LiteralPath $vlcExe -Algorithm SHA256).Hash -ne $originalHash) { throw 'VLC portable copy changed the executable' }

function Get-Summary([double[]]$Values) {
    $sorted = @($Values | Sort-Object)
    if (!$sorted.Count) { throw 'No benchmark samples' }
    $middle = [int][math]::Floor($sorted.Count / 2)
    $median = if ($sorted.Count % 2) { $sorted[$middle] } else { ($sorted[$middle - 1] + $sorted[$middle]) / 2 }
    [ordered]@{ count=$sorted.Count; median=[math]::Round($median,4); min=[math]::Round($sorted[0],4); max=[math]::Round($sorted[-1],4) }
}

function Get-FreePort {
    $listener = New-Object Net.Sockets.TcpListener([Net.IPAddress]::Loopback,0)
    try { $listener.Start(); $listener.LocalEndpoint.Port } finally { $listener.Stop() }
}

function Read-RcReply([Net.Sockets.TcpClient]$Client) {
    $stream = $Client.GetStream()
    $buffer = New-Object byte[] 4096
    $result = New-Object Text.StringBuilder
    $timer = [Diagnostics.Stopwatch]::StartNew()
    $lastRead = 0.0
    while ($timer.Elapsed.TotalMilliseconds -lt 1500) {
        if ($stream.DataAvailable) {
            $count = $stream.Read($buffer,0,$buffer.Length)
            if ($count -eq 0) { throw 'VLC control socket closed unexpectedly' }
            [void]$result.Append([Text.Encoding]::UTF8.GetString($buffer,0,$count))
            $lastRead = $timer.Elapsed.TotalMilliseconds
        } elseif ($result.Length -gt 0 -and $timer.Elapsed.TotalMilliseconds - $lastRead -ge 50) { break }
        Start-Sleep -Milliseconds 5
    }
    $result.ToString()
}

function Send-Rc([Net.Sockets.TcpClient]$Client,[string]$Command) {
    $bytes = [Text.Encoding]::UTF8.GetBytes($Command + [char]10)
    $Client.GetStream().Write($bytes,0,$bytes.Length)
    Read-RcReply $Client
}

function Get-RcNumber([string]$Reply) {
    $values = [regex]::Matches($Reply,'(?m)^\s*>?\s*(\d+)\s*$')
    if (!$values.Count) { throw 'VLC did not return the requested numeric playback counter' }
    [int]$values[$values.Count - 1].Groups[1].Value
}

function New-TrialMedia {
    $alias = Join-Path $OutputDirectory ('synthetic-' + [Guid]::NewGuid().ToString('N') + [IO.Path]::GetExtension($MediaPath))
    try { New-Item -ItemType HardLink -Path $alias -Target $MediaPath -ErrorAction Stop | Out-Null }
    catch { Copy-Item -LiteralPath $MediaPath -Destination $alias }
    if ((Get-FileHash -LiteralPath $alias -Algorithm SHA256).Hash -ne (Get-FileHash -LiteralPath $MediaPath -Algorithm SHA256).Hash) { throw 'Trial media alias changed the fixture bytes' }
    $alias
}

function Send-Mpv([IO.StreamReader]$Reader,[IO.StreamWriter]$Writer,[object[]]$Command,[int]$RequestId) {
    $request = @{ command=$Command; request_id=$RequestId } | ConvertTo-Json -Compress -Depth 5
    $Writer.WriteLine($request)
    $deadline = [Diagnostics.Stopwatch]::StartNew()
    while ($deadline.Elapsed.TotalMilliseconds -lt 2000) {
        $read = $Reader.ReadLineAsync()
        if (!$read.Wait([math]::Max(1,2000 - [int]$deadline.Elapsed.TotalMilliseconds))) { throw 'mpv IPC response timed out' }
        if ($null -eq $read.Result) { throw 'mpv IPC pipe closed unexpectedly' }
        $reply = $read.Result | ConvertFrom-Json
        if ($reply.request_id -eq $RequestId) {
            if ($reply.error -ne 'success') { throw ('mpv IPC command failed: ' + $reply.error) }
            return $reply.data
        }
    }
    throw 'mpv IPC did not acknowledge this request'
}

function Measure-Trial([string]$Player,[string]$Scenario,[int]$Trial,[string]$TrialMedia) {
    $mediaEvidence = if ($Scenario -eq 'playback') {
        [ordered]@{ file=(Split-Path $TrialMedia -Leaf); bytes=(Get-Item -LiteralPath $TrialMedia).Length; sha256=(Get-FileHash -LiteralPath $TrialMedia -Algorithm SHA256).Hash.ToLowerInvariant() }
    } else { $null }
    $smokeFile = Join-Path $OutputDirectory ('smoke-{0}-{1}-{2}.json' -f $Scenario,$Trial,[Guid]::NewGuid().ToString('N'))
    $logFile = Join-Path $OutputDirectory ('vlc-{0}-{1}-{2}.log' -f $Scenario,$Trial,[Guid]::NewGuid().ToString('N'))
    $client = $null
    $pipe = $null
    $reader = $null
    $writer = $null
    $pipeName = 'video-player-benchmark-' + [Guid]::NewGuid().ToString('N')
    $port = 0
    if ($Player -eq 'video_player') {
        $program = $Executable
        $arguments = @('"--smoke-report=' + $smokeFile + '"')
    } elseif ($Player -eq 'mpv') {
        $program = $MpvExecutable
        $arguments = @('--no-config','--load-scripts=no','--idle=yes','--force-window=immediate','--keep-open=yes',
            '--hwdec=auto-safe','--vo=gpu-next','--gpu-api=d3d11','--gpu-context=d3d11','--autofit=1100x720',
            '--no-fullscreen','--no-save-position-on-quit','--msg-level=all=warn',('--input-ipc-server=\\.\pipe\' + $pipeName))
    } else {
        $program = $vlcExe
        $port = Get-FreePort
        $arguments = @('--ignore-config','--no-one-instance','--no-one-instance-when-started-from-file',
            '--intf=qt','--extraintf=oldrc','--rc-quiet',('--rc-host=127.0.0.1:' + $port),
            '--no-qt-start-minimized','--no-qt-privacy-ask','--no-qt-updates-notif','--no-qt-recentplay','--qt-continue=0',
            '--no-metadata-network-access','--no-media-library','--no-qt-video-autoresize','--no-fullscreen')
        if ($Scenario -eq 'playback') { $arguments += @('--play-and-exit','--stop-time=10') }
        if ($Trial -eq 0 -and $Scenario -eq 'playback') {
            $arguments += @('--verbose=2','--file-logging','--log-verbose=2',('"--logfile=' + $logFile + '"'))
        } else { $arguments += '--verbose=0' }
    }
    if ($Scenario -eq 'playback') { $arguments += '"' + $TrialMedia + '"' }
    $timer = [Diagnostics.Stopwatch]::StartNew()
    $process = Start-Process -FilePath $program -ArgumentList $arguments -PassThru -WindowStyle Normal
    $windowMs = $null
    $cpuStart = $null
    $sampleStart = 0.0
    $sampleDuration = 0.0
    $cpuPercent = $null
    $workingSet = New-Object 'System.Collections.Generic.List[double]'
    $privateBytes = New-Object 'System.Collections.Generic.List[double]'
    try {
        while (!$process.HasExited -and $timer.Elapsed.TotalSeconds -lt 5.1) {
            $process.Refresh()
            $elapsed = $timer.Elapsed.TotalSeconds
            if ($null -eq $windowMs -and $process.MainWindowHandle -ne [IntPtr]::Zero) { $windowMs = $timer.Elapsed.TotalMilliseconds }
            if ($elapsed -ge 2 -and $null -eq $cpuStart) { $cpuStart = $process.TotalProcessorTime.TotalSeconds; $sampleStart = $elapsed }
            if ($null -ne $cpuStart -and $elapsed -le 5) {
                $workingSet.Add($process.WorkingSet64 / 1MB)
                $privateBytes.Add($process.PrivateMemorySize64 / 1MB)
            }
            if ($elapsed -ge 5 -and $null -eq $cpuPercent -and $null -ne $cpuStart) {
                $sampleDuration = $elapsed - $sampleStart
                $cpuPercent = 100 * ($process.TotalProcessorTime.TotalSeconds - $cpuStart) / $sampleDuration / [Environment]::ProcessorCount
            }
            Start-Sleep -Milliseconds $(if ($null -eq $windowMs) { 5 } else { 50 })
        }
        if ($process.HasExited -or $null -eq $windowMs -or $null -eq $cpuPercent -or !$workingSet.Count) {
            throw ('Incomplete measurement: player={0}, scenario={1}, trial={2}, exited={3}, elapsed={4:F3}, window_ms={5}, cpu_percent={6}, memory_samples={7}' -f $Player,$Scenario,$Trial,$process.HasExited,$elapsed,$windowMs,$cpuPercent,$workingSet.Count)
        }
        $proof = [ordered]@{}
        if ($Player -eq 'video_player') {
            if (!$process.WaitForExit(10000)) { throw 'Video Player smoke mode did not self-close' }
            $process.Refresh()
            if ($process.ExitCode -ne 0 -or !(Test-Path -LiteralPath $smokeFile)) { throw 'Video Player smoke mode failed' }
            $smoke = Get-Content -LiteralPath $smokeFile -Raw | ConvertFrom-Json
            if (!$smoke.engine_started -or !$smoke.native_surface -or $smoke.error) { throw 'Video Player engine or surface failed' }
            if ($Scenario -eq 'playback' -and ($smoke.position -lt 1 -or $smoke.position -gt 8 -or !$smoke.codec)) { throw 'Video Player did not prove fresh playback from the beginning' }
            $proof = [ordered]@{ codec=$smoke.codec; hwdec=$smoke.hwdec; position_seconds=$smoke.position }
        } elseif ($Player -eq 'mpv') {
            $pipe = New-Object IO.Pipes.NamedPipeClientStream('.', $pipeName, [IO.Pipes.PipeDirection]::InOut, [IO.Pipes.PipeOptions]::Asynchronous)
            $pipe.Connect(1500)
            $utf8 = New-Object Text.UTF8Encoding($false)
            $reader = New-Object IO.StreamReader($pipe,$utf8,$false,4096,$true)
            $writer = New-Object IO.StreamWriter($pipe,$utf8,4096,$true)
            $writer.AutoFlush = $true
            $version = Send-Mpv $reader $writer @('get_property','mpv-version') 1
            $idle = Send-Mpv $reader $writer @('get_property','idle-active') 2
            if ($Scenario -eq 'playback') {
                $actualPath = Send-Mpv $reader $writer @('get_property','path') 3
                $pause = Send-Mpv $reader $writer @('get_property','pause') 4
                $position = Send-Mpv $reader $writer @('get_property','time-pos') 5
                $duration = Send-Mpv $reader $writer @('get_property','duration') 6
                $codec = Send-Mpv $reader $writer @('get_property','video-format') 7
                $hwdec = Send-Mpv $reader $writer @('get_property','hwdec-current') 8
                $speed = Send-Mpv $reader $writer @('get_property','speed') 9
                Start-Sleep -Milliseconds 200
                $nextPosition = Send-Mpv $reader $writer @('get_property','time-pos') 10
                if ($idle -or $pause -or ![string]::Equals([IO.Path]::GetFullPath($actualPath),$TrialMedia,[StringComparison]::OrdinalIgnoreCase) -or $position -lt 1 -or $position -gt 8 -or $nextPosition -le $position -or $duration -lt 10 -or $codec -ne 'h264' -or $hwdec -ne 'd3d11va' -or $speed -ne 1) { throw 'mpv IPC did not confirm fresh H.264 playback with D3D11VA and speed 1' }
                $proof = [ordered]@{ version=$version; codec=$codec; hwdec=$hwdec; position_seconds=@($position,$nextPosition); duration_seconds=$duration; speed=$speed }
            } elseif (!$idle) { throw 'mpv idle trial unexpectedly had active media' }
            else { $proof = [ordered]@{ version=$version; idle_active=$idle } }
            $writer.WriteLine('{"command":["quit"],"request_id":99}')
            if (!$process.WaitForExit(5000)) { throw 'The measured mpv instance did not quit' }
            $process.Refresh()
            if ($process.ExitCode -ne 0) { throw 'The measured mpv instance exited with an error' }
        } else {
            $client = New-Object Net.Sockets.TcpClient
            $connection = $client.ConnectAsync('127.0.0.1',$port)
            if (!$connection.Wait(1500)) { throw 'VLC loopback control interface did not become available' }
            $status = Send-Rc $client 'status'
            if ($Scenario -eq 'playback') {
                $expectedUri = ([Uri]$TrialMedia).AbsoluteUri
                if ($status -notmatch 'play state: 3' -or $status.IndexOf($expectedUri,[StringComparison]::OrdinalIgnoreCase) -lt 0) { throw 'VLC is not playing the requested file' }
                $firstTime = Get-RcNumber (Send-Rc $client 'get_time')
                $length = Get-RcNumber (Send-Rc $client 'get_length')
                $info = Send-Rc $client 'info'
                Start-Sleep -Milliseconds 1100
                $secondTime = Get-RcNumber (Send-Rc $client 'get_time')
                if ($firstTime -lt 1 -or $secondTime -le $firstTime -or $length -lt 10 -or $info -notmatch '(?i)(h264|h\.264|avc1)') { throw 'VLC playback position or video codec could not be confirmed' }
                $proof = [ordered]@{ codec='h264'; position_seconds=@($firstTime,$secondTime); duration_seconds=$length; hwdec='verified in discarded warmup only' }
            } elseif ($status -notmatch 'stop state: 5') { throw 'VLC idle trial unexpectedly had active media' }
            Send-Rc $client 'quit' | Out-Null
            if (!$process.WaitForExit(5000)) { throw 'The measured VLC instance did not quit' }
            $process.Refresh()
            if ($process.ExitCode -ne 0) { throw 'The measured VLC instance exited with an error' }
            if ($Trial -eq 0 -and $Scenario -eq 'playback') {
                $decoderLine = Select-String -LiteralPath $logFile -Pattern 'Using D3D11VA .*for hardware decoding' | Select-Object -First 1
                if (!$decoderLine) { throw 'VLC discarded warmup did not confirm D3D11VA decoding' }
                $proof.hwdec = 'd3d11va'
                $proof.hardware_decoder_log = ($decoderLine.Line -replace '^.*?(Using D3D11VA )','$1')
                $proof.hardware_decoder_log_sha256 = (Get-FileHash -LiteralPath $logFile -Algorithm SHA256).Hash.ToLowerInvariant()
            }
        }
        [ordered]@{
            trial=$Trial; window_created_ms=[math]::Round($windowMs,4); cpu_machine_percent=[math]::Round($cpuPercent,4)
            working_set_mib=[math]::Round(($workingSet | Measure-Object -Average).Average,4)
            private_memory_mib=[math]::Round(($privateBytes | Measure-Object -Average).Average,4)
            sample_count=$workingSet.Count; sample_duration_seconds=[math]::Round($sampleDuration,4); media_alias=$mediaEvidence; playback_evidence=$proof
        }
    } finally {
        if ($writer) { $writer.Dispose() }
        if ($reader) { $reader.Dispose() }
        if ($pipe) { $pipe.Dispose() }
        if ($client) { $client.Dispose() }
        if (!$process.HasExited) { $process.Kill(); $process.WaitForExit() }
        $process.Dispose()
    }
}

$players = [ordered]@{}
$playerNames = @('video_player','vlc')
if ($MpvExecutable) { $playerNames += 'mpv' }
foreach ($name in $playerNames) {
    $path = switch ($name) { 'video_player' { $Executable }; 'mpv' { $MpvExecutable }; default { $vlcExe } }
    $version = (Get-Item -LiteralPath $path).VersionInfo
    $dependencyHashes = [ordered]@{}
    $dependencies = switch ($name) { 'video_player' { @('libmpv-2.dll') }; 'mpv' { @() }; default { @('libvlc.dll','libvlccore.dll','plugins/codec/libavcodec_plugin.dll','plugins/video_output/libdirect3d11_plugin.dll') } }
    foreach ($dependency in $dependencies) {
        $library = Join-Path (Split-Path $path -Parent) $dependency
        if (Test-Path -LiteralPath $library -PathType Leaf) { $dependencyHashes[$dependency] = (Get-FileHash -LiteralPath $library -Algorithm SHA256).Hash.ToLowerInvariant() }
    }
    $players[$name] = [ordered]@{ file=(Split-Path $path -Leaf); version=$version.FileVersion; sha256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant(); dependency_sha256=$dependencyHashes; scenarios=[ordered]@{} }
}
foreach ($scenario in @('idle','playback')) {
    Write-Host ('Measuring {0}: one discarded warmup and {1} trials per player' -f $scenario,$Runs)
    $fixture = if ($scenario -eq 'playback') { New-TrialMedia } else { $MediaPath }
    foreach ($name in $players.Keys) {
        $players[$name].scenarios[$scenario] = [ordered]@{ warmup=(Measure-Trial $name $scenario 0 $fixture); trials=New-Object 'System.Collections.Generic.List[object]' }
    }
    foreach ($trial in 1..$Runs) {
        $fixture = if ($scenario -eq 'playback') { New-TrialMedia } else { $MediaPath }
        $order = @($playerNames)
        if (!($trial % 2)) { [array]::Reverse($order) }
        foreach ($name in $order) { $players[$name].scenarios[$scenario].trials.Add((Measure-Trial $name $scenario $trial $fixture)) }
    }
    foreach ($name in $players.Keys) {
        $results = $players[$name].scenarios[$scenario]
        $summary = [ordered]@{}
        foreach ($metric in @('window_created_ms','cpu_machine_percent','working_set_mib','private_memory_mib')) {
            $summary[$metric] = Get-Summary @($results.trials | ForEach-Object { $_[$metric] })
        }
        $results.summary = $summary
        if ($name -eq 'mpv') { $players[$name].version = $results.warmup.playback_evidence.version }
    }
}
$os = Get-CimInstance Win32_OperatingSystem
$report = [ordered]@{
    schema_version=1; measured_at_utc=[DateTime]::UtcNow.ToString('o')
    system=[ordered]@{ os=$os.Caption; build=$os.BuildNumber; logical_processors=[Environment]::ProcessorCount
        cpu=@(Get-CimInstance Win32_Processor | Select-Object Name,NumberOfCores,NumberOfLogicalProcessors)
        gpu=@(Get-CimInstance Win32_VideoController | Select-Object Name,DriverVersion)
        physical_memory_gib=[math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory / 1GB,2) }
    media=[ordered]@{ file=(Split-Path $MediaPath -Leaf); bytes=(Get-Item -LiteralPath $MediaPath).Length; sha256=(Get-FileHash -LiteralPath $MediaPath -Algorithm SHA256).Hash.ToLowerInvariant() }
    conditions=[ordered]@{ measured_trials=$Runs; discarded_warmups_per_scenario=1; cache='warm OS and file cache; not flushed'; trial_order='alternating player order'
        resource_window_seconds=@(2,5); cpu_normalization='process CPU seconds / elapsed seconds / all logical processors * 100'
        memory='process working set and private bytes; excludes GPU VRAM'; window_metric='launch until MainWindowHandle becomes nonzero; not first painted UI or video frame'
        resume='fresh byte-identical media alias for each playback trial; same alias passed to all measured players; benchmark aliases may be added to Video Player local history'
        ui='windowed real GUI; different interface sizes and layouts; optional mpv autofit=1100x720'
        vlc_config='workspace portable copy; ignores VLC core configuration; personal profile untouched'
        mpv_config='optional standalone player; no user configuration or external scripts; gpu-next D3D11 context forced, hardware decoding auto-safe; these are test settings rather than its defaults'
        instrumentation='Video Player smoke mode; VLC loopback RC and optional mpv named pipe remain loaded but are queried after resource sampling; VLC file logger enabled only in discarded playback warmup'
        hardware_validation='Video Player smoke and optional mpv hwdec-current report every trial; VLC D3D11VA log checked in discarded playback warmup, same decoder settings used in measured trials'
        scope='CPU and memory comparison on one computer and one media file; does not compare first frame, seek latency, visual quality, HDR, 4K or 8K' }
    sources=@('https://github.com/videolan/vlc/blob/3.0.23/src/win32/dirs.c','https://github.com/videolan/vlc/blob/3.0.23/modules/control/oldrc.c','https://github.com/videolan/vlc/blob/3.0.23/modules/codec/avcodec/video.c','https://mpv.io/manual/master/#json-ipc','https://mpv.io/installation/')
    players=$players
}
$output = Join-Path $OutputDirectory 'comparison.json'
[IO.File]::WriteAllText($output,($report | ConvertTo-Json -Depth 15), (New-Object Text.UTF8Encoding($false)))
$summaries = [ordered]@{}
foreach ($name in $players.Keys) {
    $summaries[$name] = [ordered]@{ version=$players[$name].version; idle=$players[$name].scenarios.idle.summary; playback=$players[$name].scenarios.playback.summary }
}
$summaries | ConvertTo-Json -Depth 8
Write-Host 'Detailed evidence saved to comparison.json. Only processes started by this script were closed.'
