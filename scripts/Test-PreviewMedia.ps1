[CmdletBinding()]
param(
    [string]$FixtureEncoder,
    [string]$SourceMediaPath,
    [string]$FixtureDirectory,
    [string]$RuntimeDirectory,
    [string]$MediaProbe,
    [string]$ThumbnailProbe,
    [string]$StallWorker,
    [string]$ReportPath,
    [switch]$GenerateOnly,
    [switch]$ValidateOnly
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
if ($GenerateOnly -and $ValidateOnly) { throw 'Choose GenerateOnly or ValidateOnly, not both.' }
if (!$FixtureEncoder) { $FixtureEncoder = Join-Path $repo 'runtime/ffmpeg.exe' }
if (!$SourceMediaPath) { $SourceMediaPath = Join-Path $repo 'test-media/performance/h264-1080p30.mp4' }
if (!$FixtureDirectory) { $FixtureDirectory = Join-Path $repo 'test-media/release-preview' }
if (!$MediaProbe) { $MediaProbe = Join-Path $repo 'target/release/examples/media_probe.exe' }
if (!$ThumbnailProbe) { $ThumbnailProbe = Join-Path $repo 'target/release/examples/thumbnail_probe.exe' }
if (!$StallWorker) { $StallWorker = Join-Path $repo 'target/release/examples/thumbnail_stall_worker.exe' }
$FixtureDirectory = [IO.Path]::GetFullPath($FixtureDirectory)
$privateRoot = [IO.Path]::GetFullPath((Join-Path $repo 'test-media')) + [IO.Path]::DirectorySeparatorChar
if (!$FixtureDirectory.StartsWith($privateRoot, [StringComparison]::OrdinalIgnoreCase)) { throw 'FixtureDirectory must be inside this repository test-media directory.' }
if (!$ReportPath) { $ReportPath = Join-Path $FixtureDirectory 'preview-validation.json' }
if (!$GenerateOnly -and !$RuntimeDirectory) { throw 'Pass the explicit candidate RuntimeDirectory for validation, or use GenerateOnly.' }
New-Item -ItemType Directory -Force -Path $FixtureDirectory | Out-Null
$logs = Join-Path $FixtureDirectory 'logs'
New-Item -ItemType Directory -Force -Path $logs | Out-Null
$utf8 = New-Object Text.UTF8Encoding($false)
$manifestPath = Join-Path $FixtureDirectory 'fixture-manifest.json'

function Write-Json([string]$Path, $Value) {
    [IO.File]::WriteAllText($Path, ($Value | ConvertTo-Json -Depth 16), $utf8)
}
function Get-Hash([string]$Path) { (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() }
function Get-Artifact([string]$Path) {
    $item = Get-Item -LiteralPath $Path
    [ordered]@{ filename=$item.Name; bytes=$item.Length; sha256=(Get-Hash $Path) }
}
function Quote-NativeArgument([string]$Value) {
    '"' + (($Value -replace '(\\*)"', '$1$1\"') -replace '(\\+)$', '$1$1') + '"'
}
function Invoke-Probe([string]$Executable, [string[]]$Arguments, [string]$LogName, [int]$TimeoutSeconds = 35) {
    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = [IO.Path]::GetFullPath($Executable)
    $info.Arguments = (($Arguments | ForEach-Object { Quote-NativeArgument $_ }) -join ' ')
    $info.WorkingDirectory = $FixtureDirectory
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.StandardOutputEncoding = $utf8
    $info.StandardErrorEncoding = $utf8
    $process = New-Object Diagnostics.Process
    $process.StartInfo = $info
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $started = $false
    try {
        if (!$process.Start()) { throw 'The probe could not be started.' }
        $started = $true
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        $timedOut = !$process.WaitForExit($TimeoutSeconds * 1000)
        if ($timedOut) { $process.Kill() }
        $process.WaitForExit()
        $stdout = $stdoutTask.GetAwaiter().GetResult()
        $stderr = $stderrTask.GetAwaiter().GetResult()
        [IO.File]::WriteAllText((Join-Path $logs ($LogName + '.stdout.txt')), $stdout, $utf8)
        [IO.File]::WriteAllText((Join-Path $logs ($LogName + '.stderr.txt')), $stderr, $utf8)
        [ordered]@{ exit_code=$process.ExitCode; timed_out=$timedOut; elapsed_seconds=[math]::Round($clock.Elapsed.TotalSeconds,3); stdout=$stdout; stderr=$stderr }
    } finally {
        if ($started -and !$process.HasExited) { $process.Kill(); $process.WaitForExit() }
        $process.Dispose()
    }
}
function Get-Streams([string]$Text) {
    $inputBlock = ($Text -split 'Stream mapping:|Output #')[0]
    @([regex]::Matches($inputBlock, '(?m)^\s*Stream #\d+:\d+[^\r\n]*?: (Video|Audio|Subtitle): ([^,\s]+)([^\r\n]*)') | ForEach-Object {
        [ordered]@{ type=$_.Groups[1].Value.ToLowerInvariant(); codec=$_.Groups[2].Value; description=$_.Groups[3].Value.Trim() }
    })
}
function Assert-Streams($Case, $Streams) {
    $videos = @($Streams | Where-Object { $_.type -eq 'video' })
    $audios = @($Streams | Where-Object { $_.type -eq 'audio' })
    $subs = @($Streams | Where-Object { $_.type -eq 'subtitle' })
    if ($Case.video_codec -and ($videos.Count -ne 1 -or $videos[0].codec -ne $Case.video_codec)) { throw "Unexpected video codec for $($Case.id)." }
    if ($Case.bit_depth -eq 10 -and $videos[0].description -notmatch '(?:p010|yuv\w+p10|Main 10)') { throw "10-bit stream metadata missing for $($Case.id)." }
    foreach ($codec in $Case.audio_codecs) { if ($codec -notin @($audios | ForEach-Object { $_.codec })) { throw "Audio codec $codec missing for $($Case.id)." } }
    if ($Case.audio_tracks -and $audios.Count -ne $Case.audio_tracks) { throw "Unexpected audio track count for $($Case.id)." }
    if ($Case.subtitle_codec -and $Case.subtitle_codec -notin @($subs | ForEach-Object { $_.codec })) { throw "Embedded subtitle missing for $($Case.id)." }
}
function Get-PlaybackEvidence([string]$Text) {
    $snapshots = @([regex]::Matches($Text, '(?m)^position=([\d.]+) duration=([\d.]+) codec=(\S*) hwdec=(\S*) output=(\S*) tracks=(\d+) idle=(\S+) eof=(\S+)') | ForEach-Object {
        [ordered]@{ position_seconds=[double]::Parse($_.Groups[1].Value,[Globalization.CultureInfo]::InvariantCulture); duration_seconds=[double]::Parse($_.Groups[2].Value,[Globalization.CultureInfo]::InvariantCulture); codec=$_.Groups[3].Value; hwdec=$_.Groups[4].Value; output=$_.Groups[5].Value; tracks=[int]$_.Groups[6].Value }
    })
    $tracks = @([regex]::Matches($Text, '(?m)^track id=(\d+) type=(\S+) lang=(\S*) title=(.*?) selected=(true|false)\r?$') | ForEach-Object { [ordered]@{ id=[int]$_.Groups[1].Value; type=$_.Groups[2].Value; language=$_.Groups[3].Value; title=$_.Groups[4].Value; selected=$_.Groups[5].Value -eq 'true' } })
    $switches = @([regex]::Matches($Text, '(?m)^track-switch type=(\S+) from=(\d+) to=(\d+) selected=true\r?$') | ForEach-Object { [ordered]@{ type=$_.Groups[1].Value; from=[int]$_.Groups[2].Value; to=[int]$_.Groups[3].Value; selected=$true } })
    $subtitles = @([regex]::Matches($Text, '(?m)^subtitle-added id=(\d+) selected=true filename=([^\r\n]+)\r?$') | ForEach-Object { [ordered]@{ id=[int]$_.Groups[1].Value; selected=$true; filename=$_.Groups[2].Value } })
    [ordered]@{ snapshots=$snapshots; tracks=$tracks; track_switches=$switches; subtitles_added=$subtitles }
}

if (!$ValidateOnly) {
    $FixtureEncoder = [IO.Path]::GetFullPath($FixtureEncoder)
    $SourceMediaPath = [IO.Path]::GetFullPath($SourceMediaPath)
    $sourceHash = 'cfb288446f67e89279ed49156df893d667ca4b3099c403e18cf6ecd37912ef8a'
    if ((Get-Hash $SourceMediaPath) -ne $sourceHash) { throw 'SourceMediaPath must be the known synthetic 1080p30 performance fixture.' }
    $version = Invoke-Probe $FixtureEncoder @('-version') 'fixture-encoder-version' 10
    if ($version.exit_code -ne 0 -or $version.timed_out) { throw 'Fixture encoder version query failed.' }
    $cases = New-Object 'System.Collections.Generic.List[object]'
    function Add-Fixture([string]$Id, [string]$Filename, [string[]]$Arguments, [string]$VideoCodec, [string[]]$AudioCodecs, [int]$BitDepth = 8, [string]$SubtitleCodec = '', [int]$AudioTracks = 1) {
        $path = Join-Path $FixtureDirectory $Filename
        $result = Invoke-Probe $FixtureEncoder (@('-hide_banner','-nostdin','-loglevel','info','-y') + $Arguments + @($path)) ('generate-' + $Id) 90
        if ($result.exit_code -ne 0 -or $result.timed_out) { throw "Fixture generation failed: $Id; see its local stderr log." }
        $metadata = Invoke-Probe $FixtureEncoder @('-hide_banner','-nostdin','-i',$path) ('inspect-' + $Id) 10
        $case = [ordered]@{ id=$Id; filename=$Filename; role='media'; video_codec=$VideoCodec; audio_codecs=@($AudioCodecs); bit_depth=$BitDepth; subtitle_codec=$SubtitleCodec; audio_tracks=$AudioTracks; generation_exit_code=$result.exit_code; artifact=(Get-Artifact $path); streams=@(Get-Streams $metadata.stderr) }
        Assert-Streams $case $case.streams
        $cases.Add($case)
    }
    $turkish = 'T' + [char]0x00FC + 'rk' + [char]0x00E7 + 'e-' + [char]0x011F + [char]0x0131 + [char]0x015F + '-' + [char]0x015E + [char]0x0130
    foreach ($extension in @('mp4','m4v','mkv','mov')) {
        Add-Fixture ('h264-' + $extension) ($turkish + '-H264.' + $extension) @('-i',$SourceMediaPath,'-t','4','-map','0:v:0','-map','0:a:0','-c','copy') 'h264' @('aac')
    }
    $h264 = Join-Path $FixtureDirectory ($turkish + '-H264.mp4')
    $smallInputs = @('-f','lavfi','-i','testsrc2=size=320x180:rate=24','-f','lavfi','-i','sine=frequency=440:sample_rate=48000','-t','4')
    Add-Fixture 'hevc-10bit' 'HEVC-10bit.mkv' ($smallInputs + @('-c:v','hevc_nvenc','-preset','p1','-profile:v','main10','-pix_fmt','p010le','-b:v','700k','-c:a','aac')) 'hevc' @('aac') 10
    Add-Fixture 'av1-10bit' 'AV1-10bit.mkv' ($smallInputs + @('-c:v','av1_nvenc','-preset','p1','-pix_fmt','p010le','-b:v','700k','-c:a','aac')) 'av1' @('aac') 10
    Add-Fixture 'av1-webm' 'AV1-Opus.webm' @('-i',(Join-Path $FixtureDirectory 'AV1-10bit.mkv'),'-c:v','copy','-c:a','libopus') 'av1' @('opus') 10
    Add-Fixture 'vp8' 'VP8-Vorbis.webm' ($smallInputs + @('-c:v','libvpx','-deadline','realtime','-cpu-used','8','-b:v','400k','-c:a','libvorbis')) 'vp8' @('vorbis')
    Add-Fixture 'vp9' 'VP9-Opus.webm' ($smallInputs + @('-c:v','libvpx-vp9','-deadline','realtime','-cpu-used','8','-b:v','400k','-c:a','libopus')) 'vp9' @('opus')
    Add-Fixture 'mpeg2' 'MPEG2.mkv' ($smallInputs + @('-c:v','mpeg2video','-b:v','700k','-c:a','aac')) 'mpeg2video' @('aac')
    Add-Fixture 'mpeg4-part2' 'MPEG4-Part2.avi' ($smallInputs + @('-c:v','mpeg4','-q:v','5','-c:a','libmp3lame')) 'mpeg4' @('mp3')
    Add-Fixture 'wmv2' 'WMV2.wmv' ($smallInputs + @('-c:v','wmv2','-b:v','500k','-c:a','wmav2')) 'wmv2' @('wmav2')
    foreach ($audio in @(@('mp3','libmp3lame','mp3'),@('aac','aac','aac'),@('m4a','aac','aac'),@('flac','flac','flac'),@('wav','pcm_s16le','pcm_s16le'))) {
        Add-Fixture ('audio-' + $audio[0]) ('Audio.' + $audio[0]) @('-f','lavfi','-i','sine=frequency=440:sample_rate=48000','-t','4','-c:a',$audio[1]) '' @($audio[2]) 0
    }
    foreach ($audio in @(@('ac3','ac3'),@('eac3','eac3'),@('dts','dca'))) {
        Add-Fixture ('video-audio-' + $audio[0]) ('H264-' + $audio[0] + '.mkv') @('-i',$h264,'-map','0:v:0','-map','0:a:0','-c:v','copy','-c:a',$audio[1],'-strict','-2') 'h264' @($audio[0])
    }
    $captions = @{
        'srt' = "1`n00:00:00,200 --> 00:00:03,800`nSynthetic subtitle: $turkish`n"
        'vtt' = "WEBVTT`n`n00:00.200 --> 00:03.800`nSynthetic subtitle: $turkish`n"
        'ass' = "[Script Info]`nScriptType: v4.00+`nPlayResX: 320`nPlayResY: 180`n`n[V4+ Styles]`nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding`nStyle: Default,Arial,20,&H0000FFFF,&H000000FF,&H00000000,&H80000000,0,0,0,0,100,100,0,0,1,1,0,2,10,10,10,1`n`n[Events]`nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text`nDialogue: 0,0:00:00.20,0:00:03.80,Default,,0,0,0,,Synthetic subtitle: $turkish`n"
    }
    foreach ($extension in @('srt','ass','vtt')) {
        $filename = 'Subtitle.' + $extension
        [IO.File]::WriteAllText((Join-Path $FixtureDirectory $filename), $captions[$extension], $utf8)
        $cases.Add([ordered]@{ id=('external-' + $extension); filename=$filename; role='external_subtitle'; artifact=(Get-Artifact (Join-Path $FixtureDirectory $filename)) })
    }
    Add-Fixture 'two-audio-ass' 'Two-Audio-ASS.mkv' @('-i',$h264,'-f','lavfi','-i','sine=frequency=880:sample_rate=48000','-i',(Join-Path $FixtureDirectory 'Subtitle.ass'),'-t','4','-map','0:v:0','-map','0:a:0','-map','1:a:0','-map','2:s:0','-c:v','copy','-c:a','aac','-c:s','ass','-metadata:s:a:0','language=tur','-metadata:s:a:1','language=eng','-metadata:s:s:0','language=tur') 'h264' @('aac') 8 'ass' 2
    Add-Fixture 'embedded-srt' 'Embedded-SRT.mkv' @('-i',$h264,'-i',(Join-Path $FixtureDirectory 'Subtitle.srt'),'-map','0','-map','1:s:0','-c','copy','-c:s','srt') 'h264' @('aac') 8 'subrip'
    Add-Fixture 'embedded-vtt' 'Embedded-VTT.webm' @('-i',(Join-Path $FixtureDirectory 'VP9-Opus.webm'),'-i',(Join-Path $FixtureDirectory 'Subtitle.vtt'),'-map','0','-map','1:s:0','-c','copy','-c:s','webvtt') 'vp9' @('opus') 8 'webvtt'
    $broken = Join-Path $FixtureDirectory 'Corrupt.mkv'
    [IO.File]::WriteAllBytes($broken, [Text.Encoding]::ASCII.GetBytes('Deliberately invalid synthetic media, not a Matroska stream.'))
    $cases.Add([ordered]@{ id='corrupt'; filename='Corrupt.mkv'; role='corrupt'; artifact=(Get-Artifact $broken) })
    $manifest = [ordered]@{ schema_version=1; generated_utc=[DateTime]::UtcNow.ToString('o'); content='Original synthetic video testsrc2, sine tones and generated captions; no user media.'; fixture_encoder=(Get-Artifact $FixtureEncoder); fixture_encoder_version=($version.stdout -split '\r?\n')[0]; source=(Get-Artifact $SourceMediaPath); cases=$cases.ToArray() }
    Write-Json $manifestPath $manifest
    Write-Host ("Generated {0} synthetic artifacts. Candidate playback and COM validation have not run." -f $cases.Count)
}
if ($GenerateOnly) { return }

$RuntimeDirectory = [IO.Path]::GetFullPath($RuntimeDirectory)
$candidateFfmpeg = Join-Path $RuntimeDirectory 'ffmpeg.exe'
$provider = Join-Path $RuntimeDirectory 'video_player_thumbnail.dll'
$worker = Join-Path $RuntimeDirectory 'thumbnail-worker.exe'
foreach ($path in @($MediaProbe,$ThumbnailProbe,$candidateFfmpeg,$provider,$worker,$manifestPath)) { if (!(Test-Path -LiteralPath $path -PathType Leaf)) { throw "Required candidate/probe file missing: $([IO.Path]::GetFileName($path))." } }
$manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
$results = New-Object 'System.Collections.Generic.List[object]'
$report = [ordered]@{
    schema_version=1; validated_utc=[DateTime]::UtcNow.ToString('o'); status='running'
    scope=@('Synthetic files on this existing Windows machine.', 'media_probe uses the application libmpv wrapper and native video surface, without the full egui shell.', 'COM thumbnails use registration-free IInitializeWithStream/IThumbnailProvider calls; this does not test Explorer registration or surrogate activation.', 'Embedded/external subtitle stream presence, new external track selection and switching to a different audio track; visual subtitle styling is not verified.')
    not_tested=@('Clean Windows 11 installation','VLC association coexistence','Installer/update/uninstall','HDR correctness and monitor transitions','4K/8K performance','External subtitle UI loading','VC-1 decoding')
    fixtures_manifest_sha256=(Get-Hash $manifestPath)
    candidate_artifacts=@(foreach ($name in @('ffmpeg.exe','libmpv-2.dll','avformat-62.dll','avcodec-62.dll','avutil-60.dll','avfilter-11.dll','swscale-9.dll','swresample-6.dll','video_player_thumbnail.dll','thumbnail-worker.exe')) { $path = Join-Path $RuntimeDirectory $name; if (Test-Path -LiteralPath $path -PathType Leaf) { Get-Artifact $path } })
    probes=@((Get-Artifact $MediaProbe),(Get-Artifact $ThumbnailProbe))
    cases=$results
}
function Add-Result($Value) { $results.Add($Value); Write-Json $ReportPath $report }
foreach ($case in $manifest.cases) {
    $file = Join-Path $FixtureDirectory $case.filename
    if ([IO.Path]::GetFileName($case.filename) -ne $case.filename -or (Get-Hash $file) -ne $case.artifact.sha256) { throw "Fixture integrity mismatch: $($case.id)." }
    if ($case.role -eq 'external_subtitle') {
        $video = $manifest.cases | Where-Object { $_.id -eq 'h264-mp4' } | Select-Object -First 1
        $play = Invoke-Probe $MediaProbe @((Join-Path $FixtureDirectory $video.filename),$RuntimeDirectory,'--subtitle',$file) ('candidate-player-' + $case.id) 30
        $evidence = Get-PlaybackEvidence $play.stdout
        $passed = !$play.timed_out -and $play.exit_code -eq 0 -and $play.stdout.Contains('PASS: external subtitle loaded and selected.') -and $play.stdout.Contains('PASS: playback advanced.') -and @($evidence.snapshots | Where-Object { $_.position_seconds -gt 0.05 -and $_.duration_seconds -gt 0 }).Count -gt 0 -and @($evidence.subtitles_added | Where-Object { $_.filename -eq $case.filename -and $_.selected }).Count -eq 1
        Add-Result ([ordered]@{ id=$case.id; test='native-external-subtitle-load'; status=$(if ($passed) { 'passed' } else { 'failed' }); command='media_probe <h264-fixture> <candidate-runtime> --subtitle <caption-fixture>'; exit_code=$play.exit_code; timed_out=$play.timed_out; elapsed_seconds=$play.elapsed_seconds; artifact=$case.artifact; video_artifact=$video.artifact; snapshots=$evidence.snapshots; tracks=$evidence.tracks; subtitles_added=$evidence.subtitles_added })
        Write-Host ("{0}: loaded and selected={1}" -f $case.id,$passed)
        continue
    }
    if ($case.role -ne 'media') { continue }
    $decode = Invoke-Probe $candidateFfmpeg @('-hide_banner','-nostdin','-loglevel','info','-i',$file,'-t','1','-map','0:v?','-map','0:a?','-f','null','NUL') ('candidate-decode-' + $case.id) 25
    $streams = @(Get-Streams $decode.stderr)
    $failure = ''
    try { Assert-Streams $case $streams } catch { $failure = $_.Exception.Message }
    $decoderPass = !$decode.timed_out -and $decode.exit_code -eq 0 -and !$failure
    Add-Result ([ordered]@{ id=$case.id; test='candidate-ffmpeg-decode'; status=$(if ($decoderPass) { 'passed' } else { 'failed' }); command='ffmpeg -i <fixture> -t 1 -map 0:v? -map 0:a? -f null NUL'; exit_code=$decode.exit_code; timed_out=$decode.timed_out; elapsed_seconds=$decode.elapsed_seconds; artifact=$case.artifact; streams=$streams; failure=$failure })
    $arguments = @($file,$RuntimeDirectory)
    if ($case.id -eq 'two-audio-ass') { $arguments += '--exercise-controls' }
    $play = Invoke-Probe $MediaProbe $arguments ('candidate-player-' + $case.id) 30
    $evidence = Get-PlaybackEvidence $play.stdout
    $progress = $evidence.snapshots
    $tracks = $evidence.tracks
    $playPass = !$play.timed_out -and $play.exit_code -eq 0 -and $play.stdout.Contains('PASS: playback advanced.') -and @($progress | Where-Object { $_.position_seconds -gt 0.05 -and $_.duration_seconds -gt 0 }).Count -gt 0
    if ($case.video_codec -and $case.video_codec -notin @($progress | ForEach-Object { $_.codec })) { $playPass = $false }
    if ($case.audio_tracks -and @($tracks | Where-Object { $_.type -eq 'audio' }).Count -ne $case.audio_tracks) { $playPass = $false }
    if ($case.subtitle_codec -and @($tracks | Where-Object { $_.type -eq 'sub' }).Count -ne 1) { $playPass = $false }
    if ($case.id -eq 'two-audio-ass' -and (!$play.stdout.Contains('PASS: pause, volume, mute, speed, subtitle delay, track selection, input routing and seek.') -or @($evidence.track_switches | Where-Object { $_.type -eq 'audio' -and $_.from -ne $_.to -and $_.selected }).Count -ne 1)) { $playPass = $false }
    Add-Result ([ordered]@{ id=$case.id; test='native-player-playback'; status=$(if ($playPass) { 'passed' } else { 'failed' }); command=$(if ($case.id -eq 'two-audio-ass') { 'media_probe <fixture> <candidate-runtime> --exercise-controls' } else { 'media_probe <fixture> <candidate-runtime>' }); exit_code=$play.exit_code; timed_out=$play.timed_out; elapsed_seconds=$play.elapsed_seconds; artifact=$case.artifact; snapshots=$progress; tracks=$tracks; track_switches=$evidence.track_switches })
    Write-Host ("{0}: decoder={1}, playback={2}" -f $case.id,$decoderPass,$playPass)
}
foreach ($id in @('h264-mp4','hevc-10bit','av1-10bit','corrupt')) {
    $case = $manifest.cases | Where-Object { $_.id -eq $id } | Select-Object -First 1
    $bitmap = Join-Path $FixtureDirectory ($id + '-thumbnail.bmp')
    $arguments = @($provider,(Join-Path $FixtureDirectory $case.filename),$bitmap,'256')
    if ($id -eq 'corrupt') { $arguments += '--expect-failure' }
    $thumb = Invoke-Probe $ThumbnailProbe $arguments ('candidate-thumbnail-' + $id) 20
    $proof = [regex]::Match($thumb.stdout, 'PASS: (\d+)x(\d+) opaque BGRA thumbnail in ([\d.]+)s')
    $thumbPass = !$thumb.timed_out -and $thumb.exit_code -eq 0
    if ($id -eq 'corrupt') { $thumbPass = $thumbPass -and $thumb.stdout.Contains('PASS: invalid media rejected') } else { $thumbPass = $thumbPass -and $proof.Success -and (Test-Path -LiteralPath $bitmap -PathType Leaf) }
    Add-Result ([ordered]@{ id=$id; test='registration-free-com-thumbnail'; status=$(if ($thumbPass) { 'passed' } else { 'failed' }); command=$(if ($id -eq 'corrupt') { 'thumbnail_probe <provider> <fixture> <output.bmp> 256 --expect-failure' } else { 'thumbnail_probe <provider> <fixture> <output.bmp> 256' }); exit_code=$thumb.exit_code; timed_out=$thumb.timed_out; elapsed_seconds=$thumb.elapsed_seconds; artifact=$case.artifact; bitmap=$(if ($proof.Success -and (Test-Path -LiteralPath $bitmap -PathType Leaf)) { Get-Artifact $bitmap } else { $null }); dimensions=$(if ($proof.Success) { @([int]$proof.Groups[1].Value,[int]$proof.Groups[2].Value) } else { $null }) })
}
if (Test-Path -LiteralPath $StallWorker -PathType Leaf) {
    $case = $manifest.cases | Where-Object { $_.id -eq 'h264-mp4' } | Select-Object -First 1
    $timeout = Invoke-Probe $ThumbnailProbe @('--timeout-worker',$StallWorker,(Join-Path $FixtureDirectory $case.filename)) 'candidate-thumbnail-stall' 10
    $passed = !$timeout.timed_out -and $timeout.exit_code -eq 0 -and $timeout.stdout.Contains('PASS: worker terminated after')
    Add-Result ([ordered]@{ id='stall-worker'; test='worker-hard-timeout'; status=$(if ($passed) { 'passed' } else { 'failed' }); command='thumbnail_probe --timeout-worker <stall-worker> <fixture>'; worker=(Get-Artifact $StallWorker); exit_code=$timeout.exit_code; timed_out=$timeout.timed_out; elapsed_seconds=$timeout.elapsed_seconds })
} else {
    Add-Result ([ordered]@{ id='stall-worker'; test='worker-hard-timeout'; status='not_tested'; reason='Build the optional thumbnail_stall_worker example to exercise job timeout and cleanup.' })
}
$failed = @($results | Where-Object { $_.status -eq 'failed' })
$report.status = if ($failed.Count) { 'failed' } else { 'passed_for_documented_scope' }
$report.completed_utc = [DateTime]::UtcNow.ToString('o')
Write-Json $ReportPath $report
if ($failed.Count) { throw ("{0} preview checks failed; see the local report and logs." -f $failed.Count) }
Write-Host ("Preview checks passed for the documented scope. {0} checks remain explicitly not tested." -f @($results | Where-Object { $_.status -eq 'not_tested' }).Count)
