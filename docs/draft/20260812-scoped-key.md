## 하려고 하는 것
- Key 별로 Scope를 나눔
    - `core_untrusted` `core` `app` `passkey` `extension`
    - ? -> `core_untrusted`: `unlock`, `open`, `upgrade`, `getCoreStatus` 요청만 보낼 수 있음
    - `app` -> `core`: 모든 요청 보낼 수 있음
    - `passkey` -> `core`: 패스키 관련 요청만 보낼 수 있음

- `core_untrusted` 와의 연결:
    "이 연결을 허용하시겠습니까?" 다이얼로그로 `core_untrusted` 연결을 수립함
    - 제한된 정보만 제공함
    - `getCoreStatus` 는 `getDatabaseStatus` 와 동일하나 훨씬 제한된 정보 ({ database: DatabaseStatus }) 만 전달함

- `core` 와의 연결 (NEW):
    - 데이터베이스의 잠금이 풀리면 `core` lesswire가 새로 열림
    - 이 후 `core_untrusted` 에게 `upgrade` 요청을 보냄
    - "이 데이터베이스에 접근하는 것을 허용하시겠습니까?" 다이얼로그로 `core_untrusted`를 `core` 로 업그레이드

## 이렇게 바꾸는 이유
- Core의 키 보안 강화
    - As-Is: Core의 기기 키가 파일에 저장됨
        - 해당 파일에 접근할 수 있는 주체는 내용을 해석할 수 있음
    - To-Be: Untrusted의 기기 키만 파일에 평문으로 저장되고, Core의 키는 암호화된 상태로 저장됨
        - Core의 키는 per-database로, 데이터베이스의 마스터 비밀번호에서 파생된 키로 암호화되어 저장됨

- Persisted Approval은 Runtime Approval에 비해 취약함
    - 메모리에만 위치하는 Runtime Approval에 비해 파일시스템의 state에 위치하는 Persisted Approval은 키가 약탈되기 쉬움
    - As-Is: Passkey 의 Persisted Approval이 털리면 전체 데이터베이스를 덤프할 수 있음
    - To-Be: Passkey 의 Persisted Approval이 털려도 passkey 요청만을 보낼 수 있음  
      (fyi. 전체 권한을 가지는 `app` 키는 대부분 Runtime Approval로 사용되고 있음)

## 구현 디테일
- MessageFrame에 recipient 추가
    - Core로 보내는지 Untrusted로 보내는지 구분할 수 있게 됨

- Per-Database Config Storage 추가
    - 현재 사용하는 DesktopConfig또한 이걸 따르도록
    - 데이터베이스를 처음 Open할 때 마스터 비밀번호에서 파생된 비밀번호로 해독
    - Core 키도 여기에 저장

- Schema 정의시에 Sender와 Recipient도 같이 정의하게 수정
  - Sender Policy와 Recipient Policy에 맞지 않는 요청은 drop

- Key Bundle의 구조를 변경 `v1.signingKey.encryptionKey.(app|core|core_untrusted|extension|passkey)`
    - Approve 시에 scope를 같이 표시해주기
        - `app` scope 시에는 추가적인 경고문구도 추가 (모든 데이터베이스에 접근 가능하다는)

- 아직 앱이 배포되지 않았기에 버전을 올리거나 v1을 v2로 마이그레이션 할 필요는 없음

## 계획

### 목표 아키텍처

- Lesswire identity에는 필수 scope가 포함된다. canonical public key bundle 형식은
  `v1.<ed25519-public-key>.<x25519-public-key>.<scope>`이며, scope는
  `core_untrusted`, `core`, `app`, `passkey` 중 하나다. `extension`은 현재 구현하거나
  정책에 포함하지 않는다.
- 모든 `MessageFrame`은 `recipient`에 **수신 서버의 정확한 public key bundle**을 담는다.
  `recipient`는 signature transcript와 payload AEAD AAD에 포함되어, frame을 다른 서버로
  전환하거나 수신 대상을 바꾸면 검증에 실패한다.
