; henry_pet NSIS hooks (Tauri installer template).
; A real uninstall (not the silent uninstall an update runs first) also drops her "Run at startup"
; entry, which the app writes itself in HKCU\...\Run. Settings/bond/bed in %APPDATA%\dev.exaar.henrypet
; go only when the user ticks "Delete the application data" (Tauri's own checkbox).
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "henry_pet"
  ${EndIf}
!macroend
