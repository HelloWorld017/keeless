https://github.com/HelloWorld017/keeless이 프로젝트에서 passkey-windows 작업을 하고 있어.

현재는 `WebAuthNPluginAuthenticatorAddCredentials`  등으로 별도 credential 관리를 하지 않고 있는데, 그러다보니 윈도우에서 패스키를 못찾는 문제가 발생해서 수정하려고.

1. core의 operation 중에서 `get_passkeys` 를 수정
   1. core의 `PasskeyExtension` 이 불필요해보일 경우 `get_passkeys` 자체를 수정
   2. 여전히 필요해보일 경우 `get_passkeys` 를 `get_passkeys_metadata` 로 바꾸고 새로 `get_passkeys`를 만들기
2. passkey-windows에서 paranoia mode를 쓸 때 과도하게 비밀번호를 물어보는 문제를 방지하기 위해 `create_password_session` 사용
3. passkey-windows 프로세스가 열려서 database를 열 때 윈도우와 credentials를 동기화.

이렇게 수정하기 위한 구체적인 실행계획을 작성해줘.


----


현재 `master` 기준으로 보면, **`PasskeyExtension`은 유지하고 기존 `get_passkeys`를 `get_passkeys_metadata`로 옮긴 뒤, Windows credential cache 동기화에 필요한 정보를 반환하는 새로운 `get_passkeys`를 만드는 방향**이 가장 자연스럽습니다.

현재 `get_passkeys`는 `visible_credentials()`를 통해 후보를 찾은 뒤 `PasskeySummary`만 반환합니다. 반면 `assert_passkey`도 실제 credential을 복호화하기 전에 같은 `visible_credentials()` 경로로 `allowCredentialIds`를 필터링합니다. `PasskeyExtension`은 이때 raw credential ID를 메모리에 보관하지 않고 salted hash index만 유지하는 역할을 합니다. 따라서 이것을 없애면 특히 paranoia mode에서 후보 탐색 단계부터 credential 복호화가 필요해질 가능성이 큽니다. ([GitHub][1])

또 Windows credential cache에는 최소한 `credentialId`, `rpId`, `rpName`, `userId`가 필요하고, 추가로 `userName`, `userDisplayName`을 전달할 수 있습니다. 현재 summary의 `entry_id/rp_id/username`만으로는 부족합니다. Microsoft API도 이 데이터를 `WEBAUTHN_PLUGIN_CREDENTIAL_DETAILS`로 넘겨 `WebAuthNPluginAuthenticatorAddCredentials` 등에 등록하도록 되어 있습니다. ([GitHub][2])

## 권장 실행계획

1. **먼저 core API를 `metadata 조회`와 `credential 조회`로 분리합니다.**

   * `packages/core/src/operations/get_passkeys.rs`의 현재 구현을 `get_passkeys_metadata.rs`로 이동합니다.
   * schema의 기존 `GetPasskeysArgs / PasskeysResult`도 용도에 맞춰 `GetPasskeysMetadataArgs / PasskeysMetadataResult` 등으로 바꾸고 `Operation::GetPasskeysMetadata`를 추가합니다.
   * 기존 동작은 그대로 유지합니다.

     * `rp_id`
     * `allow_credential_ids`
     * visible entry filtering
     * `PasskeyExtension`을 이용한 credential-ID filtering
     * `PasskeySummary` 반환
   * 기존 `GetPasskeys` 호출부를 전부 `GetPasskeysMetadata`로 변경하고 generated TS/schema도 함께 갱신합니다.
   * **`PasskeyExtension`은 제거하지 않습니다.** `assert_passkey`에서 복호화 전에 `allowCredentialIds`를 거르는 용도로 계속 필요하기 때문입니다. ([GitHub][1])
   * 새 `GetPasskeys`는 Windows API에 직접 종속된 타입이 아니라 core 공용 DTO를 반환하게 합니다. 대략 다음 정도면 충분합니다.

```rust
GetPasskeysArgs {
    password: Option<String>,
    password_session: Option<String>,
}

PasskeyCredentialInfo {
    entry_id: ...,
    credential_id: String,       // base64url
    rp_id: String,
    rp_name: String,
    user_id: String,             // base64url
    user_name: String,
    user_display_name: String,
}

GetPasskeysResult {
    credentials: Vec<PasskeyCredentialInfo>,
}
```

