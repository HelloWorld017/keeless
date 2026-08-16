; Classic COM registration and external package identity for the Windows
; WebAuthn plugin. This file is referenced by electron-builder's nsis.include.

!define KEELESS_PASSKEY_WINDOWS_CLSID "{13ABEFF0-71C5-49E3-9F2F-C207A28CDB9D}"
!define KEELESS_PASSKEY_WINDOWS_SERVER "$INSTDIR\resources\bin\keeless-passkey-windows.exe"
!define KEELESS_PASSKEY_WINDOWS_PACKAGE "$INSTDIR\resources\passkey-windows\keeless-passkey-windows.msix"
!define KEELESS_PASSKEY_WINDOWS_PACKAGE_NAME "dev.nenw.keeless.passkey"
!define KEELESS_PASSKEY_WINDOWS_KEY "Software\Classes\CLSID\${KEELESS_PASSKEY_WINDOWS_CLSID}"
!define POWERSHELL "$SYSDIR\WindowsPowerShell\v1.0\powershell.exe"

!macro customInstall
  IfFileExists "${KEELESS_PASSKEY_WINDOWS_SERVER}" 0 missing_passkey_server
  IfFileExists "${KEELESS_PASSKEY_WINDOWS_PACKAGE}" 0 missing_identity_package

  ; The external package grants identity to the installed sidecar without
  ; moving its executable into an MSIX-owned installation directory.
  ExecWait '$\"${POWERSHELL}$\" -NoLogo -NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -Command $\"Add-AppxPackage -Path ''${KEELESS_PASSKEY_WINDOWS_PACKAGE}'' -ExternalLocation ''$INSTDIR'' -ForceUpdateFromAnyVersion$\"' $0
  StrCmp $0 0 register_com_server
  Abort "Keeless passkey package identity could not be installed. The application was not installed."

 register_com_server:
  SetRegView 64
  WriteRegStr HKLM "${KEELESS_PASSKEY_WINDOWS_KEY}\LocalServer32" "" '$\"${KEELESS_PASSKEY_WINDOWS_SERVER}$\" -PluginActivated'
  WriteRegStr HKLM "${KEELESS_PASSKEY_WINDOWS_KEY}\LocalServer32" "ServerExecutable" "${KEELESS_PASSKEY_WINDOWS_SERVER}"
  Goto passkey_identity_done

 missing_passkey_server:
  Abort "Keeless passkey server is missing from this installer."

 missing_identity_package:
  Abort "Keeless passkey package identity is missing."

 passkey_identity_done:
!macroend

!macro customUnInstall
  SetRegView 64
  IfFileExists "${KEELESS_PASSKEY_WINDOWS_SERVER}" 0 remove_identity_package

  ; --disable must be idempotent and call WebAuthNPluginRemoveAuthenticator
  ; before removing the package identity that authorizes the API call.
  ExecWait '$\"${KEELESS_PASSKEY_WINDOWS_SERVER}$\" --disable' $0
  StrCmp $0 0 remove_identity_package
  Abort "Keeless passkey provider could not be disabled. The application remains installed."

 remove_identity_package:
  ExecWait '$\"${POWERSHELL}$\" -NoLogo -NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -Command $\"Get-AppxPackage -Name ''${KEELESS_PASSKEY_WINDOWS_PACKAGE_NAME}'' | Remove-AppxPackage$\"' $0
  StrCmp $0 0 remove_com_registration
  Abort "Keeless passkey package identity could not be removed. The application remains installed."

 remove_com_registration:
  ReadRegStr $0 HKLM "${KEELESS_PASSKEY_WINDOWS_KEY}\LocalServer32" "ServerExecutable"
  StrCmp $0 "${KEELESS_PASSKEY_WINDOWS_SERVER}" 0 done
  DeleteRegKey HKLM "${KEELESS_PASSKEY_WINDOWS_KEY}"

 done:
!macroend
