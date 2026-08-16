# Windows Passkey Manager 계획

## 목적

`keeless-passkey-windows`를 Windows WebAuthn Plugin Passkey Manager로 구현한다.
Keeless 데이터베이스에 저장된 passkey를 Edge, Chrome 등 Windows WebAuthn을 이용하는
클라이언트에서 사용할 수 있게 하고, interactive ceremony마다 Windows Hello로 사용자
검증(User Verification, UV)을 수행한다.

Windows Hello는 passkey 개인키나 KDBX master key를 소유하거나 보관하지 않는다. passkey
개인키, 서명, KDBX journal commit의 원본은 계속 Keeless desktop host와 KDBX다.

## 범위

포함:

- Windows 11 WebAuthn Plugin API를 통한 system passkey manager 등록
- classic out-of-process COM local server인 `keeless-passkey-windows.exe`
- `IPluginAuthenticator`의 `MakeCredential`, `GetAssertion`, `CancelOperation`,
  `GetLockStatus` 구현
- 요청과 Windows Hello UV 응답의 cryptographic verification
- 기존 desktop host로의 authenticated local IPC와 passkey Core operation 재사용
- NSIS per-machine COM registration과 enable/disable UI

제외:

- Windows credential metadata cache 및 browser conditional UI/autofill
- Windows Hello만으로 KDBX를 unlock하는 기능
- CTAPHID, virtual HID, USB/NFC/BLE transport
- `hmac-secret`, PRF, largeBlob 등 현재 Core가 구현하지 않은 extension
- Windows 11 24H2 이전 시스템 지원

## UX와 제약

Windows credential metadata cache를 사용하지 않는다. KDBX만 credential metadata와
개인키의 단일 원본이다.

- 사용자는 browser 또는 Windows passkey chooser에서 Keeless를 명시적으로 선택한다.
- Windows의 passkey provider 목록에는 Keeless가 등록되지만, Windows/Chrome/Edge의
  계정 자동완성 및 conditional UI에는 Keeless account가 나타나지 않는다.
- `GetAssertion`의 interactive 요청은 Keeless native UI에서 기존과 같이 계정을 선택한다.
- conditional UI 또는 silent discovery 요청은 prompt나 database unlock을 유발하지 않고
  `NTE_NOT_FOUND`로 끝낸다.
- database가 잠겨 있으면 Windows Hello 뒤에 Keeless native password prompt가 나타날 수
  있다. 이는 Windows Hello를 KDBX unlock key로 오인하지 않기 위한 의도된 동작이다.

나중에 autofill이 필요해질 때만 `WebAuthNPluginAuthenticatorAddCredentials`를 추가한다.
그 시점에는 raw credential ID, RP, user metadata를 Windows cache에 export하고 별도
정합성 모델을 설계한다. 이 MVP에서는 해당 API, metadata 열거 schema, cache reconcile
hook을 추가하지 않는다.

## 기존 구조와 재사용

Linux 구현은 다음 경로를 사용한다.

```text
browser -> virtual CTAPHID -> keeless-passkey-linux
        -> CoreClient -> desktop host -> KeelessCore -> KDBX
```

Windows 경로는 transport만 교체한다.

```text
browser -> Windows WebAuthn service -> COM local server
        -> Windows Hello -> CoreClient -> desktop host -> KeelessCore -> KDBX
```

재사용 대상:

- `packages/host-desktop-shared`: current-user named pipe, encrypted lesswire frame,
  TOFU pairing, `CoreClient`
- `packages/core`: `RegisterPasskey`, `AssertPasskey`, database unlock, native consent,
  journaled mutation, KDBX signing
- `packages/kdbx`: credential storage, CTAP authenticator data와 signature 생성
- `packages/passkey-ctap`: Keeless AAGUID, supported algorithm 상수, Linux용 CTAP codec

Windows COM callback의 request/response byte format은 Linux CTAPHID와 다르다.
`passkey-ctap`의 `parse_command`나 status-byte response를 callback에 직접 사용하지
않는다. Windows SDK의 decode/encode API를 사용한다.