- 데스크톱과 브라우저 host는 두 Lesswire server를 가진다.
  - `core_untrusted`: 항상 존재하며 자체 identity와 persisted approval을 평문 host state에
    저장한다. `open`, `create`, `unlock`, `getCoreStatus`, `upgrade`만 처리한다.
  - `core`: 선택된 데이터베이스가 unlock된 동안에만 존재한다. identity와 persisted
    approval은 데이터베이스별 암호화 state에만 저장하며, lock 또는 DB 전환 때 즉시
    제거한다.
- `app` client는 `core_untrusted`에 연결해 DB 선택과 create/unlock을 수행한다. 성공 후
  `upgrade`로 현재 `core`의 public key를 받고, 그 key를 recipient로 하는 새 handshake를
  수행해 `core`에 연결한다.
- `passkey` client는 persisted `passkey` identity로 `core`에만 연결한다. core server의
  persisted approval이 유출되어도 key bundle scope와 operation policy 때문에 passkey
  operation 외의 요청은 보낼 수 없다.
- `KeelessConfig`, core Lesswire identity, core persisted approval은 데이터베이스별 state의
  일부다. master password로 만든 KDBX raw key에서 database ID와 purpose별 HKDF context를
  사용해 암호화 키를 파생하고, XChaCha20-Poly1305로 저장한다. raw KDBX key를 저장 키로
  직접 사용하지 않는다.

### 권한 행렬

| Sender scope | Recipient | 허용 operation |
| --- | --- | --- |
| `app` | `core_untrusted` | `open`, `create`, `unlock`, `getCoreStatus`, `upgrade` |
| `app` | `core` | 모든 core operation |
| `passkey` | `core` | `getPasskeys`, `registerPasskey`, `assertPasskey` |

- 행렬에 없는 sender/recipient/operation 조합은 operation error를 반환하지 않고 payload
  실행 전에 drop한다.
- `core_untrusted`에서는 binary transfer packet을 허용하지 않는다. `core`의 transfer는
  scope를 포함한 인증 sender bundle에 귀속한다.
- `getCoreStatus`의 응답은 정확히 `{ database: DatabaseStatus }`다. 기존
  `getDatabaseStatus`의 `syncStatus`, `dirty`, `syncError`, storage 정보는 untrusted
  endpoint에 노출하지 않는다.

### API 및 모델 변경

#### Lesswire

- `packages/lesswire/src/lib.rs`와 `packages/lesswire/src/index.ts`에 `KeyScope`를 추가하고,
  `Identity::public_key_bundle`, `PublicKeyBundle::parse`, TypeScript `parseBundle`이 scope를
  포함한 canonical bundle만 생성/수용하도록 바꾼다.
- `MessageFrame`에 `recipient: string`을 추가한다. Rust와 TypeScript의
  `transcript`, `headerTranscript`, handshake, encrypt/decrypt, frame-size 계산을 동시에
  갱신한다.
- `ServerHost`와 `Server`는 자신의 identity bundle을 recipient로 선언한다. 인증 단계에서
  `frame.recipient == server.public_key_bundle()`를 검사하고, 일치하지 않는 frame은
  복호화와 approval 전에 drop한다.
- 초기 신뢰 수립을 위해 `Relay.connect`/`Host.connect`는 대상 server의 advertised public
  key bundle을 반환한다. client는 이 bundle을 최초 handshake의 `recipient`로 사용하고,
  handshake 응답의 sender가 그 bundle과 일치하는지 검증한 후 recipient별로 pin한다.
  advertised 값 자체는 TOFU bootstrap이며, 이후에는 저장된 pin과 불일치하면 실패한다.
- `ClientStore`의 trusted-server key는 `relay ID + recipient bundle` 또는 수신 endpoint
  식별자로 분리한다. 한 relay에서 untrusted와 DB별 core key를 독립적으로 pin해야 한다.
