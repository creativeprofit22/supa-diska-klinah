; Supa Diska Klinah NSIS hooks (bundle.windows.nsis.installerHooks).
; Reviewed by scripts/check-security-boundaries.mjs; keep this file minimal.

; Before files are removed on a real uninstall, remove the app's scheduled scans
; (\SupaDiskaKlinah task folder, all users) through the bundled helper, which
; still sits beside the app at this point. The uninstaller is per-machine and
; already elevated. Updates run the uninstall step with /UPDATE and must keep
; the user's schedules, so removal is skipped then. A failure never blocks the
; uninstall; release acceptance checks that the folder is gone.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    ${If} ${FileExists} "$INSTDIR\supa-diska-klinah-privileged-helper.exe"
      ExecWait '"$INSTDIR\supa-diska-klinah-privileged-helper.exe" --remove-scheduled-tasks'
    ${EndIf}
  ${EndIf}
!macroend
