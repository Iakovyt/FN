!macro FN_STOP_RUNNING_PROCESSES
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /IM fn-app.exe'
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /IM winws.exe'
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /IM TgWsProxy_headless.exe'
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /IM TgWsProxy_windows.exe'
  Sleep 500
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro FN_STOP_RUNNING_PROCESSES
!macroend

!macro NSIS_HOOK_POSTINSTALL
  DetailPrint "Installing Microsoft Visual C++ Runtime..."
  IfFileExists "$INSTDIR\resources\prerequisites\VC_redist.x64.exe" vc_runtime_present 0
    MessageBox MB_ICONSTOP "Microsoft Visual C++ Runtime is missing from the FN installer."
    Abort

  vc_runtime_present:
  ExecWait '"$INSTDIR\resources\prerequisites\VC_redist.x64.exe" /install /quiet /norestart' $0
  StrCmp $0 "0" vc_runtime_ready
  StrCmp $0 "1638" vc_runtime_ready
  StrCmp $0 "3010" vc_runtime_ready
    MessageBox MB_ICONSTOP "Microsoft Visual C++ Runtime installation failed with code $0."
    Abort

  vc_runtime_ready:
  DetailPrint "Preparing FN components..."
  CreateDirectory "$APPDATA\FN\tgws"
  CopyFiles /SILENT "$INSTDIR\resources\tgws\TgWsProxy_headless.exe" "$APPDATA\FN\tgws\TgWsProxy_headless.exe"
  IfFileExists "$APPDATA\FN\tgws\TgWsProxy_headless.exe" tgws_component_ready 0
    MessageBox MB_ICONSTOP "FN could not prepare the TGWS component."
    Abort

  tgws_component_ready:
  IfFileExists "$APPDATA\FN\zapret\bin\winws.exe" fn_components_ready 0
  CreateDirectory "$APPDATA\FN\zapret"
  nsExec::ExecToLog '"$SYSDIR\xcopy.exe" "$INSTDIR\resources\zapret\*" "$APPDATA\FN\zapret" /E /I /H /Y /Q'
  IfFileExists "$APPDATA\FN\zapret\bin\winws.exe" fn_components_ready 0
    MessageBox MB_ICONSTOP "FN could not prepare the Zapret/WinDivert component."
    Abort

  fn_components_ready:
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro FN_STOP_RUNNING_PROCESSES
  nsExec::ExecToLog '"$SYSDIR\schtasks.exe" /Delete /TN "FN Autostart" /F'
!macroend