- `ApprovalProvider::approve`는 단순 sender bundle 대신 sender bundle, sender scope,
  recipient bundle, recipient scope, approval 종류(`initial`, `upgrade`)를 담은
  `ApprovalRequest`를 받는다.
- `Server`에는 host가 upgrade 후 사용할 persisted approval API를 추가한다. 이 API는
  bundle/scope를 검증하고, UI 승인 후 encrypted state store에 approval을 원자적으로
  저장한다. 다음 core handshake는 이 approval을 읽어 dialog 없이 진행한다.
- Lesswire persisted state의 core identity와 approval 형식은 기존 `v1` state와 호환할
  필요가 없다. 아직 배포 전이므로 개발 state는 새 형식으로 재생성한다.

#### Schema와 dispatch

- `packages/schema/src/lib.rs`에 schema 공용 `KeyScope` 또는 operation policy용 scope type을
  정의한다. `OperationMetadata`와 `OperationDefinition`에는 기존 resource metadata 외에
  sender/recipient policy를 넣는다.
- `packages/schema/src/operation_schema.rs` macro가 각 operation 선언에서
  `senders: [...]`, `recipients: [...]` 또는 동등한 허용 행렬을 받도록 확장한다. 모든
  operation이 명시적인 정책을 가지도록 compile-time 생성 경로를 단일화한다.
- 다음 operation을 추가한다.
  - `GetCoreStatus("getCoreStatus") {}` -> `CoreStatusResult { database: DatabaseStatus }`
  - `Upgrade("upgrade") {}` -> `UpgradeResult { public_key: String }`
- `upgrade`는 `app -> core_untrusted`에서만 허용한다. core server가 아직 없으면 error가
  아니라 drop 또는 명시적 locked 상태 중 하나로 고정한다. 구현에서는 `getCoreStatus`로
  unlock 여부를 확인한 뒤 호출하므로, 권장 동작은 untrusted operation error
  `database_locked`를 반환하는 것이다. 정책 위반 자체만 drop한다.
- TypeScript schema exporter(`packages/schema/src/bin/export-types.rs`)가 새 operation,
  result, scope-aware metadata를 내보내게 하고 `packages/schema/index.ts`를 재생성한다.
- `packages/core/src/network.rs`의 `handle_payload_from`는 transfer owner만이 아니라
  authenticated sender scope와 recipient bundle/scope를 입력으로 받는다. JSON request를
  파싱한 뒤 `Operation::metadata()`의 policy를 검사하고, 통과한 요청만
  `operations::execute`에 전달한다.
- `upgrade`는 DB key/approval UI/core server lifecycle을 알아야 하므로 일반 core mutation이
  아닌 host-owned lifecycle operation으로 dispatch한다. host router가 request ID와 schema
  policy를 보존해 `UpgradeResult` response를 만들고, 나머지 untrusted operation은 core
  network dispatch에 전달한다.

#### 데이터베이스별 암호화 state

- `packages/core/src/host.rs`에 선택된 database namespace에서 opaque record를 bounded
  read/write하는 `DatabaseStateStore` abstraction을 추가한다. 기존 `DatabasePersistence`
  의 `select`와 동일한 `DatabaseId` namespace를 사용해 cache, journal, encrypted state가
  같은 DB에 귀속되게 한다.
- 논리 record는 최소 다음 둘로 나눈다.
  - `config`: `KeelessConfig`를 담는 core-owned encrypted record
  - `core-wire-state`: `Server` identity와 core persisted approval을 담는
    lesswire-owned encrypted record
- Core는 create/unlock 중 KDBX raw key가 확보된 뒤 `database ID`를 salt로 하여
  database-state root key를 파생한다. `config`과 `core-wire-state`에는 서로 다른 HKDF
  info를 사용하고, AEAD AAD에는 format version, database ID, record name을 포함한다.
