[CmdletBinding()]
param(
    [string]$Work = (Join-Path (Split-Path (Split-Path $PSScriptRoot -Parent) -Parent) 'target/media-runtime/libmpv'),
    [string]$Out = (Join-Path (Split-Path (Split-Path $PSScriptRoot -Parent) -Parent) 'dist/media-runtime/libmpv')
)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Set-StrictMode -Version Latest
$pins = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'sources.json') -Raw | ConvertFrom-Json
New-Item -ItemType Directory -Force -Path $Work,$Out | Out-Null
$Work = (Resolve-Path -LiteralPath $Work).Path
$Out = (Resolve-Path -LiteralPath $Out).Path
function Invoke-Checked([string]$Command, [string[]]$Arguments) {
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Command exited with $LASTEXITCODE" }
}
function Get-PinnedSource([string]$Name, [string]$Destination) {
    $pin = $pins.$Name
    if (!(Test-Path -LiteralPath (Join-Path $Destination '.git'))) {
        Invoke-Checked git @('init','-q',$Destination)
        Invoke-Checked git @('-C',$Destination,'config','core.autocrlf','false')
        Invoke-Checked git @('-C',$Destination,'remote','add','origin',$pin.url)
        Invoke-Checked git @('-C',$Destination,'fetch','--depth','1','origin',$pin.revision)
        Invoke-Checked git @('-C',$Destination,'checkout','--detach','FETCH_HEAD')
        Invoke-Checked git @('-C',$Destination,'submodule','update','--init','--recursive','--depth','1')
    }
    $actual = (& git -C $Destination rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0 -or $actual -cne $pin.revision) { throw "$Name source revision does not match its full commit pin" }
}
$recipe = Join-Path $Work 'recipe'
$mpv = Join-Path $Work 'mpv'
$shaderc = Join-Path $Work 'shaderc'
$cross = Join-Path $Work 'spirv-cross'
foreach ($pair in @(@('recipe',$recipe),@('mpv',$mpv),@('shaderc',$shaderc),@('spirv-cross',$cross))) { Get-PinnedSource $pair[0] $pair[1] }
foreach ($name in @('glslang','spirv-headers','spirv-tools')) { Get-PinnedSource $name (Join-Path $shaderc "third_party/$name") }
$vswhere = Join-Path ([Environment]::GetFolderPath('ProgramFilesX86')) 'Microsoft Visual Studio/Installer/vswhere.exe'
$vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (!$vs) { throw 'Visual Studio C++ tools are required' }
Import-Module "$vs/Common7/Tools/Microsoft.VisualStudio.DevShell.dll"
Enter-VsDevShell -VsInstallPath $vs -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64'
$cmake = (Get-Command cmake -ErrorAction Stop).Source
$prefix = Join-Path $Work 'prefix'
$prefixUnix = $prefix.Replace('\','/')
$env:CC = 'clang'
$env:CXX = 'clang++'
$env:CC_LD = 'lld-link'
$env:CXX_LD = 'lld-link'
$env:WINDRES = 'llvm-rc'
$base = @('-G','Ninja','-DCMAKE_BUILD_TYPE=Release','-DCMAKE_C_COMPILER=clang','-DCMAKE_CXX_COMPILER=clang++','-DCMAKE_POLICY_DEFAULT_CMP0091=NEW','-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded',"-DCMAKE_INSTALL_PREFIX=$prefix",'-DCMAKE_INSTALL_LIBDIR=lib')
Invoke-Checked $cmake (@('-S',$shaderc,'-B',(Join-Path $Work 'build-shaderc')) + $base + @('-DSHADERC_SKIP_TESTS=ON','-DSHADERC_SKIP_EXAMPLES=ON','-DSHADERC_SKIP_EXECUTABLES=ON','-DSHADERC_SKIP_COPYRIGHT_CHECK=ON','-DSHADERC_ENABLE_SHARED_CRT=OFF','-DSPIRV_SKIP_TESTS=ON','-DSPIRV_SKIP_EXECUTABLES=ON','-DSHADERC_ENABLE_WERROR_COMPILE=OFF'))
Invoke-Checked $cmake @('--build',(Join-Path $Work 'build-shaderc'),'--parallel','4')
Invoke-Checked $cmake @('--install',(Join-Path $Work 'build-shaderc'))
Copy-Item -LiteralPath (Join-Path $prefix 'lib/pkgconfig/shaderc_combined.pc') -Destination (Join-Path $prefix 'lib/pkgconfig/shaderc.pc') -Force
Invoke-Checked $cmake (@('-S',$cross,'-B',(Join-Path $Work 'build-spirv-cross')) + $base + @('-DSPIRV_CROSS_SHARED=ON','-DSPIRV_CROSS_STATIC=OFF','-DSPIRV_CROSS_CLI=OFF','-DSPIRV_CROSS_ENABLE_TESTS=OFF','-DSPIRV_CROSS_ENABLE_MSL=OFF','-DSPIRV_CROSS_ENABLE_CPP=OFF','-DSPIRV_CROSS_ENABLE_REFLECT=OFF'))
Invoke-Checked $cmake @('--build',(Join-Path $Work 'build-spirv-cross'),'--parallel','4')
Invoke-Checked $cmake @('--install',(Join-Path $Work 'build-spirv-cross'))
$crossDll = Join-Path $prefix 'bin/spirv-cross-c-shared.dll'
if (!(Test-Path -LiteralPath $crossDll -PathType Leaf)) { throw 'SPIRV-Cross runtime DLL was not installed' }
$crossImports = (& dumpbin /nologo /dependents $crossDll | Out-String)
if ($LASTEXITCODE -ne 0) { throw 'SPIRV-Cross dependency audit failed' }
if ($crossImports.ToLowerInvariant() -cmatch '(?m)^\s*(vcruntime|msvcp|msvcr)[^\s]*\.dll\s*$') { throw 'SPIRV-Cross unexpectedly requires a Visual C++ redistributable' }
Write-Host 'SPIRV-Cross: Visual C++ runtime DLL dependencies are absent.'
$env:PKG_CONFIG_PATH = Join-Path $prefix 'lib/pkgconfig'
$env:PATH = (Join-Path $prefix 'bin') + ';' + $env:PATH
$subprojects = Join-Path $mpv 'subprojects'
New-Item -ItemType Directory -Force -Path $subprojects | Out-Null
Copy-Item -LiteralPath (Get-ChildItem -LiteralPath (Join-Path $PSScriptRoot 'wraps') -File).FullName -Destination $subprojects -Force
$dav1d = Join-Path $subprojects 'dav1d'
Get-PinnedSource 'dav1d' $dav1d
# FFmpeg's parent config.asm otherwise shadows dav1d's nested NASM configuration.
$dav1dMesonPath = Join-Path $dav1d 'meson.build'
$dav1dMeson = Get-Content -LiteralPath $dav1dMesonPath -Raw
$dav1dConfigOld = "configure_file(output: 'config.asm', output_format: 'nasm', configuration: cdata_asm)"
$dav1dConfigNew = "configure_file(output: 'dav1d-config.asm', output_format: 'nasm', configuration: cdata_asm)"
if ($dav1dMeson.Contains($dav1dConfigOld)) {
    $dav1dMeson.Replace($dav1dConfigOld,$dav1dConfigNew) | Set-Content -LiteralPath $dav1dMesonPath -Encoding utf8NoBOM
} elseif (!$dav1dMeson.Contains($dav1dConfigNew)) {
    throw 'Pinned dav1d NASM configuration declaration changed'
}
$dav1dAsmSources = @(Get-ChildItem -LiteralPath (Join-Path $dav1d 'src/x86') -Filter '*.asm' -File)
if (!$dav1dAsmSources.Count) { throw 'Pinned dav1d x86 assembly sources missing' }
$dav1dConfigIncludes = 0
foreach ($asm in $dav1dAsmSources) {
    $asmText = Get-Content -LiteralPath $asm.FullName -Raw
    if ($asmText.Contains('%include "config.asm"')) {
        $asmText.Replace('%include "config.asm"','%include "dav1d-config.asm"') | Set-Content -LiteralPath $asm.FullName -Encoding utf8NoBOM
        $dav1dConfigIncludes++
    } elseif ($asmText.Contains('%include "dav1d-config.asm"')) {
        $dav1dConfigIncludes++
    }
}
if (!$dav1dConfigIncludes) { throw 'Pinned dav1d assembly configuration includes missing' }
$upstreamPins = Get-Content -LiteralPath (Join-Path $recipe 'sources.json') -Raw | ConvertFrom-Json
foreach ($name in @('mpv','ffmpeg','libass','libplacebo','dav1d')) { $upstreamPins.$name = $pins.$name }
$upstreamPins | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $recipe 'sources.json') -Encoding utf8NoBOM
$buildScript = Join-Path $recipe 'windows/build.ps1'
$text = Get-Content -LiteralPath $buildScript -Raw
function Replace-Required([string]$Old, [string]$New) {
    if (!$script:text.Contains($Old)) { throw "Pinned recipe no longer contains expected text: $Old" }
    $script:text = $script:text.Replace($Old,$New)
}
$helperPattern = '(?m)^    @\{ Old = "\x27video/filter/vf_d3d11vpp[.]c\x27\)"\r?\n       New = .*?\r?\n             .*?\r?\n'
if (![regex]::IsMatch($text,$helperPattern)) { throw 'D3D11 helper patch not found in pinned recipe' }
$text = [regex]::Replace($text,$helperPattern,'')
Replace-Required 'meson wrap update-db' '# Wrap definitions are vendored at fixed versions.'
Replace-Required 'if (-not (Test-Path "subprojects/$wrap.wrap")) { meson wrap install $wrap }' 'if (-not (Test-Path "subprojects/$wrap.wrap")) { throw "Missing pinned wrap: $wrap" }'
$newline = [char]10
$continuation = [char]96
Replace-Required ("            -Dffmpeg:checkasm=disabled $continuation$newline") ''
Replace-Required '--wrap-mode=forcefallback' ("--wrap-mode=default $continuation$newline            -Dpkg_config_path=$prefixUnix/lib/pkgconfig")
foreach ($feature in @('libplacebo:d3d11','libplacebo:shaderc','d3d11','shaderc','spirv-cross')) { Replace-Required "-D$feature=disabled" "-D$feature=enabled" }
$copyExtra = @'
Copy-Item $dll.FullName, $implib.FullName $pkg
Copy-Item (Join-Path $env:VIDEO_PLAYER_MEDIA_PREFIX "bin/spirv-cross-c-shared.dll") $pkg
'@
Replace-Required 'Copy-Item $dll.FullName, $implib.FullName $pkg' $copyExtra
$env:VIDEO_PLAYER_MEDIA_PREFIX = $prefix
[IO.File]::WriteAllText($buildScript,$text,[Text.UTF8Encoding]::new($false))
& $buildScript -Work $Work -Out $Out
if ($LASTEXITCODE -ne 0) { throw 'Pinned libmpv recipe failed' }
$package = Get-ChildItem -LiteralPath $Out -Directory | Where-Object Name -like 'libmpv-*' | Select-Object -First 1
if (!$package) { throw 'libmpv binary package not found' }
$licenses = Join-Path $package.FullName 'LICENSES'
foreach ($name in @('COPYING.LGPLv3','COPYING.GPLv3')) {
    Copy-Item -LiteralPath (Join-Path $mpv "subprojects/ffmpeg/$name") -Destination $licenses -Force
}
@'
The combined libmpv DLL is distributed under LGPL-3.0-or-later.
mpv and FFmpeg are LGPL-2.1-or-later; their later-version permission is used
for compatibility with the Apache-2.0 shaderc and SPIRV-Tools components.
See COPYING.LGPLv3, COPYING.GPLv3 and each component's preserved license notices.
Video Player's original application code remains MIT licensed.
'@ | Set-Content -LiteralPath (Join-Path $licenses 'COMBINED-LICENSE.txt') -Encoding utf8NoBOM
foreach ($entry in @(@('shaderc',$shaderc),@('spirv-cross',$cross))) {
    foreach ($file in Get-ChildItem -LiteralPath $entry[1] -Recurse -File | Where-Object { $_.Name -match '^(LICENSE|COPYING|NOTICE)([._-]|$)' }) {
        if ($file.FullName -match '[\\/]\.git[\\/]') { continue }
        $relative = $file.FullName.Substring($entry[1].Length + 1)
        $destination = Join-Path $licenses ($entry[0] + '/' + $relative)
        New-Item -ItemType Directory -Force -Path (Split-Path $destination -Parent) | Out-Null
        Copy-Item -LiteralPath $file.FullName -Destination $destination
    }
}
$records = foreach ($directory in Get-ChildItem -LiteralPath $Work -Recurse -Directory -Force | Where-Object Name -eq '.git') {
    $source = Split-Path $directory.FullName -Parent
    [pscustomobject]@{
        path = $source.Substring($Work.Length + 1).Replace('\','/')
        origin = (& git -C $source remote get-url origin | Out-String).Trim()
        revision = (& git -C $source rev-parse HEAD | Out-String).Trim()
    }
}
$records | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $Work 'source-revisions.json') -Encoding utf8NoBOM
$reproduction = Join-Path $Work 'reproduction'
New-Item -ItemType Directory -Force -Path $reproduction | Out-Null
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'Build-Libmpv.ps1'),(Join-Path $PSScriptRoot 'sources.json') -Destination $reproduction
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'wraps') -Destination $reproduction -Recurse -Force
Copy-Item -LiteralPath (Join-Path $recipe 'LICENSE') -Destination (Join-Path $licenses 'recipe-LGPL-2.1.txt')
$buildInfo = Join-Path $package.FullName 'BUILD-INFO.txt'
@'

