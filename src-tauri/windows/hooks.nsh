; bilidown Windows 安装器钩子（Tauri NSIS installerHooks）
;
; 背景：默认安装器的「删除应用数据」只清理 `$APPDATA\<bundle-id>`，
; 而 bilidown 的数据目录是 `$LOCALAPPDATA\bilidown-data`（与安装目录分离），
; 这里在用户勾选删除数据时同步删除真实数据目录，避免卸载残留。
!ifndef BILIDOWN_DATA_DIR
  !define BILIDOWN_DATA_DIR "$LOCALAPPDATA\bilidown-data"
!endif
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState == 1
  ${AndIf} $UpdateMode <> 1
    SetShellVarContext current
    RmDir /r "${BILIDOWN_DATA_DIR}"
  ${EndIf}
!macroend