- Core가 host에 raw key를 노출하지 않도록, `KeelessCore`는 core-wire record 전용으로
  domain-separated `DatabaseSessionKey`를 만든 뒤 `DatabaseSessionProvider::activate`에
  전달한다. 데스크톱/브라우저 구현은 이 key를 소유하는 `EncryptedStateStore`를 만들고
  그것을 core `Server`의 `StateStore`로 사용한다.
- `KeelessCore::new`는 더 이상 전역 `core-settings.json`을 settings source로 사용하지
  않는다. DB가 선택되기 전에는 기본 settings를 사용하며, create/unlock 성공 과정에서
  encrypted `config`를 생성/복원한다. `setConfig`는 현재 활성 DB의 encrypted config만
  갱신한다.
- `create`와 `unlock`은 encrypted config/core-wire state를 성공적으로 활성화한 뒤에만
  unlocked 상태가 된다. 저장 실패, ciphertext 변조, 크기 초과, 잘못된 key, 잘못된
  database ID는 fail-closed로 처리하고 core server를 만들지 않는다.
- `create`에서 KDBX 파일 생성은 encrypted state 초기화보다 먼저 완료될 수 있다. 그 뒤
  state 저장이 실패하면 create는 error를 반환하고 core server를 열지 않는다. 이후의
  `unlock`은 올바른 credentials로 state가 없는 기존 DB를 감지해 새 encrypted config와
  core-wire state를 초기화할 수 있어야 한다. 따라서 부분 실패가 `database_already_exists`
  오류만 남겨 recovery를 막지 않는다.
- `lock`, auto-lock, `open`에 의한 DB 전환, shutdown은 core server, transfer registry,
  `DatabaseSessionKey`, 복호화된 config와 approval을 함께 drop/zeroize한다.
- `packages/host-desktop/src/persistence.rs`의 `PersistenceFile`/`DatabaseStore`에 bounded,
  owner-only, atomic replace와 directory fsync를 갖는 state record를 추가한다. 기존
  `DesktopConfig`는 `core_untrusted` wire state 같은 전역 비밀이 아닌 state만 저장하는
  단순 file store로 축소한다.
- browser는 IndexedDB에서 `DatabaseId`별 record key를 사용해 같은 logical record를
  구현한다. local-file, IndexedDB, WebDAV descriptor 모두 core가 이미 만드는 stable
  `DatabaseId(provider + path)` namespace를 사용한다.

#### Host lifecycle과 upgrade

- `packages/host-desktop/src/lib.rs`의 `InnerState`는 단일 `server` 대신
  `untrusted_server: Server`와 `core_server: Option<Server>`를 가진다. untrusted state는
  host 시작 시 열고, core server는 `DatabaseSessionProvider::activate`가 create/unlock
  성공 뒤 연다.
- frame routing은 JSON payload가 아니라 authenticated `frame.recipient`로 먼저 결정한다.
  - untrusted server bundle과 일치하면 untrusted policy path로 보낸다.
  - 활성 core server bundle과 일치하면 core policy path로 보낸다.
  - 그 외에는 즉시 drop한다.
- `open`, `create`, `unlock`, `getCoreStatus`는 untrusted server를 통해 같은
  `KeelessCore` instance로 전달한다. create/unlock 후 core server가 열렸더라도 기존
  untrusted handshake는 계속 제한된 operation만 보낼 수 있다.
- `upgrade` request를 받으면 host는 활성 core server가 있는지 확인하고
  `ApprovalRequest { kind: Upgrade, sender: app bundle, recipient: core bundle }` dialog를
  표시한다. 승인되면 해당 app bundle을 core server의 encrypted persisted approval에
  추가하고 `UpgradeResult { publicKey: core bundle }`을 untrusted connection으로 반환한다.
  거부되어도 untrusted approval과 연결은 유지한다.
