[CmdletBinding()]
param([Parameter(Mandatory)][string]$Work,[Parameter(Mandatory)][string]$Out)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
& (Join-Path $PSScriptRoot 'Initialize-Compiler.ps1')
& (Join-Path $PSScriptRoot 'Build-SharedFfmpeg.ps1') -Work $Work -Out $Out
$Work = (Resolve-Path -LiteralPath $Work).Path
$Out = (Resolve-Path -LiteralPath $Out).Path
$reproduction = Join-Path $Work 'reproduction'
New-Item -ItemType Directory -Force -Path $reproduction | Out-Null
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'Initialize-Compiler.ps1'),(Join-Path $PSScriptRoot 'Build-SharedFfmpeg.ps1'),$PSCommandPath -Destination $reproduction
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'wraps') -Destination $reproduction -Recurse -Force
$records = foreach ($directory in Get-ChildItem -LiteralPath $Work -Recurse -Directory -Force | Where-Object Name -eq '.git') {
    $source = Split-Path $directory.FullName -Parent
    [pscustomobject]@{
        path = $source.Substring($Work.Length + 1).Replace('\','/')
        origin = (& git -C $source remote get-url origin | Out-String).Trim()
        revision = (& git -C $source rev-parse HEAD | Out-String).Trim()
    }
}
$records | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $Work 'source-revisions.json') -Encoding utf8NoBOM
$name = 'video-player-ffmpeg-windows-x64'
Compress-Archive -Path (Join-Path $Out '*') -DestinationPath (Join-Path (Split-Path $Out -Parent) "$name.zip") -Force
Move-Item -LiteralPath (Join-Path (Split-Path $Out -Parent) "$name.zip") -Destination $Out -Force
$archive = Join-Path $Out "$name-sources.tar.gz"
& "$env:SystemRoot/System32/tar.exe" -czf $archive --exclude=.git --exclude=build --exclude=install -C $Work .
if ($LASTEXITCODE -ne 0) { throw 'FFmpeg source archive creation failed' }
$members = & "$env:SystemRoot/System32/tar.exe" -tf $archive
if ($LASTEXITCODE -ne 0 -or !($members -match 'ffmpeg/subprojects/zimg/meson.build') -or !($members -match 'ffmpeg/subprojects/dav1d/src/') -or !($members -match 'reproduction/Build-SharedFfmpeg.ps1')) { throw 'FFmpeg corresponding-source archive is incomplete' }
Get-ChildItem -LiteralPath $Out -File | Where-Object Name -ne 'SHA256SUMS.txt' | ForEach-Object {
    '{0}  {1}' -f (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant(),$_.Name
} | Set-Content -LiteralPath (Join-Path $Out 'SHA256SUMS.txt') -Encoding ascii
Get-ChildItem -LiteralPath $Out -File | Select-Object Name,Length
