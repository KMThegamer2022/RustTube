; RustTube Windows installer (NSIS). Build with:
;   makensis /DVERSION=0.1.0 /DSRCDIR=<dir with rusttube.exe + libmpv-2.dll> installer.nsi
Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"

!ifndef VERSION
  !define VERSION "0.1.0"
!endif
!ifndef SRCDIR
  !define SRCDIR "..\..\dist-win"
!endif

Name "RustTube"
OutFile "RustTube-${VERSION}-windows-x64-setup.exe"
InstallDir "$PROGRAMFILES64\RustTube"
InstallDirRegKey HKLM "Software\RustTube" "InstallDir"
RequestExecutionLevel admin
SetCompressor /SOLID lzma

!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\rusttube.exe"
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "Install"
  SetOutPath "$INSTDIR"
  File "${SRCDIR}\rusttube.exe"

  ; RustTube plays video in the Microsoft Edge WebView2 runtime (preinstalled on
  ; Windows 11 and current Windows 10). Install it if it is missing.
  ReadRegStr $0 HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" "pv"
  ${If} $0 == ""
    ReadRegStr $0 HKCU "Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" "pv"
  ${EndIf}
  ${If} $0 == ""
  ${OrIf} $0 == "0.0.0.0"
    DetailPrint "Installing Microsoft Edge WebView2 runtime..."
    NSISdl::download "https://go.microsoft.com/fwlink/p/?LinkId=2124703" "$TEMP\MicrosoftEdgeWebview2Setup.exe"
    Pop $1
    ${If} $1 == "success"
      ExecWait '"$TEMP\MicrosoftEdgeWebview2Setup.exe" /silent /install'
    ${Else}
      MessageBox MB_OK|MB_ICONEXCLAMATION "Could not download the WebView2 runtime. Install it from https://developer.microsoft.com/microsoft-edge/webview2/ and start RustTube again."
    ${EndIf}
  ${EndIf}
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  CreateDirectory "$SMPROGRAMS\RustTube"
  CreateShortcut "$SMPROGRAMS\RustTube\RustTube.lnk" "$INSTDIR\rusttube.exe"
  CreateShortcut "$DESKTOP\RustTube.lnk" "$INSTDIR\rusttube.exe"

  WriteRegStr HKLM "Software\RustTube" "InstallDir" "$INSTDIR"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RustTube" "DisplayName" "RustTube"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RustTube" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RustTube" "Publisher" "RustTube"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RustTube" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RustTube" "DisplayIcon" "$INSTDIR\rusttube.exe"
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RustTube" "NoModify" 1
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RustTube" "NoRepair" 1
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\rusttube.exe"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\RustTube\RustTube.lnk"
  RMDir "$SMPROGRAMS\RustTube"
  Delete "$DESKTOP\RustTube.lnk"
  DeleteRegKey HKLM "Software\RustTube"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RustTube"
  ; User data (profiles, history) in %APPDATA%\RustTube is left in place.
SectionEnd