* 여기서 **private key나 signing secret은 절대로 반환하지 않습니다.** Windows가 필요로 하는 것은 credential metadata뿐입니다. `credential_id`와 `user_id`처럼 KDBX에서 보호된 정보를 꺼내는 데 필요한 수준까지만 CompositeKey를 사용합니다.
* 새 `get_passkeys`는 entry마다 unlock하지 말고 **operation 시작 시 CompositeKey를 한 번 얻은 뒤 전체 passkey를 순회**합니다. paranoia mode에서 N개의 passkey 때문에 N회 password prompt가 발생하는 구조를 피해야 합니다.
* `rp_name`이나 `user_display_name`이 기존 KDBX representation에서 optional이라면 Windows 필수 필드 때문에 각각 `rp_id`, `user_name`으로 fallback하는 규칙을 core 쪽에서 확정해 두는 편이 좋습니다. Windows ABI에서는 credential ID, RP ID, RP name, user ID가 필수입니다. ([GitHub][2])

2. **passkey 관련 secret 접근 경로 전체를 `password_session`을 받을 수 있게 정리합니다.**

   * 새 `GetPasskeysArgs`만 session을 받게 만들면 절반만 해결됩니다. 현재 `AssertPasskey`는 최종 credential을 읽기 직전에 `passkeys::unlock(core, PasswordInputMode::Reveal)`을 호출하므로 paranoia mode에서는 assertion 때 다시 password prompt가 발생합니다. ([GitHub][3])
   * 따라서 다음 operation들의 args를 점검하고 적어도 passkey-windows에서 사용하는 secret-access operation에는 `password_session: Option<String>`을 추가합니다.

     * `GetPasskeys`
     * `AssertPasskey`
     * `RegisterPasskey`/make-credential 경로
     * `Unlock`은 이미 가지고 있으므로 그대로 사용
   * 가능하면 `features/passkeys.rs`의 key 획득 코드를 하나로 묶습니다. 개념적으로:

```text
explicit password
    ↓
password_session
    ↓
cached core credential (normal mode)
    ↓
interactive password request
```

* 다만 **passkey-windows가 session을 넘긴 경우에는 절대로 다시 UI prompt로 fallback하지 않는 것**을 권장합니다. expired/invalid session이면 `InvalidPasswordSession`을 그대로 돌려주고, sidecar가 session을 새로 만든 뒤 1회 재시도해야 prompt의 소유권과 횟수를 통제하기 쉽습니다.
* `CreatePasswordSession`은 현재 password가 생략되면 `PasswordInputMode::Session`으로 한 번 입력받고, password를 암호화한 token을 만들며 TTL은 60초입니다. lock 시 session도 revoke됩니다. 즉 passkey-windows의 짧은 unlock → sync → ceremony burst를 묶는 용도로 잘 맞습니다. ([GitHub][4])

3. **`passkey-windows::Session`이 password session을 메모리에서 관리하도록 변경합니다.**

   * 현재 `Session`은 사실상 `state`, `client`, `launcher`만 가지고 있습니다. 여기에 다음 필드를 추가합니다. ([GitHub][5])

```rust
password_session: Option<String>
```

* 디스크의 `passkey-windows-state.json`에는 절대 저장하지 않습니다.
* 다음 경우 즉시 `None`으로 버립니다.

  * IPC/Core 연결이 끊김
  * pairing reset
  * DB lock이 감지됨
  * `InvalidPasswordSession`
  * 새 password session 생성
* `ensure_password_session(client)` 같은 helper를 만들고:

  * 유효한 token이 있으면 재사용
  * 없으면 `CreatePasswordSession { password: None }`
  * 결과 token을 `Session`에 보관
* `InvalidPasswordSession`을 받은 operation은 **session 폐기 → 새 session 생성 → 딱 한 번 재시도**하게 합니다. 무한 재시도는 금지합니다.
* 60초 TTL을 sidecar에서 별도로 정확히 추적할 필요는 없습니다. Core를 source of truth로 두고 `InvalidPasswordSession`으로 갱신해도 충분합니다. ([GitHub][4])

4. **현재 `ensure_connected()`의 locked DB 흐름부터 session을 사용하도록 바꿉니다.**

   * 지금은 bootstrap endpoint에서 DB가 `Locked`이면 곧바로 다음 요청을 합니다.

