Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "x64.nsh"
!ifndef VERSION
  !error "Pass /DVERSION=..."
!endif
!ifndef PAYLOAD_ID
  !error "Pass /DPAYLOAD_ID=..."
!endif
!ifndef STAGE_DIRECTORY
  !error "Pass /DSTAGE_DIRECTORY=..."
!endif
!ifndef OUTPUT_FILE
  !error "Pass /DOUTPUT_FILE=..."
!endif
Name "Video Player"
OutFile "${OUTPUT_FILE}"
InstallDir "$LOCALAPPDATA\Programs\VideoPlayer\versions\${VERSION}-${PAYLOAD_ID}"
RequestExecutionLevel highest
ManifestSupportedOS all
SetCompressor /SOLID lzma
VIProductVersion "${VERSION}.0"
VIAddVersionKey /LANG=1033 "ProductName" "Video Player"
VIAddVersionKey /LANG=1033 "FileDescription" "Video Player automatic user or machine installer"
VIAddVersionKey /LANG=1033 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=1033 "LegalCopyright" "MIT licensed Video Player contributors"
Var IntegrationResult
Var InstallScope
Var InstallRoot
Var InstallScopeDescription
Var ElevationArguments
!define MUI_ICON "..\crates\player\assets\video-player.ico"
!define MUI_UNICON "..\crates\player\assets\video-player.ico"
!define MUI_ABORTWARNING
!define MUI_WELCOMEPAGE_TEXT "$(InstallWelcome)$\r$\n$\r$\n$InstallScopeDescription"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "..\LICENSE"
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\video-player.exe"
!define MUI_FINISHPAGE_RUN_NOTCHECKED
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "Turkish"
LangString InstallWelcome ${LANG_ENGLISH} "This wizard will install Video Player and its Explorer thumbnail integration."
LangString InstallWelcome ${LANG_TURKISH} "Bu sihirbaz Video Player'ı ve Explorer küçük resim entegrasyonunu kuracak."
LangString MachineScopeInfo ${LANG_ENGLISH} "Administrator access is available. Video Player will be installed in Program Files for all Windows accounts. Windows UAC settings will remain unchanged."
LangString MachineScopeInfo ${LANG_TURKISH} "Yönetici yetkisi kullanılabiliyor. Video Player tüm Windows hesapları için Program Files'a kurulacak. Windows UAC ayarları değiştirilmeyecek."
LangString UserScopeInfo ${LANG_ENGLISH} "Video Player will be installed for your current Windows account. Administrator access is not required."
LangString UserScopeInfo ${LANG_TURKISH} "Video Player mevcut Windows hesabınıza kurulacak. Yönetici yetkisi gerekmiyor."
LangString MachineUninstallElevation ${LANG_ENGLISH} "Removing this installation requires administrator access. Approve the Windows permission request or ask an administrator to remove Video Player."
LangString MachineUninstallElevation ${LANG_TURKISH} "Bu kurulumu kaldırmak için yönetici yetkisi gerekiyor. Windows izin isteğini onaylayın veya bir yöneticiden Video Player'ı kaldırmasını isteyin."
!macro CheckResolvedDirectory DIRECTORY PREFIX
  System::Call 'kernel32::GetFileAttributesW(w "${DIRECTORY}") i.r0'
  ${If} $0 != -1
    IntOp $0 $0 & 0x400
    StrCmp $0 0 0 ${PREFIX}unsafe_directory
    System::Call 'kernel32::CreateFileW(w "${DIRECTORY}", i 0, i 7, p 0, i 3, i 0x02000000, p 0) i.r2'
    StrCmp $2 -1 ${PREFIX}unsafe_directory
    System::Call 'kernel32::GetFinalPathNameByHandleW(p r2, w .r3, i ${NSIS_MAX_STRLEN}, i 0) i.r4'
    System::Call 'kernel32::CloseHandle(p r2)'
    ${If} $4 == 0
    ${OrIf} $4 >= ${NSIS_MAX_STRLEN}
      Goto ${PREFIX}unsafe_directory
    ${EndIf}
    StrCmp $3 "\\?\${DIRECTORY}" 0 ${PREFIX}unsafe_directory
  ${EndIf}
!macroend