- core server의 transfer registry만 `DesktopTransfers`에 연결한다. untrusted server에는
  transfer provider를 제공하지 않으며, lock과 DB 전환 시 registry를 clear한다.
- Electron의 `desktop:register-client`/`replaceClient` runtime approval 경로는 제거한다.
  renderer host의 `connect`는 untrusted server bundle만 받아 오고, 첫 signed handshake가
  `core_untrusted`의 native approval dialog를 연다. 이로써 desktop app도 다른 local
  client와 같은 initial approval 흐름을 따른다. `upgrade`의 core approval은 항상
  per-database encrypted persisted approval으로 저장한다.
- `packages/host-browser/src/browser.rs`도 동일하게 untrusted/core server를 분리한다.
  BrowserCore 생성 시 untrusted server만 열고, create/unlock callback에서 per-database
  encrypted state store를 사용해 core server를 열며, `handle`은 exact recipient bundle로
  route한다. 이로써 desktop과 browser가 동일한 app unlock/upgrade 경로를 사용한다.

#### App과 passkey client

- `packages/app/src/utils/request/request.ts`의 `RequestClient`는 untrusted client와
  optional core client를 관리한다. app identity는 `app` scope bundle로 고정하고, 각
  recipient server key는 별도로 pin한다.
- Open 화면(`packages/app/src/fragments/open/OpenFragment.tsx`)은 unlock 전
  `getDatabaseStatus` 대신 `getCoreStatus`를 호출한다. `open`, `create`, `unlock`은
  untrusted client로 보내고, create/unlock 성공 뒤 자동으로 `upgrade`를 호출한다.
  반환받은 `publicKey`를 대상으로 core handshake가 성공한 뒤 database 화면으로 이동한다.
- DB 화면의 일반 요청은 core client로만 보낸다. core 연결이 lock/host restart 때문에
  거부되면 untrusted client로 상태를 다시 확인하고 unlock 화면으로 돌아간다.
- `Host`, desktop preload/main bridge, browser host adapter의 `connect` API는 bootstrap
  recipient bundle 반환을 반영한다. relay는 caller가 만든 frame의 recipient를 변경하지
  않고 그대로 전달한다.
- `packages/host-desktop-shared/src/state.rs`는 sender identity scope와 recipient별 pinned
  server key를 저장하도록 바꾼다. `CoreClient`는 생성 시 sender scope와 target recipient를
  명시적으로 받는다.
- Linux/Windows passkey `Session`은 `passkey` sender scope와 `core` target만 사용한다.
  operation API를 `getPasskeys`, `registerPasskey`, `assertPasskey`로 제한하거나, 최소한
  이 세 operation 외 요청을 client와 server 양쪽에서 거부한다.
- native UI의 connection request에는 scope, exact recipient fingerprint, approval kind를
  넣는다. dialog는 initial untrusted 연결과 DB core upgrade를 구분해 설명하고, `app`
  scope 승인에는 "모든 데이터베이스에 접근할 수 있음" 경고를 명시한다.

### 구현 순서

1. **공용 protocol 모델을 먼저 변경한다.**
   - Lesswire Rust/TypeScript의 scoped bundle, exact recipient, transcript/AAD, recipient별
     trust pinning을 한 변경으로 완성한다.
   - lesswire debug CLI, native UI one-shot response path, frame serde 타입, README를 새
     frame/bundle 형식에 맞춘다.
   - 이 단계에서 wrong recipient, recipient 변조, unknown scope, unscoped bundle,
     replay/transfer owner 회귀를 protocol test로 고정한다.

2. **Schema policy와 제한된 untrusted operation을 정의한다.**
   - operation schema macro와 exporter를 확장하고, 기존 모든 operation에 명시적 policy를
     작성한다.
   - `getCoreStatus`, `upgrade` request/response와 policy를 추가한다.
   - core network dispatch에서 policy 위반 drop을 구현하고, policy가 operation 실행 전에
     적용되는지 test한다.

