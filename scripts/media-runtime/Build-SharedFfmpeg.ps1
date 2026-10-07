[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Work,
    [Parameter(Mandatory)][string]$Out
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

$ffmpegRevision = 'ff04763ef7cd858605cb118c5626070498f2c49c'
$dav1dRevision = '54706fc6bc0cdecab7e9593974a4039cc038fca7'
$zimgRevision = 'f819b14e8f39d1282400b0d9543e8ef73c1b2bbd'
$Work = [IO.Path]::GetFullPath($Work)
$Out = [IO.Path]::GetFullPath($Out)
New-Item -ItemType Directory -Path $Work, $Out -Force | Out-Null

function Invoke-Native {
    param([string]$Command, [string[]]$Arguments)
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Command failed with exit code $LASTEXITCODE" }
}

function Get-PinnedSource {
    param([string]$Directory, [string]$Url, [string]$Revision)
    if (-not (Test-Path -LiteralPath (Join-Path $Directory '.git'))) {
        Invoke-Native git @('init', $Directory)
        Invoke-Native git @('-C', $Directory, 'remote', 'add', 'origin', $Url)
    }
    Invoke-Native git @('-C', $Directory, 'config', 'core.autocrlf', 'false')
    Invoke-Native git @('-C', $Directory, 'fetch', '--depth', '1', 'origin', $Revision)
    Invoke-Native git @('-C', $Directory, 'checkout', '--detach', 'FETCH_HEAD')
    $actual = (& git -C $Directory rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0 -or $actual -ne $Revision) { throw "Source revision mismatch: $Url" }
    Invoke-Native git @('-C', $Directory, 'submodule', 'update', '--init', '--recursive', '--depth', '1')
    & git -C $Directory submodule status --recursive |
        Set-Content -LiteralPath (Join-Path $Directory 'SOURCE-SUBMODULES.txt') -Encoding utf8
    if ($LASTEXITCODE -ne 0) { throw "Submodule inventory failed: $Url" }
}

foreach ($tool in @('git', 'meson', 'ninja', 'clang', 'clang++')) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { throw "Missing build tool: $tool" }
}
$env:CC = 'clang'
$env:CXX = 'clang++'
$env:CC_LD = 'lld-link'
$env:CXX_LD = 'lld-link'

$source = Join-Path $Work 'ffmpeg'
$build = Join-Path $Work 'build'
$prefix = Join-Path $Work 'install'
Get-PinnedSource $source 'https://gitlab.freedesktop.org/gstreamer/meson-ports/ffmpeg.git' $ffmpegRevision
$subprojects = Join-Path $source 'subprojects'
New-Item -ItemType Directory -Path $subprojects -Force | Out-Null

$dav1d = Join-Path $subprojects 'dav1d'
Get-PinnedSource $dav1d 'https://code.videolan.org/videolan/dav1d.git' $dav1dRevision
@"
[wrap-git]
directory = dav1d
url = https://code.videolan.org/videolan/dav1d.git
revision = $dav1dRevision
depth = 1

[provide]
dav1d = dav1d_dep
"@ | Set-Content -LiteralPath (Join-Path $subprojects 'dav1d.wrap') -Encoding utf8

$zimg = Join-Path $subprojects 'zimg'
Get-PinnedSource $zimg 'https://github.com/sekrit-twc/zimg.git' $zimgRevision
@"
[wrap-git]
directory = zimg
url = https://github.com/sekrit-twc/zimg.git
revision = $zimgRevision
depth = 1

[provide]
zimg = zimg_dep
"@ | Set-Content -LiteralPath (Join-Path $subprojects 'zimg.wrap') -Encoding utf8