`authenticatorGetInfo`는 plugin 등록에 raw CBOR payload가 필요하다.
`passkey-ctap::response::authenticator_info_cbor`는 status 없는 map을 제공하고,
`response::get_info`만 CTAPHID용 CTAP success status byte를 앞에 붙인다. 등록 blob에는
반드시 전자만 사용한다.

## SDK Binding

### 결정

COM shim이나 C++ bridge는 만들지 않는다. Windows API와 COM server 모두 Rust로
구현한다.

현재 crates.io의 `windows-sys`와 공식 `windows-rs` commit
`5f8e1504dea507f1d86af7bf5a824eb49ff8b5a5`만으로는 완전한 plugin surface가 없다.
그 revision에는 일반 `webauthn` API와 일부 experimental structure가 있지만,
`WebAuthNPlugin*` 함수, CTAP decode/encode API, `IPluginAuthenticator`의 IID/vtable이
없다. 따라서 이 패키지는 `microsoft/webauthn`의 공개 source를 ABI source of truth로
사용한다.

### 생성과 고정

1. `microsoft/webauthn` commit
   `ef82c157125a0490e05f6ea82a7adb1b8e1bad08`를 ABI source로 고정한다.
   `pluginauthenticator.idl`, `pluginauthenticator.h`, `webauthnplugin.h`,
   `webauthn.h`를 입력으로 Rust binding을 생성한다. IDL만으로는 COM callback
   ABI만 정의하며, plugin 함수와 CTAP-CBOR 구조체는 두 header가 필요하다.
2. 생성된 Rust source를 `packages/passkey-windows/src/sdk_bindings.rs`에 commit한다.
   개발자 PC의 Windows SDK path나 network에서 build 때마다 재생성하지 않는다.
3. source 상단에 upstream commit, input file revision, 생성 tool revision과 upstream
   MIT license notice를 기록한다.
4. 재생성 명령과 pinned source 조건은 `packages/passkey-windows/README.md`에 문서화한다.
5. Windows CI는 generated binding을 다시 만들거나 ABI compile check를 수행해,
   source와 pinned upstream input의 drift를 검출한다.

생성 binding에는 최소한 다음이 있어야 한다.

- `IPluginAuthenticator`, IID, vtable, `IClassFactory`
- `WEBAUTHN_PLUGIN_OPERATION_REQUEST`, response, cancellation request, lock status
- `WebAuthNPluginAddAuthenticator`, remove/update/state/status callback API
- `WebAuthNPluginGetOperationSigningPublicKey`
- `WebAuthNPluginPerformUserVerification`, UV public key/count, free API
- `WebAuthNDecodeMakeCredentialRequest`, `WebAuthNDecodeGetAssertionRequest`와 free API
- `WebAuthNEncodeMakeCredentialResponse`, `WebAuthNEncodeGetAssertionResponse`
- request/response와 authenticator-info 구조체 및 constants

지원하지 않는 Windows에서 process loader가 plugin DLL import 때문에 시작 자체에
실패하지 않도록 `webauthn.dll` 함수는 delay-loaded 또는 `LoadLibrary`/
`GetProcAddress`로 해결한다. 생성 binding의 structure와 interface 정의는 그대로
사용하되, 호출용 function table은 처음 사용 시 한 번 해석한다. `doctor`와 enable UI는
DLL, 필요한 export, OS build를 확인한 뒤 지원하지 않는 시스템에서는 명확히 비활성화한다.

### Reference ceremony contract

현재 Windows fixture를 만들 환경이 없으므로 다음 contract는
`yusei36/KeePassPasskey` commit
`08a3e0b13b81ee55929c4e9e0895e7197118b3b6`의 동작을 reference로 삼는다.
해당 프로젝트는 GPL-3.0이므로 source나 test data를 복사하지 않고, 공개 ABI와 아래의
동작만 독립적으로 Rust로 구현한다.

- operation request와 Windows Hello v1 response의 signed bytes는 정확히
  `pbEncodedRequest`의 `cbEncodedRequest` bytes다.
