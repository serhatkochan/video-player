[CmdletBinding()]
param([string]$NsisPath)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
if (!$NsisPath) { $NsisPath = Join-Path $repo 'tools/nsis-3.11/makensis.exe' }
if (!(Test-Path -LiteralPath $NsisPath -PathType Leaf)) { throw 'NSIS compiler not found; run Get-BuildTools.ps1 first' }
$source = Get-Content -LiteralPath (Join-Path $repo 'packaging/VideoPlayer.nsi') -Raw
$macros = [regex]::Match($source, '(?s)!macro CheckResolvedDirectory DIRECTORY PREFIX.*?!macro CheckInstallPath PREFIX.*?!macroend').Value
$initialize = [regex]::Match($source, '(?s)Function \.onInit.*?FunctionEnd').Value
$languages = ([regex]::Matches($source, '(?m)^LangString .+$') | ForEach-Object { $_.Value }) -join "`n"
if (!$macros -or !$initialize -or !$languages) { throw 'Installer initialization source blocks not found' }
if ($initialize -match '(?im)^\s*(WriteReg|DeleteReg|CreateDirectory|CreateShortcut|Delete|RMDir|SetOutPath|Exec)') { throw 'The initialization probe must not execute installation mutations' }
$variables = ([regex]::Matches($source, '(?m)^Var ([\w.]+)\s*$') | Where-Object {
    ($macros + $initialize).Contains('$' + $_.Groups[1].Value)
} | ForEach-Object { $_.Value }) -join "`n"
$versionMatch = Select-String -LiteralPath (Join-Path $repo 'Cargo.toml') -Pattern '^version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"' | Select-Object -First 1
if (!$versionMatch) { throw 'Workspace version not found' }
$version = $versionMatch.Matches[0].Groups[1].Value
$payloadId = [Guid]::NewGuid().ToString('N').Substring(0,12)
$testRoot = Join-Path $repo "target/installer-initialization/$payloadId"
$actualRoot = Join-Path $testRoot 'actual'
$redirectedRoot = Join-Path $testRoot 'redirected'
New-Item -ItemType Directory -Force -Path (Join-Path $actualRoot 'VideoPlayer') | Out-Null
New-Item -ItemType Junction -Path $redirectedRoot -Target $actualRoot | Out-Null
try {
    foreach ($case in @('Native','Machine','User','Redirected')) {
        $probe = $initialize
        if ($case -ne 'Native') {
            $admin = if ($case -eq 'User') { '0' } else { '1' }
            $probe = $probe.Replace("System::Call 'shell32::IsUserAnAdmin() i.r0'", ('StrCpy $0 ' + $admin))
        }
        if ($case -eq 'Redirected') {
            $redirectedInstallRoot = (Join-Path $redirectedRoot 'VideoPlayer').Replace('$','$$')
            $probe = $probe.Replace('StrCpy $InstallRoot "$PROGRAMFILES64\VideoPlayer"', ('StrCpy $InstallRoot "' + $redirectedInstallRoot + '"'))
        }
        $probe = $probe.Replace('Function .onInit', ('Function .onInit' + "`n" + '  FileOpen $9 "${PROBE_LOG}" w'))
        $probe = $probe.Replace('install_unsafe_directory:', ('install_unsafe_directory:' + "`n" + '  FileWrite $9 "RESULT=Rejected$\r$\nSCOPE=$InstallScope$\r$\nPATH=$INSTDIR$\r$\n"' + "`n" + '  FileClose $9'))
        $probe = $probe.Replace('install_path_valid:', ('install_path_valid:' + "`n" + '  FileWrite $9 "RESULT=Accepted$\r$\nSCOPE=$InstallScope$\r$\nPATH=$INSTDIR$\r$\n"' + "`n" + '  FileClose $9'))
        $header = @'
Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "x64.nsh"
Name "Video Player initialization regression probe"
OutFile "${PROBE_OUTPUT}"
RequestExecutionLevel user
ManifestSupportedOS all
SilentInstall silent
'@
        $pages = @'
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "Turkish"
'@
        $script = "$header`n$variables`n$pages`n$languages`n$macros`n$probe`nSection`nSectionEnd`n"
        $scriptPath = Join-Path $testRoot "$case.nsi"
        $outputPath = Join-Path $testRoot "$case.exe"
        $logPath = Join-Path $testRoot "$case.log"
        $script | Set-Content -LiteralPath $scriptPath -Encoding UTF8
        & $NsisPath /V2 /WX /INPUTCHARSET UTF8 "/DVERSION=$version" "/DPAYLOAD_ID=$payloadId" "/DPROBE_OUTPUT=$($outputPath.Replace('$','$$'))" "/DPROBE_LOG=$($logPath.Replace('$','$$'))" $scriptPath
        if ($LASTEXITCODE -ne 0) { throw "$case probe compilation failed" }
        $process = Start-Process -FilePath $outputPath -ArgumentList '/S' -WindowStyle Hidden -Wait -PassThru
        $values = @{}
        foreach ($line in Get-Content -LiteralPath $logPath) {
            $pair = $line.Split(@('='), 2)
            if ($pair.Count -eq 2) { $values[$pair[0]] = $pair[1] }
        }
        $expectedExit = if ($case -eq 'Redirected') { 1 } else { 0 }
        $expectedResult = if ($case -eq 'Redirected') { 'Rejected' } else { 'Accepted' }
        if ($process.ExitCode -ne $expectedExit -or $values.RESULT -ne $expectedResult) { throw "$case initialization failed: exit=$($process.ExitCode), result=$($values.RESULT)" }
        if ($values.SCOPE -notin @('User','Machine')) { throw "$case returned an invalid install scope" }
        if ($case -eq 'User' -and $values.SCOPE -ne 'User') { throw 'User classification changed scope' }
        if ($case -in @('Machine','Redirected') -and $values.SCOPE -ne 'Machine') { throw 'Administrator classification changed scope' }
        $programFiles = if ($env:ProgramW6432) { $env:ProgramW6432 } else { $env:ProgramFiles }
        $expectedRoot = if ($case -eq 'Redirected') { Join-Path $redirectedRoot 'VideoPlayer' } elseif ($values.SCOPE -eq 'Machine') { Join-Path $programFiles 'VideoPlayer' } else { Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Programs/VideoPlayer' }
        $expectedPath = Join-Path $expectedRoot "versions/$version-$payloadId"
        if ($values.PATH -ne $expectedPath) { throw "$case returned an unexpected installation directory" }
        if (Test-Path -LiteralPath $expectedPath) { throw "$case probe unexpectedly wrote installation files" }
        Write-Host "$case initialization: $expectedResult; scope=$($values.SCOPE); no installation files written."
    }
} finally {
    $expectedRedirectedRoot = [IO.Path]::GetFullPath((Join-Path $repo "target/installer-initialization/$payloadId/redirected"))
    if ([IO.Path]::GetFullPath($redirectedRoot) -ne $expectedRedirectedRoot) { throw 'Unsafe junction cleanup path' }
    $junction = Get-Item -LiteralPath $redirectedRoot -Force -ErrorAction SilentlyContinue
    if ($junction) {
        if ($junction.LinkType -ne 'Junction') { throw 'Expected a test junction during cleanup' }
        Remove-Item -LiteralPath $redirectedRoot -Force
    }
}