Video Player modifications
  D3D11 and D3D11VA enabled in mpv; D3D11 enabled in libplacebo.
  Shaderc combined static library and SPIRV-Cross shared C API use /MT.
  All patched source trees, dependency revisions and this recipe are retained.
  Binary smoke checks validate loading and initialization; physical HDR requires device acceptance tests.
'@ | Add-Content -LiteralPath $buildInfo
$baseName = $package.Name
Compress-Archive -Path (Join-Path $package.FullName '*') -DestinationPath (Join-Path $Out "$baseName.zip") -Force
$sourceArchive = Join-Path $Out "$baseName-sources.tar.gz"
Invoke-Checked "$env:SystemRoot/System32/tar.exe" @('-czf',$sourceArchive,'--exclude=.git','--exclude=mpv/build','--exclude=build-shaderc','--exclude=build-spirv-cross','--exclude=prefix','-C',$Work,'.')
$members = & "$env:SystemRoot/System32/tar.exe" -tf $sourceArchive
if ($LASTEXITCODE -ne 0 -or !($members -match 'reproduction/Build-Libmpv.ps1') -or !($members -match 'shaderc/third_party/spirv-tools/source/') -or !($members -match 'mpv/subprojects/ffmpeg/')) { throw 'Corresponding-source archive is incomplete' }
Get-ChildItem -LiteralPath $Out -File | ForEach-Object {
    '{0}  {1}' -f (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant(),$_.Name
} | Set-Content -LiteralPath (Join-Path $Out 'SHA256SUMS.txt') -Encoding ascii
Get-ChildItem -LiteralPath $Out -File | Select-Object Name,Length
