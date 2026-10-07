[CmdletBinding()]
param([string]$RuntimeDirectory = (Join-Path (Split-Path $PSScriptRoot -Parent) 'runtime'))
$ErrorActionPreference = 'Stop'
foreach ($file in @('libmpv-2.dll', 'ffmpeg.exe', 'avformat-62.dll', 'avcodec-62.dll', 'avutil-60.dll', 'avfilter-11.dll', 'swscale-9.dll', 'swresample-6.dll')) {
    if (!(Test-Path -LiteralPath (Join-Path $RuntimeDirectory $file))) { throw "Missing runtime: $file" }
}
$ffmpeg = Join-Path $RuntimeDirectory 'ffmpeg.exe'
$version = (& $ffmpeg -hide_banner -version 2>&1 | Out-String)
if ($LASTEXITCODE -ne 0) { throw 'FFmpeg runtime did not start' }
if ($version -match '--enable-gpl|--enable-nonfree|--enable-libx264|--enable-libx265|--enable-libfdk-aac') { throw 'GPL/nonfree FFmpeg is not permitted in this package' }
if ($version -notmatch '--enable-shared' -or $version -notmatch 'libavformat\s+62\.' -or $version -notmatch 'libavfilter\s+11\.') { throw 'Unexpected FFmpeg configuration or ABI' }
$filters = (& $ffmpeg -hide_banner -filters 2>&1 | Out-String)
if ($LASTEXITCODE -ne 0 -or $filters -notmatch '\bzscale\b' -or $filters -notmatch '\btonemap\b') { throw 'HDR thumbnail filters are missing' }
$audit = Join-Path $RuntimeDirectory 'ffmpeg-build-config.txt'
$version | Set-Content -LiteralPath $audit -Encoding UTF8
Write-Host 'FFmpeg: LGPL configuration, expected shared ABI, zscale and tonemap verified.'
Write-Host 'libmpv archive checksum is enforced by Get-Runtime; full source redistribution is a separate release gate.'
