; Let other devices reach Aonin through Windows Firewall, so hosting works
; without a prompt (on any network profile: Aonin only talks on the LAN).
; Only takes effect when installed for all users, which runs elevated.
!macro NSIS_HOOK_POSTINSTALL
  nsExec::Exec 'netsh advfirewall firewall add rule name="Aonin" dir=in action=allow program="$INSTDIR\aonin.exe" enable=yes profile=any'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::Exec 'netsh advfirewall firewall delete rule name="Aonin" program="$INSTDIR\aonin.exe"'
!macroend
