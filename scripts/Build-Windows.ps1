[CmdletBinding()]
param([string]$NsisPath, [switch]$SkipCargo, [switch]$ForRelease, [string]$AcceptanceFile)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
Push-Location $repo
try {
    if (!$SkipCargo) {
        & cargo build --workspace --release --locked
        if ($LASTEXITCODE -ne 0) { throw 'Release build failed' }
        & cargo build -p video-player-thumbnail --example thumbnail_worker --release --locked
        if ($LASTEXITCODE -ne 0) { throw 'Thumbnail worker build failed' }
    }
    & (Join-Path $PSScriptRoot 'Get-Runtime.ps1')
    & (Join-Path $PSScriptRoot 'Get-RustNotices.ps1')
    & (Join-Path $PSScriptRoot 'Get-SourceSeeds.ps1')
    if (!$NsisPath) {
        & (Join-Path $PSScriptRoot 'Get-BuildTools.ps1')
        $local = Get-ChildItem (Join-Path $repo 'tools') -Recurse -Filter makensis.exe -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($local) { $NsisPath = $local.FullName } else { $NsisPath = (Get-Command makensis.exe -ErrorAction Stop).Source }
    }
    $nsisCopyright = Join-Path (Split-Path $NsisPath -Parent) 'COPYING'
    if (!(Test-Path -LiteralPath $nsisCopyright -PathType Leaf)) { throw 'NSIS COPYING notice is required alongside the compiler' }
    $versionMatch = Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"' | Select-Object -First 1
    if (!$versionMatch) { throw 'Workspace version not found' }
    $version = $versionMatch.Matches[0].Groups[1].Value
    $stage = Join-Path $repo 'dist/stage'
    if (Test-Path -LiteralPath $stage) {
        if ([IO.Path]::GetFullPath($stage) -ne [IO.Path]::GetFullPath((Join-Path $repo 'dist/stage'))) { throw 'Unsafe staging path' }
        Remove-Item -LiteralPath $stage -Recurse -Force
    }
    New-Item -ItemType Directory -Force -Path $stage | Out-Null
    foreach ($file in @('video-player.exe','video_player_thumbnail.dll')) {
        Copy-Item -LiteralPath (Join-Path $repo "target/release/$file") -Destination $stage
    }
    Copy-Item -LiteralPath (Join-Path $repo 'target/release/examples/thumbnail_worker.exe') -Destination (Join-Path $stage 'thumbnail-worker.exe')
    Get-ChildItem (Join-Path $repo 'runtime') -File | Where-Object { $_.Extension -eq '.dll' -or $_.Name -in @('ffmpeg.exe','runtime-manifest.json','ffmpeg-build-config.txt') } | Copy-Item -Destination $stage
    Copy-Item -LiteralPath (Join-Path $repo 'packaging/Windows-Integration.ps1') -Destination $stage
    Copy-Item -LiteralPath (Join-Path $repo 'packaging/VideoPlayer.nsi') -Destination (Join-Path $stage 'installer-source.nsi')
    Copy-Item -LiteralPath (Join-Path $repo 'LICENSE'), (Join-Path $repo 'THIRD_PARTY_NOTICES.md') -Destination $stage
    Copy-Item -LiteralPath (Join-Path $repo 'runtime/licenses') -Destination (Join-Path $stage 'licenses') -Recurse
    $nsisLicenses = Join-Path $stage 'licenses/nsis'
    New-Item -ItemType Directory -Force -Path $nsisLicenses | Out-Null
    Copy-Item -LiteralPath $nsisCopyright -Destination (Join-Path $nsisLicenses 'COPYING')
    if (Test-Path (Join-Path $repo 'runtime/sources')) { Copy-Item -LiteralPath (Join-Path $repo 'runtime/sources') -Destination (Join-Path $stage 'sources') -Recurse }
    if ($ForRelease) {
        & (Join-Path $PSScriptRoot 'Test-ReleaseGate.ps1') -AcceptanceFile $AcceptanceFile -Version $version -StageDirectory $stage
    }
    $hashes = Get-ChildItem -LiteralPath $stage -Recurse -File | Sort-Object FullName | ForEach-Object {
        '{0}  {1}' -f (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant(), $_.FullName.Substring($stage.Length + 1).Replace('\','/')
    }
    $bytes = [Text.Encoding]::UTF8.GetBytes(($hashes -join "`n"))
    $sha = [Security.Cryptography.SHA256]::Create()
    try { $payloadId = ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-','').ToLowerInvariant().Substring(0,12) } finally { $sha.Dispose() }
    $hashes | Set-Content -LiteralPath (Join-Path $stage 'SHA256SUMS.txt') -Encoding UTF8
    $output = Join-Path $repo "dist/Video-Player-$version-windows-x64-setup.exe"
    & $NsisPath /WX /INPUTCHARSET UTF8 "/DVERSION=$version" "/DPAYLOAD_ID=$payloadId" "/DSTAGE_DIRECTORY=$stage" "/DOUTPUT_FILE=$output" (Join-Path $repo 'packaging/VideoPlayer.nsi')
    if ($LASTEXITCODE -ne 0) { throw 'NSIS compilation failed' }
    $installerHash = (Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash.ToLowerInvariant()
    "$installerHash  $(Split-Path $output -Leaf)" | Set-Content -LiteralPath "$output.sha256" -Encoding ASCII
    Get-FileHash -LiteralPath $output -Algorithm SHA256 | Format-List
    Write-Host 'Automatic install scope: administrators use Program Files and machine registration; standard accounts use their local profile and user registration.'
    if (!$ForRelease) { Write-Host 'Development installer built. Public release is blocked until Test-ReleaseGate passes.' }
} finally { Pop-Location }
