**주요 문제**

1. **Critical** `packages/core/src/network.rs:47-72`  
   core endpoint로 `lock` 요청을 처리하면 `lock::run`이 `core_server`를 제거하지만, `handle_frame`이 이전 server를 다시 넣습니다. lock 뒤에도 core key, approval, transfer registry가 살아납니다.

2. **High** `packages/core/src/network.rs:47-68`  
   server를 `take()`한 뒤 wire/approval 저장 오류가 나면 restore 전에 `?`로 반환합니다. 이후 untrusted server가 `None`이 되고 `untrusted_public_key_bundle()`가 panic합니다.

3. **High** `packages/app/src/utils/request/request.ts:41-100`  
   `RequestClient`가 untrusted client를 보관하지 않고 `upgrade()`에서 같은 `wire`를 core client로 교체합니다. Core 연결이 lock/restart로 끊겼을 때 설계 문서의 “untrusted로 상태 확인 후 unlock 화면 복귀”가 불가능합니다. untrusted/core를 별도 필드로 보관해야 합니다.

4. **High** `packages/lesswire/src/index.ts:169-210`  
   recipient bundle 자체를 trusted-server 저장 key로 사용합니다. advertised key가 바뀌면 새로운 저장 key를 조회하므로 pin mismatch를 절대 감지하지 못하고 새 TOFU 연결로 승인됩니다. pin key는 relay/endpoint처럼 안정적인 식별자여야 합니다.

5. **High** `packages/app/src/utils/request/request.ts:9-21`  
   app identity가 renderer 메모리 random key입니다. 새로고침마다 sender bundle이 바뀌고 untrusted/core persisted approval이 계속 새로 생성됩니다. 매 reload마다 initial + upgrade dialog가 필요하며 approval state도 누적됩니다.

6. **Medium** `packages/schema/src/operation_schema.rs:23-24, 203-204`  
   `senders`/`recipients`가 optional이고 빠지면 자동으로 `app -> core`입니다. 문서의 “모든 operation에 명시적인 policy”와 반대이며, 새 operation 추가 시 권한이 묵시적으로 부여됩니다.

7. **Medium** `packages/core/src/lib.rs:462-490`, `packages/core/src/network.rs:8-18`  
   production Core에 test/direct-embedding용 `VolatileStateStore`, global `ConfigProvider` fallback, 공개 unrestricted `handle_payload_from`가 남았습니다. encrypted DB state와 Core-owned wire boundary를 도입한 뒤에도 두 lifecycle과 두 dispatch path가 공존해 구조가 복잡합니다.

8. **Medium** `packages/core/src/operations/upgrade.rs`, `packages/lesswire/src/lib.rs`  
   `ApprovalKind::Upgrade`와 Lesswire `ApprovalRequest`는 만들었지만, upgrade는 Lesswire approval 경로를 통하지 않고 Core가 host provider를 직접 호출합니다. `ApprovalKind::Upgrade`는 현재 dead API에 가깝고 approval 흐름이 둘로 분리됐습니다.

9. **Medium, 요구사항 해석 필요** `packages/schema/src/lib.rs`의 untrusted 정책  
   `passkey` sender도 `open/create/unlock/upgrade`를 보낼 수 있습니다. 문서의 권한 행렬에는 부합하지만, passkey가 “세 passkey operation만” 사용한다는 목표와는 충돌합니다. 악의적 passkey key는 현재 DB 선택을 바꾸거나 lock을 유발할 수 있습니다.

10. **Medium** `packages/app/src/fragments/open/OpenFragment.tsx:68-91`  
   자동 upgrade를 detached promise로 호출합니다. upgrade 거부/실패는 outer `.catch`에 전달되지 않아 checking 화면이 error 없이 끝날 수 있습니다.

**테스트 점검**

- API 변경에 맞춘 기존 fixture 수정은 대부분 필요했습니다.
- `packages/lesswire/src/tests.rs`의 `recipient_is_bound_before_approval_and_payload_decryption`은 recipient를 서명 후 변경합니다. 서명 실패만 검증하므로 recipient 선검사/approval 미호출을 입증하지 못합니다.
- `packages/native-ui/src/tests.rs`의 `rejects_unknown_fields_and_unsafe_labels`는 더 이상 label을 검사하지 않고 invalid scope를 검사합니다. 이름과 목적을 바꾸거나 label validation 테스트를 별도로 정리해야 합니다.
- `packages/host-desktop-shared/src/state.rs` 테스트는 canonical하지 않은 `"v1.server.app"`을 pin으로 저장합니다. pin API 경계의 bundle 검증을 테스트하지 못합니다.
- 빠진 핵심 회귀 테스트:
  - `unlock -> upgrade -> core reconnect -> lock` 후 이전 core frame drop
  - approval/store 오류 후 server가 복원되는지
  - recipient pin replacement 거부
  - app reload identity/approval 동작
  - upgrade 거부 시 Open 화면 오류 표시
  - browser IndexedDB v1 state와 기존 desktop wire state의 의도된 reset 처리

**인터페이스 변경 브리핑**

- Lesswire Rust:
  - `Identity::{generate, from_secrets, from_bytes}`가 `KeyScope`를 요구합니다.
  - bundle은 `v1.<ed25519>.<x25519>.<scope>`가 됐습니다.
  - `MessageFrame.recipient`가 필수입니다.
  - `ApprovalProvider::approve`는 `ApprovalRequest`를 받습니다.
  - `ServerHost`에 `scope`, `allow_transfers`가 추가됐습니다.
  - `Server::handle_frame` callback은 `TransferOwner` 대신 `AuthenticatedSender`를 받습니다.
  - `Client::new`는 정확한 recipient bundle이 필수입니다.

- Lesswire TypeScript:
  - `Relay.connect()`는 sender bundle을 받지 않고 advertised recipient bundle을 반환합니다.
  - `Client.connect(relay, scope, recipient?, store?)` 형태로 바뀌었습니다.
  - `ClientStore` trusted-server API가 recipient 인자를 추가했습니다. 현재 pin key 설계는 수정이 필요합니다.

- Core/host:
  - `KeelessHost`에 `untrusted_state`, `connection_approval`이 추가됐습니다.
  - `DatabasePersistence`에 named state record read/write가 추가됐습니다.
  - host는 `core.handle_frame(raw_bytes)`만 호출하고 server 선택/JSON dispatch를 하지 않도록 바뀌었습니다.
  - Desktop `replaceClient`와 Electron `desktop:register-client`는 제거되고 bootstrap bundle을 반환하는 `connect`로 변경됐습니다.
  - Browser `BrowserCore.create()`는 기본 승인 bundle을 더 받지 않으며 `connect()`를 추가했습니다.

- App:
  - `Host.connect()`는 `Promise<void>`에서 `Promise<string>`으로 바뀌었습니다.
  - `getDatabaseStatus` pre-unlock 호출은 `getCoreStatus`로 전환됐습니다.
  - create/unlock 뒤 `upgrade` 후 core recipient로 재연결합니다.

우선 수정 순서는 lock/server restore, app의 dual client 구조, pin key 설계, policy macro 강제화가 적절합니다.

