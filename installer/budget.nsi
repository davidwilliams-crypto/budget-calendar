; Budget Calendar (Rust port) — NSIS installer script
; Build with installer\build-installer.ps1, which checks VERSION against budget-gui/Cargo.toml and packages the
; archived executable and icon. Direct form: makensis /DVERSION=<version> /DFILEVERSION=<a.b.c.0>
; /DPAYLOAD=<archived budget-gui.exe> /DICON_PAYLOAD=<archived budget-calendar.ico> installer\budget.nsi
; OUTPUT_DIR optionally names the folder for the setup program (default ..\target).

!ifndef VERSION
  !error "Pass /DVERSION=<budget-gui version from budget-gui/Cargo.toml> (use installer\build-installer.ps1)"
!endif
!ifndef FILEVERSION
  !error "Pass /DFILEVERSION=<major.minor.patch.0> (use installer\build-installer.ps1)"
!endif
!ifndef PAYLOAD
  !error "Pass /DPAYLOAD=<archived budget-gui.exe> (use installer\build-installer.ps1)"
!endif
!ifndef ICON_PAYLOAD
  !error "Pass /DICON_PAYLOAD=<archived budget-calendar.ico> (use installer\build-installer.ps1)"
!endif
!ifndef OUTPUT_DIR
  !define OUTPUT_DIR "..\target"
!endif
!include "MUI2.nsh"
!include "LogicLib.nsh"
!define APPNAME "Budget Calendar"
!define EXE "BudgetCalendar.exe"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\BudgetCalendarRust"
!define PUBLISHER "Budget Calendar Rust port"

Unicode true
ManifestDPIAware true
Name "${APPNAME}"
Caption "${APPNAME} ${VERSION} Setup"
BrandingText "${APPNAME} ${VERSION}"
OutFile "${OUTPUT_DIR}\BudgetCalendar-${VERSION}-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\BudgetCalendar"
InstallDirRegKey HKCU "${UNINST_KEY}" "InstallLocation"
RequestExecutionLevel user
SetCompressor /SOLID lzma

VIProductVersion "${FILEVERSION}"
VIAddVersionKey "ProductName" "${APPNAME}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "${APPNAME} Setup"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "LegalCopyright" "${PUBLISHER}"

; Pages: welcome, folder, install, finish (with "Start Budget Calendar" and "Desktop shortcut" ticked).
!define MUI_ICON "${ICON_PAYLOAD}"
!define MUI_UNICON "${ICON_PAYLOAD}"
!define MUI_ABORTWARNING
!define MUI_WELCOMEPAGE_TITLE "Welcome to ${APPNAME}"
!define MUI_WELCOMEPAGE_TEXT "This will install ${APPNAME} ${VERSION} for your Windows account. No administrator password is needed.$\r$\n$\r$\nYour budgets stay as files on this PC. Nothing is sent anywhere unless you choose to share a budget.$\r$\n$\r$\nClick Next to continue."
!define MUI_FINISHPAGE_TITLE "${APPNAME} is installed"
!define MUI_FINISHPAGE_TEXT "You can start it any time from the Start menu."
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Start ${APPNAME} now"
; The "show readme" checkbox is reused for the optional desktop shortcut.
!define MUI_FINISHPAGE_SHOWREADME ""
!define MUI_FINISHPAGE_SHOWREADME_TEXT "Put a shortcut on my desktop"
!define MUI_FINISHPAGE_SHOWREADME_FUNCTION CreateDesktopShortcut

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

; Replacing or removing the program while it runs would fail part-way. An open executable cannot be opened
; for writing, so ask the person to close it (their open budget and unsaved work stay theirs to save).
!macro WAIT_FOR_APP_TO_CLOSE
  ${Do}
    ClearErrors
    ${IfNot} ${FileExists} "$INSTDIR\${EXE}"
      ${Break}
    ${EndIf}
    FileOpen $9 "$INSTDIR\${EXE}" a
    ${IfNot} ${Errors}
      FileClose $9
      ${Break}
    ${EndIf}
    ${If} ${Cmd} `MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "${APPNAME} is open. Please save your work and close it, then click Retry." /SD IDCANCEL IDCANCEL`
      Abort
    ${EndIf}
  ${Loop}