# Upstream 3.0.6 uses Autotools. Keep the equivalent Meson overlay in the source archive.
$makefile = Get-Content -LiteralPath (Join-Path $zimg 'Makefile.am') -Raw
function Get-ZimgFiles {
    param([string]$Target)
    $pattern = '(?m)^' + [regex]::Escape($Target) + '\s*=\s*(?<files>(?:[^\r\n]*\\\r?\n)*[^\r\n]*)'
    $match = [regex]::Match($makefile, $pattern)
    if (-not $match.Success) { throw "Missing zimg source list: $Target" }
    return @([regex]::Matches($match.Groups['files'].Value, 'src/zimg/[A-Za-z0-9_./+-]+\.cpp') |
        ForEach-Object { "'" + $_.Value + "'" })
}
$base = @(Get-ZimgFiles 'libzimg_internal_la_SOURCES')
$x86Match = [regex]::Match($makefile, '(?ms)^if X86SIMD\r?\n.*?^libzimg_internal_la_SOURCES \+=\s*(?<files>(?:[^\r\n]*\\\r?\n)*[^\r\n]*)')
if (-not $x86Match.Success) { throw 'Missing zimg x86 dispatch source list' }
$base += @([regex]::Matches($x86Match.Groups['files'].Value, 'src/zimg/[A-Za-z0-9_./+-]+\.cpp') |
    ForEach-Object { "'" + $_.Value + "'" })
$zimgMeson = @"
project('zimg', 'cpp', version: '3.0.6', default_options: ['cpp_std=c++14', 'b_vscrt=mt'])
inc = include_directories('src/zimg', 'src/zimg/api')
base_args = ['-DZIMG_X86', '-DNDEBUG']
objects = []
"@
$simdGroups = @(
    @{ Name = 'sse'; Args = "'-msse'" },
    @{ Name = 'sse2'; Args = "'-msse2'" },
    @{ Name = 'avx'; Args = "'-mavx'" },
    @{ Name = 'f16c'; Args = "'-mavx', '-mf16c'" },
    @{ Name = 'avx2'; Args = "'-mavx2', '-mf16c', '-mfma'" }
)
foreach ($group in $simdGroups) {
    $files = (Get-ZimgFiles ("lib" + $group.Name + '_la_SOURCES')) -join ",`n"
    $zimgMeson += @"

simd = static_library('zimg_$($group.Name)', files($files),
  include_directories: inc, cpp_args: base_args + [$($group.Args)], install: false)
objects += simd.extract_all_objects(recursive: true)
"@
}
$zimgMeson += @"

zimg_lib = static_library('zimg', files($($base -join ",`n")), objects: objects,
  include_directories: inc, cpp_args: base_args, install: true)
zimg_dep = declare_dependency(link_with: zimg_lib, include_directories: inc, version: '3.0.6')
meson.override_dependency('zimg', zimg_dep)
install_headers('src/zimg/api/zimg.h', 'src/zimg/api/zimg++.hpp')
pkg = import('pkgconfig')
pkg.generate(zimg_lib, name: 'zimg', description: 'Image processing library', version: '3.0.6')
"@
$zimgMeson | Set-Content -LiteralPath (Join-Path $zimg 'meson.build') -Encoding utf8
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'wraps/zlib.wrap') -Destination $subprojects -Force

$options = @(
    '-Ddefault_library=shared', '-Db_vscrt=mt', '-Dgpl=disabled', '-Dversion3=disabled',
    '-Dnonfree=disabled', '-Dprograms=enabled', '-Dffmpeg=enabled', '-Dffplay=disabled',
    '-Dffprobe=disabled', '-Dtests=disabled', '-Davdevice=disabled', '-Ddevices=disabled',
    '-Dpostproc=disabled', '-Dlibdav1d=enabled', '-Dlibzimg=enabled', '-Dzlib=enabled',
    '-Dbzlib=disabled', '-Dlzma=disabled', '-Diconv=disabled',
    '-Ddav1d:default_library=static', '-Ddav1d:enable_tools=false', '-Ddav1d:enable_tests=false',
    '-Dzlib:default_library=static', '-Dzimg:default_library=static',
    '-Dforce_fallback_for=dav1d,zimg,zlib'
)
$setup = @('setup', $build, $source, '--prefix', $prefix, '--libdir', 'lib', '--buildtype', 'release') + $options
if (Test-Path -LiteralPath (Join-Path $build 'meson-private/coredata.dat')) { $setup += '--wipe' }
Invoke-Native meson $setup
Invoke-Native meson @('compile', '-C', $build)
Invoke-Native meson @('install', '-C', $build, '--no-rebuild')

