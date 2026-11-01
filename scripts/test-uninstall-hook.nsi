Unicode true
Name "bilidown uninstall hook test"
OutFile "${TEST_OUTFILE}"
RequestExecutionLevel user
SilentInstall silent

!include "LogicLib.nsh"
!include "FileFunc.nsh"

Var DeleteAppDataCheckboxState
Var UpdateMode

!define BILIDOWN_DATA_DIR "$TEMP\bilidown-uninstall-hook-test"
!include "..\src-tauri\windows\hooks.nsh"

Section "Test"
  SetShellVarContext current
  RmDir /r "${BILIDOWN_DATA_DIR}"
  CreateDirectory "${BILIDOWN_DATA_DIR}"
  FileOpen $0 "${BILIDOWN_DATA_DIR}\queue.sqlite" w
  FileWrite $0 "uninstall-hook-test"
  FileClose $0
  FileOpen $0 "${BILIDOWN_DATA_DIR}\settings.json" w
  FileWrite $0 "{}"
  FileClose $0

  StrCpy $DeleteAppDataCheckboxState 0
  StrCpy $UpdateMode 0
  ${GetOptions} $CMDLINE "/MODE=" $1
  ${If} $1 == "delete"
    StrCpy $DeleteAppDataCheckboxState 1
  ${ElseIf} $1 == "update"
    StrCpy $DeleteAppDataCheckboxState 1
    StrCpy $UpdateMode 1
  ${EndIf}

  !insertmacro NSIS_HOOK_POSTUNINSTALL
  SetErrorLevel 0
  Quit
SectionEnd