- cancel request는 transaction ID가 아니라 해당 active ceremony에 저장한 원
  `pbEncodedRequest` bytes에 대해 서명 검증한다.
- operation-signing key와 Windows Hello UV key는 callback마다
  `WebAuthNPluginGetOperationSigningPublicKey`와
  `WebAuthNPluginGetUserVerificationPublicKey`로 읽는다.
- public key는 CNG `GenericPublicBlob`으로 import하고 signed bytes의 SHA-256
  digest를 검증한다. RSA는 PSS를 먼저 시도하고 PKCS#1 v1.5를 fallback으로 허용하며,
  EC는 CNG ECDSA verification을 사용한다.
- 알 수 없는 key blob magic, malformed length, 지원하지 않는 CNG public key type은
  허용하지 않는다. RSA/EC fallback은 검증 순서일 뿐 임의 blob format을 허용하는
  fallback이 아니다.

## 패키지 구조

새 Cargo workspace member:

```text
packages/passkey-windows/
  Cargo.toml
  README.md
  src/
    main.rs
    lib.rs
    sdk_bindings.rs       # pinned microsoft/webauthn source에서 생성하여 commit
    api.rs                # dynamically resolved WebAuthnPlugin function table
    com.rs                # class factory, COM lifetime, activation loop
    authenticator.rs      # IPluginAuthenticator callback adapter
    ceremony.rs           # decode -> Hello -> Core operation -> encode
    session.rs            # CoreClient state와 installed desktop launcher
    verify.rs             # request/UV signature verification
    registration.rs       # add/remove/state/status callback
```

`Cargo.toml`은 `keeless_host_desktop_shared`, `keeless_schema`,
`keeless_passkey_ctap`을 사용한다. CNG signature verification과 COM runtime에는
`windows-sys`를 사용하고, generated COM interface가 요구하는 `windows-core`/
`windows` revision도 binding generator와 호환되는 revision으로 고정한다.

모든 Windows-specific source와 dependency는 `cfg(windows)`로 제한한다. Linux에서
workspace test를 실행할 때 새 package가 Windows API 때문에 실패하면 안 된다.

## COM Activation과 등록

### External-location package and NSIS install

plugin은 classic out-of-process COM local server로 배포한다. signed NSIS installer는
external-location MSIX identity package를 `$INSTDIR`에 연결해 stage/provision하고,
manifest의 `windows.comServer` extension이 COM local server를 등록한다. Windows
WebAuthn service는 CLSID로 COM activation을 수행하며, package identity가 포함된
token으로 `keeless-passkey-windows.exe -PluginActivated -Embedding`을 실행한다.

- CLSID `13ABEFF0-71C5-49E3-9F2F-C207A28CDB9D`와 Keeless AAGUID는 release 후 변경하지
  않는다. CLSID는 Rust `COM_CLASS_ID`와 NSIS
  `packages/desktop/build/installer.nsh`에서 동일해야 한다.
- installer는 `keeless-passkey-windows.exe`를 `$INSTDIR\\resources\\bin`에 설치하고,
  manifest의 relative `ExeServer` path로 활성화한다. NSIS는 `HKLM`에 `LocalServer32`
  key를 직접 만들지 않는다.
- `packages/desktop/electron-builder.yml`는 `nsis.perMachine: true`와 elevation을
  요구한다. installer는 `Add-AppxPackage -Stage -ExternalLocation`와
  `Add-AppxProvisionedPackage`로 identity package를 모든 사용자에 provisioning한다.
- server executable과 설치 directory는 administrator만 수정할 수 있어야 하며,
  installer, sidecar, identity MSIX는 code signed여야 한다. identity manifest publisher는
  MSIX signing certificate subject와 정확히 일치해야 한다. user-writable path, config,
  `PATH`는 registration이나 launcher에 사용하지 않는다.
- uninstall은 sidecar의 idempotent `--disable` command로
  `WebAuthNPluginRemoveAuthenticator`를 먼저 호출한다. 성공했을 때만 installer가
  provisioned identity package를 제거하며, disable 실패 시 uninstall을 중단해 stale
  provider를 남기지 않는다.