!macroend

Function CreateDesktopShortcut
  CreateShortcut "$DESKTOP\${APPNAME}.lnk" "$INSTDIR\${EXE}"
FunctionEnd

Section "Install"
  SetOutPath "$INSTDIR"
  !insertmacro WAIT_FOR_APP_TO_CLOSE
  File "/oname=${EXE}" "${PAYLOAD}"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  CreateDirectory "$SMPROGRAMS\${APPNAME}"
  CreateShortcut "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk" "$INSTDIR\${EXE}"
  CreateShortcut "$SMPROGRAMS\${APPNAME}\Uninstall.lnk" "$INSTDIR\uninstall.exe"

  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${APPNAME}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\${EXE}"
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1

  ; .bgt files open with the app (per-user association). Save the association it replaces, so uninstall
  ; can put it back. PreviousBgtDefaultPresent records whether a default value existed at all.
  ClearErrors
  ReadRegStr $0 HKCU "Software\Classes\.bgt" ""
  ${If} ${Errors}
    StrCpy $0 ""
    StrCpy $1 0
  ${Else}
    StrCpy $1 1
  ${EndIf}
  ; Reinstalling over our own association keeps the value saved by the first install.
  ${If} $0 S!= "BudgetCalendar.bgt"
    WriteRegStr HKCU "${UNINST_KEY}" "PreviousBgtDefault" $0
    WriteRegDWORD HKCU "${UNINST_KEY}" "PreviousBgtDefaultPresent" $1
  ${EndIf}
  WriteRegStr HKCU "Software\Classes\.bgt" "" "BudgetCalendar.bgt"
  WriteRegStr HKCU "Software\Classes\BudgetCalendar.bgt" "" "Budget Calendar file"
  WriteRegStr HKCU "Software\Classes\BudgetCalendar.bgt\DefaultIcon" "" '"$INSTDIR\${EXE}",0'
  WriteRegStr HKCU "Software\Classes\BudgetCalendar.bgt\shell\open\command" "" '"$INSTDIR\${EXE}" "%1"'
  ; Tell Explorer the association changed, so .bgt files show the new icon without a sign-out.
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
SectionEnd

Section "Uninstall"
  !insertmacro WAIT_FOR_APP_TO_CLOSE
  ; Restore the saved .bgt association only while it still points at this installation. The key is
  ; deleted only when nothing else remains in it, so another program's values under .bgt survive.
  ReadRegStr $0 HKCU "${UNINST_KEY}" "PreviousBgtDefault"
  ReadRegDWORD $1 HKCU "${UNINST_KEY}" "PreviousBgtDefaultPresent"
  ReadRegStr $2 HKCU "Software\Classes\.bgt" ""
  ${If} $2 S== "BudgetCalendar.bgt"
    ${If} $1 == 1
      WriteRegStr HKCU "Software\Classes\.bgt" "" $0
    ${Else}
      DeleteRegValue HKCU "Software\Classes\.bgt" ""
    ${EndIf}
  ${EndIf}
  DeleteRegKey /ifempty HKCU "Software\Classes\.bgt"

  ; Only the program is removed. Budgets, backups and settings belong to the person and are never deleted;
  ; RMDir without /r leaves the folder if anything else (such as an older sample budget) is still in it.
  Delete "$INSTDIR\${EXE}"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$DESKTOP\${APPNAME}.lnk"
  Delete "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk"
  Delete "$SMPROGRAMS\${APPNAME}\Uninstall.lnk"
  RMDir "$SMPROGRAMS\${APPNAME}"
  DeleteRegKey HKCU "${UNINST_KEY}"
  DeleteRegKey HKCU "Software\Classes\BudgetCalendar.bgt"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
SectionEnd