!macro CheckInstallPath PREFIX
  System::Call 'kernel32::GetFullPathNameW(w "$InstallRoot\versions\${VERSION}-${PAYLOAD_ID}", i ${NSIS_MAX_STRLEN}, w .r0, p 0) i.r2'
  ${If} $2 == 0
  ${OrIf} $2 >= ${NSIS_MAX_STRLEN}
    Goto ${PREFIX}unsafe_directory
  ${EndIf}
  System::Call 'kernel32::GetFullPathNameW(w "$INSTDIR", i ${NSIS_MAX_STRLEN}, w .r1, p 0) i.r2'
  ${If} $2 == 0
  ${OrIf} $2 >= ${NSIS_MAX_STRLEN}
    Goto ${PREFIX}unsafe_directory
  ${EndIf}
  StrCmp $0 "" ${PREFIX}unsafe_directory
  StrCmp $1 "" ${PREFIX}unsafe_directory
  StrCmp $0 $1 0 ${PREFIX}unsafe_directory
  StrCpy $INSTDIR $1
  ${If} $InstallScope == "Machine"
    !insertmacro CheckResolvedDirectory "$PROGRAMFILES64" "${PREFIX}"
  ${Else}
    !insertmacro CheckResolvedDirectory "$LOCALAPPDATA" "${PREFIX}"
    !insertmacro CheckResolvedDirectory "$LOCALAPPDATA\Programs" "${PREFIX}"
  ${EndIf}
  !insertmacro CheckResolvedDirectory "$InstallRoot" "${PREFIX}"
  !insertmacro CheckResolvedDirectory "$InstallRoot\versions" "${PREFIX}"
  !insertmacro CheckResolvedDirectory "$INSTDIR" "${PREFIX}"
!macroend

Function .onInit
  ${IfNot} ${IsNativeAMD64}
    MessageBox MB_ICONSTOP "Video Player requires Windows 11 on an Intel or AMD x64 computer. ARM64 Explorer cannot load this x64 thumbnail provider." /SD IDOK
    Abort
  ${EndIf}
  SetShellVarContext current
  SetRegView 64
  ReadRegStr $0 HKLM "SOFTWARE\Microsoft\Windows NT\CurrentVersion" "CurrentBuildNumber"
  IntCmp $0 22000 supported_windows unsupported_windows supported_windows
unsupported_windows:
  MessageBox MB_ICONSTOP "Video Player requires Windows 11 (build 22000 or later)." /SD IDOK
  Abort
supported_windows:
  System::Call 'shell32::IsUserAnAdmin() i.r0'
  ${If} $0 != 0
    StrCpy $InstallScope "Machine"
    StrCmp $PROGRAMFILES64 "" install_unsafe_directory
    StrCpy $InstallRoot "$PROGRAMFILES64\VideoPlayer"
    StrCpy $InstallScopeDescription "$(MachineScopeInfo)"
    SetShellVarContext all
  ${Else}
    StrCpy $InstallScope "User"
    StrCmp $LOCALAPPDATA "" install_unsafe_directory
    StrCpy $InstallRoot "$LOCALAPPDATA\Programs\VideoPlayer"
    StrCpy $InstallScopeDescription "$(UserScopeInfo)"
  ${EndIf}
  StrCpy $INSTDIR "$InstallRoot\versions\${VERSION}-${PAYLOAD_ID}"
  !insertmacro CheckInstallPath "install_"
  Goto install_path_valid
install_unsafe_directory:
  SetErrorLevel 1
  MessageBox MB_ICONSTOP "Refusing to use an unexpected or redirected installation directory." /SD IDOK
  Abort
install_path_valid:
FunctionEnd
Section "Video Player" SEC_PLAYER
  DetailPrint "$InstallScopeDescription"
  IfFileExists "$INSTDIR\payload.complete" payload_ready
  IfFileExists "$INSTDIR\*.*" partial_install
  ClearErrors
  SetOutPath "$INSTDIR"
  IfErrors payload_write_failed
  !insertmacro CheckInstallPath "copy_"
  ClearErrors
  File /r "${STAGE_DIRECTORY}\*.*"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  IfErrors payload_write_failed
  FileOpen $0 "$INSTDIR\install-scope" w
  IfErrors payload_write_failed
  FileWrite $0 "$InstallScope"
  FileClose $0
  IfErrors payload_write_failed
  FileOpen $0 "$INSTDIR\payload.complete" w
  IfErrors payload_write_failed
  FileWrite $0 "${PAYLOAD_ID}"
  FileClose $0
  IfErrors payload_write_failed
payload_ready:
  ClearErrors
  FileOpen $0 "$INSTDIR\install-scope" r
  IfErrors invalid_scope
  FileRead $0 $1
  FileClose $0
  StrCmp $1 $InstallScope 0 invalid_scope
  nsExec::ExecToStack '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\Windows-Integration.ps1" -Action Install -InstallDirectory "$INSTDIR" -Version "${VERSION}" -Scope "$InstallScope"'
  Pop $IntegrationResult
  Pop $0
  DetailPrint "$0"
  ${If} $IntegrationResult != 0
    SetErrorLevel 1
    MessageBox MB_ICONSTOP "Windows integration could not be registered. The registry backup has been preserved. See the installer details." /SD IDOK
    Abort
  ${EndIf}
  CreateShortcut "$SMPROGRAMS\Video Player.lnk" "$INSTDIR\video-player.exe" "" "$INSTDIR\video-player.exe" 0
  Goto installation_done