- `WebAuthNPluginAddAuthenticator`는 calling process의 package identity를 요구한다.
  unsigned MSIX는 SignPath 입력용이며 release installer가 `-AllowUnsigned`로 우회하지
  않는다.

### Activation

`-PluginActivated` process는 COM apartment와 security를 초기화하고 class factory를
`CoRegisterClassObject`로 등록한다. COM이 `LocalServer32` command 뒤에 붙이는
`-Embedding`도 허용한다. Windows가 callback을 호출하는 동안 process와 factory lifetime을
유지한다.

COM boundary의 원칙:

- 어떤 Rust panic도 `extern "system"` callback 밖으로 전파하지 않는다.
- null pointer, byte length, UTF-16 string, enum 값은 사용 전에 검증한다.
- Windows가 caller-owned request buffer를 회수하기 전에 필요한 값만 복사한다.
- response buffer는 SDK가 요구하는 COM allocation 규약으로 할당한다.
- 동시에 하나의 ceremony만 처리한다. 추가 요청은 `ERROR_BUSY` HRESULT로 거절한다.

### 등록 lifecycle

앱 Settings의 explicit enable action이 다음을 수행한다.

1. OS/DLL/export/SDK feature gate를 검사한다.
2. `authenticatorGetInfo` raw CBOR, stable AAGUID, `internal` transport,
   resident key/user presence/user verification capability를 담아
   `WebAuthNPluginAddAuthenticator`를 호출한다.
3. 등록 응답의 operation-signing public key를 secure user state에 저장하거나 각
   callback마다 platform API로 다시 읽는다.
4. Windows Settings의 passkey manager enable state를 조회하고 표시한다.

disable은 plugin registration을 제거하는 action이며 KDBX passkey를 삭제하지 않는다.
metadata cache가 없으므로 remove 시 추가 credential cleanup은 필요 없다.

## Ceremony 처리

### 공통 검증

`MakeCredential`과 `GetAssertion`은 callback 진입 직후 다음 순서로 처리한다.

1. request pointer와 길이를 검증하고 `requestType`이 CTAP CBOR인지 확인한다.
2. `WebAuthNPluginGetOperationSigningPublicKey`의 public key로 `pbEncodedRequest`
   전체의 request signature를 검증한다. 실패하면 decode, Hello, IPC를 수행하지 않고
   실패한다.
3. Windows SDK decoder로 request를 structured data로 해석한다.
4. 알려지지 않은 option, 지원하지 않는 algorithm 또는 요구 extension은 정확한
   `NTE_NOT_SUPPORTED`/invalid-parameter 오류로 끝낸다. 구현하지 않은 기능을
   `authenticatorGetInfo`에 광고하지 않는다.

request signature와 user-verification signature 검증에는 CNG를 사용한다. reference
contract대로 `pbEncodedRequest`의 SHA-256 digest에 대해 RSA-PSS를 먼저, RSA-PKCS#1
v1.5를 다음으로 검증하며 EC key는 ECDSA로 검증한다. CNG `GenericPublicBlob`으로
검증 가능한 RSA/EC public key만 받고, 임의의 public key format이나 oversized buffer를
허용하지 않는다.

### Windows Hello

interactive ceremony는 `WebAuthNPluginPerformUserVerification` v1을 호출한다.

- caller HWND와 transaction ID를 요청에서 가져온다.
- request의 encoded bytes는 v1 API가 반환하는 UV signature의 signed bytes다. UV public
  key를 가져와 반환 signature를 같은 bytes에 대해 검증한다.
- cancel은 `NTE_USER_CANCELLED`로, signature failure는 일반 authentication failure로
  매핑한다.
- Hello display hint는 relying party를 포함한다. account는 아직 Core가 선택하지 않았을
  수 있으므로 user name을 선택적으로만 제공한다.

