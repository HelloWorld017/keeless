; Package identity and COM registration for the Windows WebAuthn plugin.
; This file is referenced by electron-builder's nsis.include setting.

!define KEELESS_PASSKEY_WINDOWS_SERVER "$INSTDIR\resources\bin\keeless-passkey-windows.exe"
!define KEELESS_PASSKEY_WINDOWS_PACKAGE "$INSTDIR\resources\passkey-windows\keeless-passkey-identity.msix"
!define KEELESS_PASSKEY_WINDOWS_PACKAGE_SCRIPT "$INSTDIR\resources\passkey-windows\manage-identity-package.ps1"
!define KEELESS_PASSKEY_WINDOWS_PACKAGE_NAME "dev.nenw.keeless.passkey"
!define POWERSHELL "$SYSDIR\WindowsPowerShell\v1.0\powershell.exe"

!macro customInstall
  IfFileExists "${KEELESS_PASSKEY_WINDOWS_SERVER}" 0 missing_passkey_server
  IfFileExists "${KEELESS_PASSKEY_WINDOWS_PACKAGE}" 0 missing_identity_package
  IfFileExists "${KEELESS_PASSKEY_WINDOWS_PACKAGE_SCRIPT}" 0 missing_identity_package

  ; COM registration comes from the signed package manifest. This gives the
  ; local server package identity when Windows activates it.
  ExecWait '$\"${POWERSHELL}$\" -NoLogo -NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File $\"${KEELESS_PASSKEY_WINDOWS_PACKAGE_SCRIPT}$\" -Action Install -PackageName ${KEELESS_PASSKEY_WINDOWS_PACKAGE_NAME} -PackagePath $\"${KEELESS_PASSKEY_WINDOWS_PACKAGE}$\" -ExternalLocation $\"$INSTDIR$\"' $0
  StrCmp $0 0 passkey_identity_done
  Abort "Keeless passkey package identity could not be installed. The application was not installed."

 missing_passkey_server:
  Abort "Keeless passkey server is missing from this installer."

 missing_identity_package:
  Abort "Keeless passkey package identity is missing."

 passkey_identity_done:
!macroend

!macro customUnInstall
  IfFileExists "${KEELESS_PASSKEY_WINDOWS_SERVER}" 0 remove_identity_package

  ; --disable must be idempotent and call WebAuthNPluginRemoveAuthenticator
  ; before the package manifest removes its COM class registration.
  ExecWait '$\"${KEELESS_PASSKEY_WINDOWS_SERVER}$\" --disable' $0
  StrCmp $0 0 remove_identity_package
  Abort "Keeless passkey provider could not be disabled. The application remains installed."

 remove_identity_package:
  IfFileExists "${KEELESS_PASSKEY_WINDOWS_PACKAGE_SCRIPT}" 0 missing_identity_package_script
  ExecWait '$\"${POWERSHELL}$\" -NoLogo -NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File $\"${KEELESS_PASSKEY_WINDOWS_PACKAGE_SCRIPT}$\" -Action Uninstall -PackageName ${KEELESS_PASSKEY_WINDOWS_PACKAGE_NAME}' $0
  StrCmp $0 0 identity_package_removed
  Abort "Keeless passkey package identity could not be removed. The application remains installed."

 missing_identity_package_script:
  Abort "Keeless passkey package identity management script is missing. The application remains installed."

 identity_package_removed:
!macroend