```rust
UnlockArgs {
    password: None,
    password_session: None,
}
```

이 때문에 해당 연결마다 독립적인 password prompt가 생길 수 있습니다. ([GitHub][5])

* 변경 후 locked path는 다음 순서로 만듭니다.

```text
bootstrap connect
  → GetCoreStatus
  → Locked
  → CreatePasswordSession(password=None)   // password prompt 1회
  → Unlock(password_session=token)
  → Upgrade
  → trusted CoreClient connect
  → GetPasskeys(password_session=같은 token)
  → Windows credential sync
```

* `CreatePasswordSession`이 현재 bootstrap/untrusted endpoint의 허용 operation이 아니라면 endpoint allowlist도 함께 수정해야 합니다. 이건 구현 전에 확인해야 할 첫 번째 integration point입니다.
* DB가 이미 `Unlocked`인 경우에는 무조건 `CreatePasswordSession`부터 호출하지 않는 편이 좋습니다. normal mode에서도 불필요한 prompt가 생기기 때문입니다. 추천 흐름은:

  * trusted connection 생성
  * session 없이 `GetPasskeys`
  * cached CompositeKey로 처리 가능하면 그대로 완료
  * paranoia 때문에 credential이 필요하다는 명확한 에러가 오면 `CreatePasswordSession`을 한 번 생성하고 retry
* 이렇게 하면 **normal mode는 추가 prompt 0회, paranoia mode는 sync/ceremony burst당 최초 1회**를 목표로 잡을 수 있습니다.

5. **Windows WebAuthn credential-cache ABI를 `passkey-windows`에 추가합니다.**

   * 현재 `sdk_bindings.rs`는 스스로 “cache-free Keeless provider가 사용하는 ABI만 포함한다”고 명시하고 있으므로 이 부분부터 확장해야 합니다. ([GitHub][6])
   * `sdk_bindings.rs`에 Microsoft header와 동일한 ABI를 추가합니다.

     * `WEBAUTHN_PLUGIN_CREDENTIAL_DETAILS`
     * `WebAuthNPluginAuthenticatorAddCredentials`
     * `WebAuthNPluginAuthenticatorRemoveCredentials`
     * `WebAuthNPluginAuthenticatorRemoveAllCredentials`
     * `WebAuthNPluginAuthenticatorGetAllCredentials`
     * `WebAuthNPluginAuthenticatorFreeCredentialDetailsArray`
   * `api.rs`에서도 기존 방식과 동일하게 `webauthn.dll` symbol을 runtime resolve합니다. Microsoft ABI에는 위 add/remove/get/free API가 모두 정의되어 있습니다. ([GitHub][2])
   * FFI 처리는 `credential_cache.rs` 같은 별도 모듈로 분리하는 것을 추천합니다.

```text
Core PasskeyCredentialInfo
        ↓
WindowsCredentialDetails (owned Rust)
        ↓
UTF-16 / pointer backing storage
        ↓
WEBAUTHN_PLUGIN_CREDENTIAL_DETAILS
```

* `GetAllCredentials`가 돌려준 pointer는 필요한 값을 Rust-owned 데이터로 복사한 직후 반드시 `WebAuthNPluginAuthenticatorFreeCredentialDetailsArray`로 반환합니다. ([GitHub][2])

6. **DB를 source of truth로 두는 idempotent `sync_credentials()`를 구현합니다.**

   * 첫 구현부터 `RemoveAllCredentials → AddCredentials(all)`로 만들 수도 있지만 권하지 않습니다. Add 단계가 실패하면 Windows cache 전체가 비어 버리고 매 startup마다 불필요하게 전체 cache가 churn합니다.
   * 대신:

```text
DB credentials        Windows cached credentials
       │                       │
       └──── canonicalize ─────┘
                  ↓
                diff
          ┌───────┼────────┐
          ↓       ↓        ↓
        add    replace    remove
```

* 비교 key는 우선 `credential_id`로 잡고, value로 `rp_id/rp_name/user_id/user_name/user_display_name` 전체를 비교합니다.
* 분류는:

  * Windows에 없고 DB에 있음 → add
  * 둘 다 있고 metadata 동일 → no-op
  * credential ID 같고 metadata 다름 → old remove + current add
  * Windows에만 있음 → remove
