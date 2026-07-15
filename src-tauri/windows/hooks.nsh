!macro NSIS_HOOK_POSTINSTALL
  ; Remove context-menu entries left by pre-rename LightSync builds.
  DeleteRegKey HKCU "Software\Classes\*\shell\LightSync"
  DeleteRegKey HKCU "Software\Classes\Directory\shell\LightSync"

  WriteRegStr HKCU "Software\Classes\*\shell\GitSyncTools" "" "同步到 GitSyncTools"
  WriteRegStr HKCU "Software\Classes\*\shell\GitSyncTools" "Icon" "$INSTDIR\GitSyncTools.exe"
  WriteRegStr HKCU "Software\Classes\*\shell\GitSyncTools" "MultiSelectModel" "Player"
  WriteRegStr HKCU "Software\Classes\*\shell\GitSyncTools\command" "" '$\"$INSTDIR\GitSyncTools.exe$\" --publish $\"%1$\"'

  WriteRegStr HKCU "Software\Classes\Directory\shell\GitSyncTools" "" "同步到 GitSyncTools"
  WriteRegStr HKCU "Software\Classes\Directory\shell\GitSyncTools" "Icon" "$INSTDIR\GitSyncTools.exe"
  WriteRegStr HKCU "Software\Classes\Directory\shell\GitSyncTools" "MultiSelectModel" "Player"
  WriteRegStr HKCU "Software\Classes\Directory\shell\GitSyncTools\command" "" '$\"$INSTDIR\GitSyncTools.exe$\" --publish $\"%1$\"'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DeleteRegKey HKCU "Software\Classes\*\shell\GitSyncTools"
  DeleteRegKey HKCU "Software\Classes\Directory\shell\GitSyncTools"
!macroend