Windows Hello 검증이 성공하기 전에는 Core에 signing/mutation operation을 보내지
않는다. Hello 결과 자체를 disk에 저장하거나 다음 ceremony에 재사용하지 않는다.

### MakeCredential

1. decoded RP, user handle/name, client-data hash, algorithms, exclude list를
   `RegisterPasskeyArgs`로 변환한다.
2. Windows Hello UV를 완료한다.
3. `CoreClient`를 통해 `Operation::RegisterPasskey`를 보낸다.
4. Core는 기존 정책대로 database unlock, exclude credential 검사, native consent,
   journal commit을 처리한다.
5. commit 성공 후 받은 authenticator data와 credential ID를 Windows SDK response
   structure에 넣고 `WebAuthNEncodeMakeCredentialResponse`로 CBOR를 만든다.
6. encoded buffer를 COM response에 이전한다.

Core mutation이 성공하기 전에 Windows response를 만들거나 성공을 반환하지 않는다.
반대로 encoder 실패 시 이미 commit된 KDBX credential은 유지된다. 사용자는 이후
registration을 다시 시도할 수 있고 Core의 exclude check가 중복 생성을 막는다.

### GetAssertion

metadata cache가 없으므로 browser autofill과 silent discovery는 지원하지 않는다.

1. decoded authenticator option이 user presence를 요구하지 않으면 즉시
   `NTE_NOT_FOUND`를 반환한다. password prompt, native UI, Windows Hello는 열지 않는다.
2. interactive request만 Windows Hello UV를 수행한다.
3. decoded RP ID, client-data hash, allow list를 `AssertPasskeyArgs`로 변환하고
   `user_present: true`로 Core에 보낸다.
4. Core가 KDBX의 실제 credential을 검색하고, 여러 account가 맞으면 native UI에서
   사용자가 선택한다.
5. Core의 authenticator data, signature, credential ID, user handle을 Windows SDK
   assertion response에 넣고 `WebAuthNEncodeGetAssertionResponse`로 응답한다.

allow list가 비어 있는 discoverable interactive request도 지원한다. 이 경우 Core가 RP에
맞는 KDBX credential을 찾고 native UI에서 선택한다. 이는 Windows cache가 아니라 KDBX
조회 결과에만 의존한다.

### IPC, pairing, launcher

COM server는 Linux sidecar와 동일하게 `CoreClient`를 사용한다. client identity는
`passkey-windows-state.json`에 owner-only로 저장하고 desktop host key를 pin한다.

- 최초 연결은 기존 native approval prompt를 거친다.
- desktop host가 꺼져 있으면 installed COM server의 fixed install path를 기준으로
  desktop executable을 해석해 `--minimized`로 시작하고 named pipe가 열릴 때까지
  기다린다.
- production package에서는 user-writable config나 `PATH`로 desktop executable을 찾지
  않는다. 개발 모드에서만 explicit absolute `--desktop` override를 허용한다.
- private key와 decrypted KDBX field는 COM process에 전달하지 않는다.

`GetLockStatus`는 host reachability와 database status를 사용해 `PluginLocked` 또는
`PluginUnlocked`를 보고한다. status 조회는 app을 새로 실행하거나 password prompt를
열지 않는다.

### 취소

`CancelOperation`은 transaction ID가 현재 active ceremony와 일치할 때만 처리한다.

1. active ceremony가 보관한 원 encoded request bytes에 대해 cancellation signature를
   검증한다. 실패하면 cancellation signal이나 IPC 종료를 수행하지 않는다.
2. ceremony cancellation token을 signal한다.
3. pending `CoreClient` request/IPC connection을 drop한다.
4. desktop host는 requester disconnect를 감지해 in-flight native UI child를 종료한다.
5. COM callback은 `NTE_USER_CANCELLED`를 반환한다.

Hello, desktop launch, Core IPC, native UI, response encode 각 단계에서 cancellation을
관찰한다. 취소된 request가 뒤늦게 assertion이나 registration response를 반환하지
않게 transaction ID와 active-operation guard를 사용한다.

## Core와 Schema 변경

