; Budget Calendar (Rust port) — NSIS installer script
; Build with installer\build-installer.ps1, which checks VERSION against budget-gui/Cargo.toml and packages the
; archived executable. Direct form: makensis /DVERSION=<version> /DPAYLOAD=<archived budget-gui.exe> installer\budget.nsi

!ifndef VERSION
  !error "Pass /DVERSION=<budget-gui version from budget-gui/Cargo.toml> (use installer\build-installer.ps1)"
!endif
!ifndef PAYLOAD
  !error "Pass /DPAYLOAD=<archived budget-gui.exe> (use installer\build-installer.ps1)"
!endif
!include "LogicLib.nsh"
!define APPNAME "Budget Calendar"
!define EXE "BudgetCalendar.exe"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\BudgetCalendarRust"

Unicode true
Name "${APPNAME} ${VERSION}"
OutFile "..\target\BudgetCalendar-${VERSION}-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\BudgetCalendar"
InstallDirRegKey HKCU "${UNINST_KEY}" "InstallLocation"
RequestExecutionLevel user
SetCompressor /SOLID lzma

Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

Section "Install"
  SetOutPath "$INSTDIR"
  File "/oname=${EXE}" "${PAYLOAD}"
  ; Sample budget is user-editable: install only if absent, never delete on uninstall.
  IfFileExists "$INSTDIR\sample.bgt" +2 0
  File "..\reference\sample.bgt"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  CreateDirectory "$SMPROGRAMS\${APPNAME}"
  CreateShortcut "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk" "$INSTDIR\${EXE}"
  CreateShortcut "$SMPROGRAMS\${APPNAME}\Uninstall.lnk" "$INSTDIR\uninstall.exe"

  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${APPNAME}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "Budget Calendar Rust port"
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
  WriteRegStr HKCU "Software\Classes\BudgetCalendar.bgt\shell\open\command" "" '"$INSTDIR\${EXE}" "%1"'
SectionEnd

Section "Uninstall"
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

  Delete "$INSTDIR\${EXE}"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk"
  Delete "$SMPROGRAMS\${APPNAME}\Uninstall.lnk"
  RMDir "$SMPROGRAMS\${APPNAME}"
  DeleteRegKey HKCU "${UNINST_KEY}"
  DeleteRegKey HKCU "Software\Classes\BudgetCalendar.bgt"
SectionEnd
