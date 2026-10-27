; Budget Calendar (Rust port) — NSIS installer script
; Build: makensis /DVERSION=0.2.0 installer\budget.nsi   (run from the repo root after `cargo build --release`)

!ifndef VERSION
  !define VERSION "0.2.0"
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
  File "..\reference\sample.bgt"
  File "..\vendor\webbrowser\src\lib.rs"
  Delete "$INSTDIR\lib.rs"
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
  Delete "$INSTDIR\sample.bgt"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk"
  Delete "$SMPROGRAMS\${APPNAME}\Uninstall.lnk"
  RMDir "$SMPROGRAMS\${APPNAME}"
  DeleteRegKey HKCU "${UNINST_KEY}"
  DeleteRegKey HKCU "Software\Classes\BudgetCalendar.bgt"
  DeleteRegKey HKCU "Software\Classes\.bgt"
SectionEnd