metadata cache를 사용하지 않으므로 passkey metadata enumeration operation이나 raw
credential ID export API를 추가하지 않는다.

필요한 최소 변경만 한다.

- Windows adapter가 request를 표현하기에 현재 `RegisterPasskeyArgs`와
  `AssertPasskeyArgs`가 부족한지 확인하고, 필요할 때만 shared semantic field를 추가한다.
- `user_present: true`는 Windows adapter가 signature-verified Hello UV를 마친 뒤에만
  보내며, 현재 Core가 만드는 UP/UV authenticator flags와 일관되게 유지한다.
- 기존 native consent를 우회하지 않는다. client scope나 Windows verification proof를
  Core까지 전달해 consent를 생략하는 설계는 이 작업의 범위 밖이다.
- Windows registration용 raw authenticator-info encoder가 필요하면 `passkey-ctap`에
  transport-independent helper만 추가한다.

## 오류 매핑

Windows-visible failure는 browser가 다른 authenticator로 fallback할 수 있게 적절히
분류한다.

| 상황 | Windows 결과 |
| --- | --- |
| 사용자가 Hello 또는 Keeless UI를 취소 | `NTE_USER_CANCELLED` |
| silent/autofill discovery | `NTE_NOT_FOUND` |
| allow list/RP에 맞는 credential 없음 | `NTE_NOT_FOUND` |
| exclude credential 발견 | duplicate/excluded credential에 대응하는 Windows error |
| 지원하지 않는 algorithm/extension | `NTE_NOT_SUPPORTED` |
| desktop host 없음 또는 pairing 거절 | generic unavailable error |
| request/UV signature 검증 실패 | authentication failure, 세부 정보 비노출 |
| 두 ceremony 동시 도착 | `ERROR_BUSY` |

Core operation error를 Windows HRESULT로 변환하는 mapping은 한 함수에 모으고 모든
mapping을 unit test한다. 사용자-facing error에 KDBX path, client identity, cryptographic
detail을 포함하지 않는다.

## 테스트와 완료 기준

### 자동 테스트

- Windows-independent: operation field 변환, Core error mapping, cancellation state machine,
  raw authenticator-info CBOR fixture
- Windows native: generated binding compile check, COM reference/lifetime test, malformed
  pointer/length rejection, CNG RSA-PSS/RSA-PKCS#1/EC request와 Hello signature
  verification test
- package test: dynamic export lookup이 필요한 모든 symbol을 확인하고 unsupported OS에서는
  process가 정상적으로 unsupported result를 반환하는지 검증

### Windows VM 통합 테스트

지원 build의 깨끗한 Windows 11 VM에서 다음을 수행한다.

1. signed per-machine NSIS installer를 설치하고 Settings에서 Keeless provider를 enable한다.
2. Edge와 Chrome에서 Keeless를 명시적으로 선택해 passkey 등록과 assertion을 수행한다.
3. database unlocked/locked/paranoia mode에서 각각 Hello, native password, native account
   selection의 순서를 확인한다.
4. Hello cancel, native UI cancel, browser cancel, desktop 종료, daemon reconnect, pairing
   reset을 검증한다.
5. request signature 또는 UV response signature를 변조한 test path가 Core signing 전에
   실패하는지 확인한다.
6. input field의 conditional/autofill UI에 Keeless credential이 나타나지 않는 것을
   확인한다. 이는 의도된 cache-free 동작이다.
7. enable/disable, app upgrade, uninstall/reinstall 뒤 CLSID activation과 registration
   state를 확인한다.

### 완료 조건

- Windows 11 24H2 build `26100.6725+` 또는 25H2 build `26200.6725+`에서 위 integration
  test가 통과한다.
- 지원하지 않는 Windows에서는 app/sidecar가 loader failure 없이 기능을 비활성화한다.
- request와 UV signature 검증 실패 시 Core operation, password prompt, native consent가
  한 번도 실행되지 않는다.
- passkey 개인키가 KDBX/Core process 밖으로 복사되지 않는다.
- Windows metadata cache API가 어떤 code path에서도 호출되지 않는다.