$binaries = @(Get-ChildItem -LiteralPath (Join-Path $prefix 'bin') -File |
    Where-Object { $_.Extension -eq '.dll' -or $_.Name -eq 'ffmpeg.exe' })
foreach ($file in $binaries) { Copy-Item -LiteralPath $file.FullName -Destination $Out -Force }
foreach ($name in @('ffmpeg.exe', 'avcodec-62.dll', 'avformat-62.dll', 'avutil-60.dll',
                   'avfilter-11.dll', 'swscale-9.dll', 'swresample-6.dll')) {
    if (-not (Test-Path -LiteralPath (Join-Path $Out $name))) { throw "Missing runtime binary: $name" }
}
Copy-Item -LiteralPath (Join-Path $prefix 'include') -Destination $Out -Recurse -Force
$licenses = Join-Path $Out 'licenses'
New-Item -ItemType Directory -Path $licenses -Force | Out-Null
foreach ($component in @(
    @{Name='ffmpeg'; Path=$source; Pattern='COPYING*'},
    @{Name='dav1d'; Path=$dav1d; Pattern='COPYING*'},
    @{Name='zimg'; Path=$zimg; Pattern='COPYING*'}
)) {
    $destination = Join-Path $licenses $component.Name
    New-Item -ItemType Directory -Path $destination -Force | Out-Null
    Get-ChildItem -LiteralPath $component.Path -Filter $component.Pattern -File |
        ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination $destination -Force }
}
$zlibSource = Get-ChildItem -LiteralPath $subprojects -Directory -Filter 'zlib-*' | Select-Object -First 1
if (-not $zlibSource) { throw 'Missing bundled zlib sources' }
New-Item -ItemType Directory -Path (Join-Path $licenses 'zlib') -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $zlibSource.FullName 'README') -Destination (Join-Path $licenses 'zlib') -Force

$ffmpegExe = Join-Path $Out 'ffmpeg.exe'
$licenseText = (& $ffmpegExe -hide_banner -L 2>&1 | Out-String)
if ($LASTEXITCODE -ne 0 -or $licenseText -notmatch 'Lesser General Public License.*version 2\.1') {
    throw 'FFmpeg LGPL 2.1 license assertion failed'
}
$filters = (& $ffmpegExe -hide_banner -filters 2>&1 | Out-String)
if ($LASTEXITCODE -ne 0 -or $filters -notmatch '\bzscale\b' -or $filters -notmatch '\btonemap\b') {
    throw 'Required HDR thumbnail filters missing'
}
$decoders = (& $ffmpegExe -hide_banner -decoders 2>&1 | Out-String)
if ($LASTEXITCODE -ne 0 -or $decoders -notmatch '\blibdav1d\b') { throw 'AV1 dav1d decoder missing' }
$licenseText | Set-Content -LiteralPath (Join-Path $Out 'FFMPEG-LICENSE.txt') -Encoding utf8
$filters | Set-Content -LiteralPath (Join-Path $Out 'FFMPEG-FILTERS.txt') -Encoding utf8
$decoders | Set-Content -LiteralPath (Join-Path $Out 'FFMPEG-DECODERS.txt') -Encoding utf8
@{
    ffmpeg = @{ url='https://gitlab.freedesktop.org/gstreamer/meson-ports/ffmpeg.git'; revision=$ffmpegRevision }
    dav1d = @{ url='https://code.videolan.org/videolan/dav1d.git'; revision=$dav1dRevision }
    zimg = @{ url='https://github.com/sekrit-twc/zimg.git'; revision=$zimgRevision; simd='SSE/SSE2/AVX/F16C/AVX2' }
    compiler = ((& clang --version) | Select-Object -First 1)
    meson = (& meson --version)
    options = $options
    license = 'LGPL-2.1-or-later'
} | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $Out 'BUILD-INFO.json') -Encoding utf8
$recipeDir = Join-Path $Work 'build-recipes'
New-Item -ItemType Directory -Path $recipeDir -Force | Out-Null
Copy-Item -LiteralPath $PSCommandPath -Destination $recipeDir -Force
Write-Host "Shared FFmpeg runtime ready: $Out"
