[CmdletBinding()]
param([string]$ArchiveDirectory, [string]$RuntimeDirectory)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
if (!$RuntimeDirectory) { $RuntimeDirectory = Join-Path $repo 'runtime' }
if (!$ArchiveDirectory) { $ArchiveDirectory = Join-Path $RuntimeDirectory 'downloads' }
$manifest = Get-Content (Join-Path $repo 'packaging/runtime-manifest.json') -Raw | ConvertFrom-Json
function Copy-PinnedRuntimeFile([string]$Source) {
    $destination = Join-Path $RuntimeDirectory (Split-Path $Source -Leaf)
    if ((Test-Path -LiteralPath $destination) -and
        (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash -ceq (Get-FileHash -LiteralPath $Source -Algorithm SHA256).Hash) { return }
    Copy-Item -LiteralPath $Source -Destination $destination -Force
}
New-Item -ItemType Directory -Force -Path $RuntimeDirectory, $ArchiveDirectory | Out-Null
foreach ($asset in $manifest.assets) {
    $archive = Join-Path $ArchiveDirectory $asset.file
    if (!(Test-Path -LiteralPath $archive)) {
        Write-Host "Downloading pinned $($asset.id)..."
        Invoke-WebRequest -Uri $asset.url -OutFile $archive
    }
    if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $asset.sha256) {
        throw "Checksum mismatch: $archive. No unverified runtime will be staged."
    }
    $extract = Join-Path $ArchiveDirectory $asset.id
    New-Item -ItemType Directory -Force -Path $extract | Out-Null
    & tar.exe -xf $archive -C $extract
    if ($LASTEXITCODE -ne 0) { throw "Could not extract $archive with Windows tar.exe" }
    if ($asset.id -eq 'libmpv') {
        $dll = Get-ChildItem -LiteralPath $extract -Recurse -Filter libmpv-2.dll | Select-Object -First 1
        if (!$dll) { throw 'libmpv archive has no libmpv-2.dll' }
        Copy-PinnedRuntimeFile $dll.FullName
        $include = Get-ChildItem -LiteralPath $extract -Directory -Recurse | Where-Object Name -eq 'include' | Select-Object -First 1
        if ($include) {
            New-Item -ItemType Directory -Force -Path (Join-Path $RuntimeDirectory 'include') | Out-Null
            Copy-Item -Path (Join-Path $include.FullName '*') -Destination (Join-Path $RuntimeDirectory 'include') -Recurse -Force
        }
    } else {
        $exe = Get-ChildItem -LiteralPath $extract -Recurse -Filter ffmpeg.exe | Select-Object -First 1
        if (!$exe) { throw 'FFmpeg archive has no executable for runtime auditing' }
        Get-ChildItem -LiteralPath $exe.DirectoryName -Filter *.dll -File | ForEach-Object { Copy-PinnedRuntimeFile $_.FullName }
        Copy-PinnedRuntimeFile $exe.FullName
        $include = Join-Path (Split-Path $exe.DirectoryName -Parent) 'include'
        if (Test-Path -LiteralPath $include) {
            New-Item -ItemType Directory -Force -Path (Join-Path $RuntimeDirectory 'include') | Out-Null
            Copy-Item -Path (Join-Path $include '*') -Destination (Join-Path $RuntimeDirectory 'include') -Recurse -Force
        }
        $licenses = Join-Path $RuntimeDirectory 'licenses/ffmpeg'
        New-Item -ItemType Directory -Force -Path $licenses | Out-Null
        Get-ChildItem -LiteralPath $extract -Recurse -File | Where-Object { $_.Name.ToLowerInvariant() -cmatch '^(license|copying)' } |
            ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination $licenses -Force }
    }
}
$licenseDir = Join-Path $RuntimeDirectory 'licenses/mpv'
New-Item -ItemType Directory -Force -Path $licenseDir | Out-Null
$mpv = $manifest.assets | Where-Object id -eq 'libmpv'
foreach ($name in @('Copyright', 'LICENSE.LGPL', 'LICENSE.GPL')) {
    $path = Join-Path $licenseDir $name
    if (!(Test-Path -LiteralPath $path)) {
        Invoke-WebRequest -Uri "https://raw.githubusercontent.com/mpv-player/mpv/$($mpv.upstreamCommit)/$name" -OutFile $path
    }
}
$ffmpegSource = $manifest.assets | Where-Object id -eq 'ffmpeg'
foreach ($name in @('COPYING.GPLv3','COPYING.LGPLv3','LICENSE.md')) {
    $path = Join-Path $RuntimeDirectory "licenses/ffmpeg/$name"
    if (!(Test-Path -LiteralPath $path)) {
        Invoke-WebRequest -Uri "https://raw.githubusercontent.com/FFmpeg/FFmpeg/$($ffmpegSource.upstreamCommit)/$name" -OutFile $path
    }
}
Copy-Item -LiteralPath (Join-Path $repo 'packaging/runtime-manifest.json') -Destination $RuntimeDirectory -Force
foreach ($bundle in $manifest.sourceBundles) {
    $archive = Join-Path $ArchiveDirectory $bundle.file
    if (!(Test-Path -LiteralPath $archive)) { Invoke-WebRequest -Uri $bundle.url -OutFile $archive }
    if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $bundle.sha256) { throw 'Corresponding source bundle checksum mismatch' }
    $sources = Join-Path $RuntimeDirectory 'sources'
    New-Item -ItemType Directory -Force -Path $sources | Out-Null
    & tar.exe -xf $archive -C $sources
    if ($LASTEXITCODE -ne 0) { throw 'Could not extract corresponding source bundle' }
}
& (Join-Path $PSScriptRoot 'Test-Runtime.ps1') -RuntimeDirectory $RuntimeDirectory