partial_install:
  SetErrorLevel 1
  MessageBox MB_ICONSTOP "A previous incomplete installation exists at $INSTDIR. Remove that incomplete version before retrying. Active versions are never overwritten." /SD IDOK
  Abort
invalid_scope:
  SetErrorLevel 1
  MessageBox MB_ICONSTOP "This installation has an invalid scope marker. Its files and Windows integration have been retained." /SD IDOK
  Abort
copy_unsafe_directory:
  SetErrorLevel 1
  MessageBox MB_ICONSTOP "The installation directory was redirected during setup. Windows integration has not been changed." /SD IDOK
  Abort
payload_write_failed:
  SetErrorLevel 1
  MessageBox MB_ICONSTOP "Video Player files could not be installed. Windows integration has not been changed. See the installer details." /SD IDOK
  Abort
installation_done:
SectionEnd
Function un.onInit
  SetShellVarContext current
  SetRegView 64
  StrCpy $InstallScope "User"
  ClearErrors
  FileOpen $0 "$INSTDIR\install-scope" r
  IfErrors un_legacy_scope
  FileRead $0 $InstallScope
  FileClose $0
un_legacy_scope:
  ${If} $InstallScope == "Machine"
    StrCpy $InstallRoot "$PROGRAMFILES64\VideoPlayer"
  ${ElseIf} $InstallScope == "User"
    StrCpy $InstallRoot "$LOCALAPPDATA\Programs\VideoPlayer"
  ${Else}
    Goto un_unsafe_directory
  ${EndIf}
  !insertmacro CheckInstallPath "un_"
  IfFileExists "$INSTDIR\payload.complete" 0 un_unsafe_directory
  FileOpen $0 "$INSTDIR\payload.complete" r
  IfErrors un_unsafe_directory
  FileRead $0 $1
  FileClose $0
  StrCmp $1 "${PAYLOAD_ID}" 0 un_unsafe_directory
  ${If} $InstallScope == "Machine"
    SetShellVarContext all
    System::Call 'shell32::IsUserAnAdmin() i.r0'
    ${If} $0 == 0
      StrCpy $ElevationArguments "_?=$INSTDIR"
      IfSilent 0 un_elevate
      StrCpy $ElevationArguments "/S _?=$INSTDIR"
un_elevate:
      ClearErrors
      ExecShell "runas" "$INSTDIR\uninstall.exe" "$ElevationArguments" SW_SHOWNORMAL
      IfErrors un_elevation_failed
      Quit
un_elevation_failed:
      SetErrorLevel 1
      MessageBox MB_ICONSTOP "$(MachineUninstallElevation)" /SD IDOK
      Abort
    ${EndIf}
  ${EndIf}
  Goto un_safe_directory
un_unsafe_directory:
  SetErrorLevel 1
  MessageBox MB_ICONSTOP "Refusing to remove an unexpected or redirected installation directory." /SD IDOK
  Abort
un_safe_directory:
FunctionEnd
Section "Uninstall"
  nsExec::ExecToStack '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\Windows-Integration.ps1" -Action Uninstall -InstallDirectory "$INSTDIR" -Scope "$InstallScope"'
  Pop $IntegrationResult
  Pop $0
  DetailPrint "$0"
  ${If} $IntegrationResult == 0
    Delete "$SMPROGRAMS\Video Player.lnk"
  ${ElseIf} $IntegrationResult != 2
    SetErrorLevel 1
    MessageBox MB_ICONSTOP "Windows integration could not be safely removed. Files and the registry backup are retained for recovery." /SD IDOK
    Abort
  ${EndIf}
  !insertmacro CheckInstallPath "un_remove_"
  RMDir /r "$INSTDIR"
  ${GetParent} "$INSTDIR" $0
  RMDir "$0"
  ${GetParent} "$0" $1
  RMDir "$1"
  Goto un_remove_complete
un_remove_unsafe_directory:
  SetErrorLevel 1
  MessageBox MB_ICONSTOP "The installation directory changed during removal. Remaining files were retained." /SD IDOK
  Abort
un_remove_complete:
  IfFileExists "$INSTDIR\video_player_thumbnail.dll" 0 uninstall_done
  IfSilent uninstall_done
  MessageBox MB_ICONINFORMATION "Explorer still has this version loaded. Integration has been removed. Close Explorer or sign out to release the remaining inactive files." /SD IDOK
uninstall_done:
SectionEnd