* DB는 현재 선택된 KDBX 하나만 source of truth로 정의합니다. 따라서 다른 DB를 열면 기존 plugin credential cache가 새 DB 기준으로 교체됩니다.
* empty DB도 중요한 case입니다. 이때 이전 DB에서 남아 있던 Windows credentials를 모두 제거해야 합니다.
* 이 API들이 저장하는 것은 Microsoft가 명시한 browser autofill용 credential metadata입니다. 따라서 Windows 쪽에는 RP와 사용자명 등의 metadata가 노출되지만 **private key는 계속 KDBX/Core 밖으로 나가지 않는 구조**를 유지할 수 있습니다. ([GitHub][2])

7. **sync trigger는 `Session::ensure_connected()`에서 “DB가 실제 사용 가능해진 직후” 한 번 실행합니다.**

   * 현재 `ensure_connected()`는 DB 상태 확인 → 필요 시 unlock → endpoint upgrade → trusted `CoreClient` 생성 → `self.client = Some(client)` 순입니다. 따라서 credential sync를 넣을 가장 명확한 지점은 **trusted CoreClient가 만들어진 직후**입니다. ([GitHub][5])
   * 다만 `sync_credentials()`에서 다시 public `Session::request()`를 호출하면 `ensure_connected()`로 재진입할 수 있으므로 그렇게 만들지 않습니다.
   * 구조는 다음처럼 두는 편이 안전합니다.

```text
ensure_connected()
  ├─ bootstrap / unlock
  ├─ trusted client 생성
  ├─ sync_credentials(&mut client, password_session)
  └─ self.client = Some(client)
```

* 또는 먼저 `self.client`에 넣더라도 sync용 private direct-request helper를 따로 만들어 recursion을 피합니다.
* startup sync 자체가 실패했다고 **WebAuthn ceremony 전체를 무조건 실패시킬지는 별도로 결정**해야 합니다. 제안은:

  * Core/KDBX에서 passkey를 읽지 못함 → connection 실패
  * Windows cache API만 일시 실패 → diagnostics 기록 후 ceremony는 계속 허용
* 이유는 Windows cache는 discovery용 mirror이고 실제 assertion의 source of truth는 여전히 Core이기 때문입니다.

8. **registration 직후에도 한 번 재동기화합니다.**

   * startup/open 시점만 동기화하면 같은 `passkey-windows` 프로세스에서 새 credential을 만든 직후 Windows cache에는 아직 그 credential이 없습니다.
   * 따라서 successful make-credential 후 현재 password session이 살아 있는 동안 `GetPasskeys → sync_credentials`를 한 번 더 실행하는 것이 좋습니다.
   * 초기 구현에서는 incremental `AddCredentials(new credential)`보다 **동일한 diff sync를 재사용**하는 것이 오류가 적습니다.
   * 반대로 Keeless desktop에서 credential을 삭제/rename한 경우까지 실시간 반영하려면 Core→sidecar change notification이 필요하므로 이번 범위 밖으로 두고, 최소한 다음 process/database-open sync에서 바로 정리되게 하면 됩니다.
   * Windows cache에서 제거된 것을 KDBX credential 삭제로 해석하는 양방향 sync도 이번 변경에는 넣지 않는 것을 권합니다. 이 작업에서는 **KDBX → Windows 단방향 mirror**라는 contract를 먼저 명확히 하는 편이 안전합니다.