3. **per-database encrypted state 기반을 만든다.**
   - Core의 database session/state store abstraction, key derivation, encrypted config
     codec, lock zeroization lifecycle을 구현한다.
   - Desktop persistence에 state record를 추가하고, Browser IndexedDB에도 동일한
     database-ID record store를 추가한다.
   - create/unlock/relock/DB 전환 간 identity 복원과 isolation을 unit/integration test로
     검증한다.

4. **두 server lifecycle과 host-owned upgrade를 구현한다.**
   - desktop host부터 untrusted/core routing, activation callback, transfer 분리,
     persisted approval upgrade를 구현한다.
   - native approval protocol/UI를 scope-aware로 변경하고, app 경고 문구와 upgrade
     안내를 추가한다.
   - 같은 lifecycle abstraction을 browser host에 연결해 DB별 core server를 열고 닫는다.

5. **모든 caller를 새 연결 흐름으로 전환한다.**
   - app은 `core_untrusted -> create/unlock -> upgrade -> core` 상태 전환을 관리한다.
   - desktop Electron bridge가 bootstrap bundle 반환과 scoped relay를 전달하게 수정한다.
   - passkey Linux/Windows는 `passkey -> core` client로 전환하고 persisted state/pin을
     recipient별로 갱신한다.

6. **보안 회귀와 end-to-end flow를 검증한다.**
   - 새 client는 untrusted 승인 후 제한된 요청만 가능해야 한다.
   - unlock은 core identity를 생성/복원하지만 global untrusted state에 core secret을
     쓰지 않아야 한다.
   - upgrade 승인 후에는 app이 core에 재handshake하고, 재시작 후에도 같은 DB core에
     dialog 없이 연결되어야 한다. 다른 DB에는 다시 upgrade approval이 필요하다.
   - lock/auto-lock/DB 전환 후 이전 core recipient, approval, transfer가 모두 무효인지
     확인한다.
   - passkey persisted identity가 세 허용 passkey operation만 수행하고 entry/config/export
     요청은 응답 없이 drop되는지 확인한다.

### 테스트 및 완료 조건

- `packages/lesswire/src/tests.rs`와 TypeScript tests:
  - scoped bundle canonical parsing/serialization
  - recipient가 signature/AAD에 결속됨
  - 잘못된 recipient, 변조된 recipient, 잘못된 scope, unscoped bundle drop
  - recipient별 key pin과 persisted approval 복원
  - untrusted transfer 거부 및 core transfer owner isolation
- `packages/schema/tests/json.rs`와 core network tests:
  - 모든 operation definition이 sender/recipient policy를 가짐
  - 권한 행렬 전체와 policy 위반의 pre-execution drop
  - `getCoreStatus` 응답에 제한된 `database` 필드만 존재
  - `upgrade` request/response JSON 및 TypeScript exporter 최신성
- core/host persistence tests:
  - DB별 config/core identity 생성, relock 후 메모리 제거, 재unlock 후 동일 identity 복원
  - database ID 간 state 분리, wrong key/corrupt/oversized state fail-closed
  - 전역 untrusted state와 DB별 encrypted state를 검사해 core secret의 평문 저장 부재 확인
  - DB 전환, auto-lock, shutdown이 active core server와 transfer를 정리함
- desktop/browser/passkey integration tests:
  - initial untrusted connect -> open/create/unlock -> upgrade -> core reconnect -> lock
  - upgrade 거부 시 untrusted endpoint는 계속 사용 가능하고 core endpoint는 사용 불가
  - DB별 persisted core approval과 browser의 동일 lifecycle
  - Linux/Windows passkey client의 scoped pairing 및 권한 제한
- 최종 검증은 `cargo test --workspace`, schema export check, `pnpm check`,
  `pnpm typecheck`, `pnpm test`를 모두 통과하는 것으로 한다.
