; Classic COM registration for the Windows WebAuthn plugin local server.
; This file is referenced by electron-builder's nsis.include setting.

!define KEELESS_PASSKEY_WINDOWS_CLSID "{13ABEFF0-71C5-49E3-9F2F-C207A28CDB9D}"
!define KEELESS_PASSKEY_WINDOWS_SERVER "$INSTDIR\resources\bin\keeless-passkey-windows.exe"
!define KEELESS_PASSKEY_WINDOWS_KEY "Software\Classes\CLSID\${KEELESS_PASSKEY_WINDOWS_CLSID}"

!macro customInstall
  ; The plugin only supports 64-bit Windows 11. This is also the registry view
  ; used for native x64 and ARM64 COM activation.
  SetRegView 64
  IfFileExists "${KEELESS_PASSKEY_WINDOWS_SERVER}" 0 skip_passkey_server

  ; The command is quoted because COM appends -Embedding when it activates a
  ; LocalServer32 process. ServerExecutable prevents command-path ambiguity.
  WriteRegStr HKLM "${KEELESS_PASSKEY_WINDOWS_KEY}\LocalServer32" "" '$\"${KEELESS_PASSKEY_WINDOWS_SERVER}$\" -PluginActivated'
  WriteRegStr HKLM "${KEELESS_PASSKEY_WINDOWS_KEY}\LocalServer32" "ServerExecutable" "${KEELESS_PASSKEY_WINDOWS_SERVER}"

skip_passkey_server:
!macroend

!macro customUnInstall
  SetRegView 64
  IfFileExists "${KEELESS_PASSKEY_WINDOWS_SERVER}" 0 remove_com_registration

  ; --disable must be idempotent and call WebAuthNPluginRemoveAuthenticator
  ; before this COM class can be removed.
  ExecWait '$\"${KEELESS_PASSKEY_WINDOWS_SERVER}$\" --disable' $0
  StrCmp $0 0 remove_com_registration
  Abort "Keeless passkey provider could not be disabled. The application remains installed."

remove_com_registration:
  ; Delete the class only if this installation owns the configured server.
  ReadRegStr $0 HKLM "${KEELESS_PASSKEY_WINDOWS_KEY}\LocalServer32" "ServerExecutable"
  StrCmp $0 "${KEELESS_PASSKEY_WINDOWS_SERVER}" 0 done
  DeleteRegKey HKLM "${KEELESS_PASSKEY_WINDOWS_KEY}"

done:
!macroend