9. **테스트는 prompt 횟수와 cache convergence를 중심으로 잡습니다.**

   * core 단위 테스트:

     * 기존 `GetPasskeysMetadata`의 rp/allowCredentialIds filtering이 변경 전과 동일
     * `PasskeyExtension`으로 assertion 후보 filtering이 그대로 동작
     * 새 `GetPasskeys`가 Windows에 필요한 6개 metadata 필드를 반환
     * private key가 schema에 포함되지 않음
     * 하나의 password session으로 `Unlock → GetPasskeys → AssertPasskey` 성공
     * registration에도 동일 session 재사용 가능
     * expired session은 `InvalidPasswordSession`
     * lock 뒤에는 session이 revoke됨 — 현재 extension도 lock 시 revoke하도록 되어 있습니다. ([GitHub][4])
     * mock `request_password` count를 세어 paranoia mode에서 `CreatePasswordSession` 1회 이후 같은 session을 쓰는 작업들은 추가 prompt가 0회인지 확인
   * passkey-windows 단위 테스트:

     * FFI UTF-16 및 byte-array 변환
     * Windows empty / DB non-empty → 전부 add
     * 양쪽 동일 → API 호출 없음
     * stale Windows entry → remove
     * changed username/display name → replace
     * DB empty → stale credentials 전부 remove
     * expired session → 새 session을 한 번만 만들고 retry
     * IPC disconnect/reset pairing → cached session 제거
     * credential APIs symbol 없음 → panic 없이 unsupported/degraded path
   * Windows 수동/E2E 검증:

     * 기존 KDBX passkey가 process 시작 + DB open 후 Windows account picker에 등장
     * passkey 생성 직후 다시 등장
     * DB에서 삭제 후 재실행하면 Windows에서도 사라짐
     * DB A → DB B 전환 후 cache가 B로 수렴
     * paranoia mode에서 기존처럼 unlock/sync/assert 각각 password를 묻지 않고 한 session window 안에서는 1회만 질문
     * 60초 이상 지난 후 assertion하면 새 session을 한 번 만들고 정상 진행. Password session TTL은 현재 60초입니다. ([GitHub][4])

### 커밋 순서

실제 작업도 **schema/core → password-session 통합 → Windows ABI → sync → integration tests** 순으로 끊는 것이 좋습니다. 특히 첫 PR/commit에서 Windows 쪽까지 한꺼번에 건드리기보다, `GetPasskeysMetadata + 새로운 GetPasskeys + session-aware Assert/Register`까지 core contract를 먼저 확정하면 이후 Windows 구현이 상당히 단순해집니다.

제가 잡는 최종 호출 흐름은 다음입니다.

```text
Windows launches passkey-windows
          │
          ▼
Session::ensure_connected()
          │
          ├─ DB locked?
          │     └─ CreatePasswordSession ── password 1회
          │              │
          │              └─ Unlock(password_session)
          │
          ▼
trusted Core connection
          │
          ├─ GetPasskeys(password_session)
          │
          ▼
sync_credentials()
  ├─ WebAuthNPluginAuthenticatorGetAllCredentials
  ├─ diff against KDBX
  ├─ RemoveCredentials
  └─ AddCredentials
          │
          ▼
WebAuthn ceremony
          │
          └─ Assert/Register(password_session 재사용)
```

이 구조의 핵심은 **`PasskeyExtension = 저비용 credential 후보 탐색용 index`**, **`GetPasskeys = 인증 후 Windows cache에 필요한 credential metadata를 얻는 operation`**, **`PasswordSession = paranoia mode에서 짧은 operation 묶음의 password 재사용 수단`**, **`Windows credential store = KDBX의 단방향 metadata mirror`**로 각각 책임을 분리하는 것입니다. 이렇게 두면 현재 보안 경계를 크게 흐리지 않으면서 “Windows가 credential을 못 찾는 문제”와 “paranoia mode의 반복 password prompt”를 같이 해결할 수 있습니다.

[1]: https://github.com/HelloWorld017/keeless/blob/master/packages/core/src/operations/get_passkeys.rs "keeless/packages/core/src/operations/get_passkeys.rs at master · HelloWorld017/keeless · GitHub"
[2]: https://github.com/microsoft/webauthn/blob/master/webauthnplugin.h "webauthn/webauthnplugin.h at master · microsoft/webauthn · GitHub"
[3]: https://github.com/HelloWorld017/keeless/blob/master/packages/core/src/operations/assert_passkey.rs "keeless/packages/core/src/operations/assert_passkey.rs at master · HelloWorld017/keeless · GitHub"
[4]: https://github.com/HelloWorld017/keeless/blob/master/packages/core/src/extensions/password_session.rs "keeless/packages/core/src/extensions/password_session.rs at master · HelloWorld017/keeless · GitHub"
[5]: https://github.com/HelloWorld017/keeless/blob/master/packages/passkey-windows/src/session.rs "keeless/packages/passkey-windows/src/session.rs at master · HelloWorld017/keeless · GitHub"
[6]: https://github.com/HelloWorld017/keeless/blob/master/packages/passkey-windows/src/sdk_bindings.rs "keeless/packages/passkey-windows/src/sdk_bindings.rs at master · HelloWorld017/keeless · GitHub"
