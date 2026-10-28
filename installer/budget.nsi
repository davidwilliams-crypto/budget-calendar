; Budget Calendar (Rust port) — NSIS installer script
; Build: makensis /DVERSION=<budget-gui version from Cargo.toml> installer\budget.nsi   (run from the repo root after `cargo build --release`)

!ifndef VERSION
  !error "Pass /DVERSION=<budget-gui version from budget-gui/Cargo.toml>"
!endif
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
  File "/oname=${EXE}" "..\target\release\budget-gui.exe"
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

  ; .bgt files open with the app (per-user association)
  WriteRegStr HKCU "Software\Classes\.bgt" "" "BudgetCalendar.bgt"
  WriteRegStr HKCU "Software\Classes\BudgetCalendar.bgt" "" "Budget Calendar file"
  WriteRegStr HKCU "Software\Classes\BudgetCalendar.bgt\shell\open\command" "" '"$INSTDIR\${EXE}" "%1"'
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\${EXE}"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk"
  Delete "$SMPROGRAMS\${APPNAME}\Uninstall.lnk"
  RMDir "$SMPROGRAMS\${APPNAME}"
  DeleteRegKey HKCU "${UNINST_KEY}"
  DeleteRegKey HKCU "Software\Classes\BudgetCalendar.bgt"
  DeleteRegKey HKCU "Software\Classes\.bgt"
SectionEnd
