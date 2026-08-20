## User
CompositeKey가 현재 raw key를 들고 있는데 CompositeKey에서 db의 kdf를 수행한 key를 들고 있고,
MemoryProtection, EncryptedDatabaseStateStore 등에서는 전부 kdf된 key를 hkdf로 파생시켜서 쓰게 하고 싶어.

기존에 저장된 최근 DB들은 다 버려도 괜찮아.

## Agent
가능합니다. 현재 Keeless 구조를 기준으로 보면, 이번 변경은 단순히 `CompositeKey.raw_key`를 다른 값으로 교체하는 수준보다는 **“비싼 DB KDF는 unlock/rekey 시점에만 수행하고, unlock 이후의 모든 내부 암호화는 그 결과에서 HKDF로 파생한다”**는 원칙으로 키 계층 전체를 정리하는 작업으로 잡는 것이 좋습니다.

현재 `Database`는 이미 `kdf_parameters: Option<KdfParameters>`를 가지고 있고, `MemoryProtectionContext`는 DB KDF를 다시 수행하는 구조입니다. 또한 core unlock에서는 raw key를 만들어 recent 복원, mutation journal, state store, credential vault 등에 전달하고 있습니다. ([GitHub][1])

# 1. 최종 목표 구조

먼저 최종 상태를 다음처럼 고정하는 것을 권합니다.

```text
Password / Keyfile / Hardware Key
                │
                ▼
     CompositeCredentials
                │
                ▼
       build_raw_key()
                │
         Raw Composite Key
         (매우 짧게 존재)
                │
                │ DB KDF
                │ Argon2 / AES-KDF
                ▼
          CompositeKey
       (= transformed key)
                │
       ┌────────┼─────────┬───────────┐
       │        │         │           │
       ▼        ▼         ▼           ▼
   KDBX file   HKDF      HKDF        HKDF
              memory    state      mutations
                │         │           │
                ▼         ▼           ▼
           MemoryRoot  StateRoot  JournalRoot
```

핵심 불변조건은 다음입니다.

* `CompositeKey`에는 **DB KDF를 수행한 32-byte transformed key만** 들어간다.
* password/keyfile/hardware key와 raw composite key는 unlock/rekey 중에만 존재한다.
* 일반적인 DB operation에서는 Argon2/AES-KDF를 절대 수행하지 않는다.
* `MemoryProtection`, `EncryptedDatabaseStateStore`, mutation/cache 등 애플리케이션 내부 암호화는 모두 `CompositeKey`에서 HKDF로 domain-separated key를 만든다.
* 일반 save 역시 DB KDF를 수행하지 않는다.
* KDF parameter를 바꾸는 것은 별도의 `rekey` operation으로 취급한다.

---

# 2. `CompositeKey`와 credential 타입 분리

현재의 `CompositeKey`는 사실 두 가지 책임을 갖고 있습니다.

1. password/keyfile/hardware credential 조합
2. 조합 결과인 raw composite key 보관

이를 분리합니다.

### 새 타입

`packages/kdbx/src/model/db/composite_key.rs`

```rust
pub struct CompositeCredentials {
    password_data: Option<SecureBytes>,
    key_file_data: Option<SecureBytes>,
    hardware_key: Option<SecureBytes>,
    raw_key: Option<SecureArray<32>>,
}

pub struct CompositeKey {
    key: SecureArray<32>,
    kdf_fingerprint: [u8; 32],
}
```

여기서 기존 `CompositeKey`의 builder API는 `CompositeCredentials`로 옮깁니다.

```rust
CompositeCredentials::new()
    .with_password(...)
    .with_key_file(...)
    .with_hardware_key(...)
```

그리고 기존:

```rust
build_raw_key()
```

도 `CompositeCredentials`에만 둡니다.

새로운 핵심 API는:

```rust
impl CompositeCredentials {
    pub fn derive_key(
        &self,
        params: &KdfParameters,
    ) -> DatabaseResult<CompositeKey>;
}
```

내부 구현은:

```text
credentials
→ build_raw_key()
→ create_kdf(params.kdf_uuid)
→ kdf.transform(raw, params)
→ raw 즉시 drop/zeroize
→ CompositeKey
```

입니다.

`CompositeKey`에는 `build_raw_key()`가 **없어야 합니다.**

이게 컴파일 단계에서 raw key의 잘못된 재사용을 막아줍니다.

---

# 3. `CompositeKey`에 KDF fingerprint 추가

transformed key는 특정 KDF parameter에 종속됩니다.

예를 들어 같은 password라도:

```text
Argon2(password, salt A) != Argon2(password, salt B)
```

입니다.

따라서 잘못된 header에 `CompositeKey`를 사용하는 실수를 막기 위해 fingerprint를 같이 들고 있게 합니다.

```rust
impl CompositeKey {
    pub(crate) fn new(
        key: SecureArray<32>,
        params: &KdfParameters,
    ) -> Self;

    pub(crate) fn matches(
        &self,
        params: &KdfParameters,
    ) -> bool;
}
```

fingerprint는 예를 들면:

```text
SHA-256(KdfParameters::serialize())
```

이면 충분합니다.

중요한 점은 fingerprint에는 secret이 없다는 것입니다.

이를 통해 writer가:

```rust
if !composite_key.matches(kdf_parameters) {
    return Err(DatabaseError::KdfParametersMismatch);
}
```

를 수행할 수 있습니다.

이 검사는 반드시 넣는 것을 권합니다. 그렇지 않으면 향후 KDF parameter가 변경된 `Database`와 이전 transformed key를 조합해서 저장하면서 복구 불가능한 파일을 만들 수 있습니다.

---

# 4. KDBX4 reader: KDF를 수행하는 첫 번째 핵심 지점

현재 KDBX4 writer에서는 매 저장마다 `kdf.randomize()`를 호출한 후 raw key를 다시 KDF에 넣고 있습니다. ([GitHub][2])

reader 쪽에서는 이를 반대로 이용합니다.

새 API를:

```rust
pub struct OpenedDatabase {
    pub database: Database,
    pub key: CompositeKey,
}
```

로 잡고,

```rust
pub fn open_database<R: Read>(
    reader: R,
    credentials: &CompositeCredentials,
) -> DatabaseResult<OpenedDatabase>;
```

로 변경합니다.

KDBX4 flow는:

```text
1. signature/version read
2. outer header read
3. KdfParameters 확보
4. credentials.derive_key(kdf_parameters)
                  ↓
            CompositeKey
5. master_seed + CompositeKey → file encryption key
6. master_seed + CompositeKey → HMAC key
7. payload decrypt
8. XML parse
9. memory protection sealing
10. OpenedDatabase { database, key }
```

여기서 4번이 **unlock 과정에서 유일한 DB KDF**가 됩니다.

파일용 master key 계산은 지금처럼:

```text
SHA256(master_seed || transformed_key)
```

를 유지하면 됩니다.

즉 KDBX 포맷 자체에는 아무 변화가 없습니다.

---

# 5. KDBX3.1도 `KdfParameters`로 normalize

KDBX3.1 reader는 현재 header의:

```text
transform_seed
transform_rounds
```

를 읽고 즉석에서 `KdfParameters`를 만든 다음 raw key에 AES-KDF를 수행합니다. ([github.com][3])

여기를 통일합니다.

reader에서:

```rust
let mut params = KdfParameters::new(AES_KDF_UUID);
params.set_byte_array("S", &header.transform_seed);
params.set_uint64("R", header.transform_rounds);

let key = credentials.derive_key(&params)?;
```

로 만들고,

```rust
database.kdf_parameters = Some(params);
```

도 반드시 설정합니다.

현재 `Database`에는 이미 `kdf_parameters`가 있으므로 별도 KDBX31 전용 필드를 만들 필요가 없습니다. ([GitHub][1])

그러면 이후 writer 입장에서는 KDBX3.1인지 KDBX4인지와 관계없이:

```rust
database.kdf_parameters
```

와

```rust
CompositeKey
```

를 대응시킬 수 있습니다.

---

# 6. 일반 save에서 KDF 완전 제거

이번 리팩터링에서 가장 중요한 변경 중 하나입니다.

현재 KDBX4 writer는:

```rust
kdf.randomize(&mut kdf_params)?;

let raw_key = file_key.build_raw_key()?;
let transformed =
    kdf.transform(raw_key, &kdf_params)?;
```

를 매 save마다 수행합니다. ([GitHub][2])

이 로직을 전부 제거합니다.

새 writer signature:

```rust
pub fn write_kdbx4<W: Write>(
    writer: &mut W,
    database: &Database,
    key: &CompositeKey,
) -> DatabaseResult<()>
```

그리고:

```rust
let kdf_params = database
    .kdf_parameters
    .as_ref()
    .ok_or(DatabaseError::MissingKdfParameters)?;

key.verify_kdf_parameters(kdf_params)?;
```

후 곧바로:

```rust
key.unlock(|transformed| {
    HashEngine::sha256_multi(&[
        &master_seed,
        transformed,
    ])
})
```

를 수행합니다.

즉 save 시에는:

```text
fresh master_seed
fresh encryption_iv
fresh inner stream key
기존 KDF parameters
기존 transformed CompositeKey
```

를 사용합니다.

`master_seed`, IV, inner stream key 등 실제 encryption randomness는 계속 매번 갱신합니다.

**고정하는 것은 KDF parameters뿐입니다.**

---

# 7. KDBX3.1 writer도 transform seed 재생성 제거

KDBX3.1도 같은 원칙입니다.

일반 저장에서는 기존:

```text
transform_seed
transform_rounds
```

를 `database.kdf_parameters`에서 가져옵니다.

그리고:

```text
CompositeKey
+ fresh master_seed
→ final encryption key
```

만 계산합니다.

즉 일반 save에서:

```rust
generate transform_seed
AES-KDF(raw_key)
```

두 단계가 없어집니다.

새 transform seed가 필요한 경우는 오직:

* password 변경
* keyfile 변경
* hardware key 변경
* KDF rounds 변경

같은 **rekey**뿐입니다.

---

# 8. 신규 DB 생성과 일반 저장을 API 레벨에서 분리

일반 save와 신규 DB 생성은 이제 의미가 완전히 다릅니다.

신규 DB에는 아직 KDF parameters도 transformed key도 없습니다.

그래서 다음처럼 나누는 게 좋습니다.

```rust
pub fn initialize_database_key(
    database: &mut Database,
    credentials: &CompositeCredentials,
) -> DatabaseResult<CompositeKey>
```

이 함수는:

```text
Database::new()
↓
default KDF 선택
↓
KDF parameters randomize
↓
credentials → raw
↓
DB KDF
↓
CompositeKey
↓
database.kdf_parameters = Some(...)
```

를 수행합니다.

그 뒤 일반 writer를 그대로 호출합니다.

상위 API는:

```rust
pub fn create_database<W: Write>(
    writer: W,
    database: &mut Database,
    credentials: &CompositeCredentials,
) -> Result<CompositeKey>;
```

정도로 제공하면 됩니다.

반대로 일반 저장은:

```rust
pub fn save_database<W: Write>(
    writer: W,
    database: &Database,
    key: &CompositeKey,
) -> Result<()>;
```

입니다.

이 둘을 API 단계에서 분리해야 향후 실수로 일반 save가 KDF salt를 randomize하는 회귀가 생기지 않습니다.

---

# 9. password 변경은 `rekey_database`로 독립

현재 `save_database_with_credentials(memory_key, file_key)`는 old credential로 memory를 풀고 new credential로 파일을 저장하는 password rotation 역할까지 맡고 있습니다.

이를 명시적인 operation으로 바꾸는 것을 권합니다.

```rust
pub fn rekey_database(
    database: &mut Database,
    old_key: &CompositeKey,
    new_credentials: &CompositeCredentials,
    options: RekeyOptions,
) -> DatabaseResult<CompositeKey>;
```

`RekeyOptions`:

```rust
pub struct RekeyOptions {
    pub regenerate_kdf_salt: bool,
    pub kdf_parameters: Option<KdfParameters>,
}
```

실제 flow:

```text
old CompositeKey
        │
        └─ MemoryProtection unlock
                 │
                 ▼
        plaintext/protected values
                 │
new credentials
        │
        ▼
new/randomized KDF params
        │
        ▼
new CompositeKey
        │
        ├─ memory protection reseal
        └─ DB save
```

성공한 후에만 core의 active key를 교체합니다.

```rust
core.composite_key = Some(new_key);
```

중간 실패 시에는 old key가 계속 유효해야 합니다.

---

# 10. `MemoryProtectionContext`에서 KDF 완전히 제거

현재 `MemoryProtectionContext`는 `kdf_parameters`를 가지고 있고, `unlock()`에서 raw composite key를 다시 KDF에 넣습니다. `Database::create_memory_context()`도 DB KDF parameters를 가져와 이 과정을 구성합니다. ([GitHub][1])

이 부분을 가장 먼저 성능 개선 대상으로 볼 수 있습니다.

새 구조:

```rust
pub struct MemoryProtectionContext {
    id: [u8; 16],
    salt: [u8; 32],
    verifier_nonce: [u8; 24],
    verifier: Vec<u8>,
}
```

삭제:

```rust
kdf_parameters: KdfParameters
```

API:

```rust
pub fn create(
    key: &CompositeKey,
) -> DatabaseResult<(Self, SecureArray<32>)>;

pub fn unlock(
    &self,
    key: &CompositeKey,
) -> DatabaseResult<SecureArray<32>>;
```

root는:

```text
HKDF-SHA256(
    IKM  = CompositeKey,
    salt = context.salt,
    info = "keeless/kdbx/memory/root/v2"
)
```

로 만듭니다.

그 아래는 그대로:

```text
MemoryRoot
   ↓ HKDF(entry-id)
EntryKey
```

를 유지합니다.

domain string은 v2로 올립니다.

```rust
b"keeless/kdbx/memory/root/v2"
b"keeless/kdbx/memory/entry/v2"
...
```

기존 memory context는 DB 내부 runtime state이므로 migration하지 않고 DB open 때 새 context를 만들면 됩니다.

---

# 11. `MemoryUnlockSession` 단순화

현재 session은 `CompositeKey`를 받아 context별 root를 캐시합니다.

새 구조에서도 캐시는 유지하는 게 좋지만 KDF가 HKDF뿐이므로 역할은 훨씬 단순해집니다.

```rust
pub struct MemoryUnlockSession<'a> {
    key: &'a CompositeKey,
    roots: HashMap<ContextId, SecureArray<32>>,
}
```

그리고:

```rust
context.unlock(key)
```

는 HKDF + verifier 검사만 합니다.

따라서 operation마다 `MemoryUnlockSession`이 새로 만들어져도 더 이상 Argon2 비용이 발생하지 않습니다.

이 변경이 `reveal_entry_fields`에서 기대하는 가장 직접적인 성능 개선입니다.

---

# 12. Database model API 전부 `CompositeKey` 기준으로 교체

현재 `Database`에는 다음 식의 API가 있습니다.

```rust
seal_protected_strings(&CompositeKey)
protect_entry_strings(&CompositeKey)
memory_unlock(&CompositeKey)
create_memory_context(&CompositeKey)
```

현재는 이 `CompositeKey`가 raw credential 역할을 하지만, 리팩터링 후에는 transformed key를 그대로 받게 됩니다.

즉 호출 API 모양은 크게 유지하면서 내부 의미가 바뀝니다.

`create_memory_context()`에서 현재 사용하는:

```rust
self.kdf_parameters.clone()
```

로직은 제거합니다. 현재 이 부분 때문에 DB KDF와 memory KDF가 서로 강하게 결합되어 있습니다. ([GitHub][1])

---

# 13. `EncryptedDatabaseStateStore`를 `CompositeKey` 기반으로 변경

현재 constructor는:

```rust
EncryptedDatabaseStateStore::new(
    raw_key: &SecureArray<32>,
    persistence,
    database_id,
)
```

이고 raw key에서 database ID를 salt로 HKDF root를 만듭니다. ([GitHub][4])

이를:

```rust
EncryptedDatabaseStateStore::new(
    key: &CompositeKey,
    persistence,
    database_id,
)
```

로 변경합니다.

파생 계층:

```text
CompositeKey
    │
    ▼ HKDF
DatabaseStateRoot
    │
    ├─ HKDF("config/v2")
    └─ HKDF("core-wire/v2")
```

예:

```rust
const ROOT_HKDF_INFO: &[u8] =
    b"keeless/database-state/root/v2";
```

salt는 계속 `database_id`를 써도 좋습니다.

```rust
HKDF(
    salt = database_id,
    ikm  = CompositeKey,
    info = ROOT_HKDF_INFO
)
```

기존 state를 버려도 된다고 하셨으므로:

```rust
STATE_VERSION = 2
```

로 바로 올리고 v1 fallback은 구현하지 않습니다.

decrypt 실패 시 v1 state를 migration하려고 시도하지 않고 삭제/초기화하도록 합니다.

---

# 14. recent DB bootstrap은 별도로 고쳐야 함

여기는 기존 state 호환성 여부와 별개로 반드시 수정해야 합니다.

현재 unlock 순서는:

```rust
let key = CompositeKey::new().with_password(password)?;
let raw_key = key.build_raw_key()?;

core.restore_recent_selection(&raw_key).await?;

let provider = ...
let path = ...

FileHandle::open(...)
```

입니다. 즉 **DB 파일 위치를 찾기 전에 raw key를 사용합니다.** ([GitHub][5])

하지만 새 `CompositeKey`는 DB header를 읽은 뒤에야 만들 수 있습니다.

따라서 future recent DB까지 정상 동작시키려면 recent metadata 자체에 locator를 저장해야 합니다.

현재 `RecentDatabase`에는 visible record상 `id`, `name`, `storage_type`, `last_opened_at_ms` 정도만 기록되고 실제 descriptor는 별도 state에 의존합니다. ([GitHub][6])

이를 v2 recent state에서:

```rust
struct RecentDatabase {
    id: String,
    name: String,

    descriptor: StorageDescriptor,

    last_opened_at_ms: u64,
}
```

식으로 바꿉니다.

그 결과:

```text
RecentDatabase
→ StorageDescriptor
→ provider/path
→ DB header read
→ KDF parameters
→ password KDF
→ CompositeKey
```

순서가 됩니다.

이 recent descriptor를 암호화해야 하는 보안 요구가 있다면 DB key로 암호화해서는 안 됩니다. bootstrap chicken-and-egg가 다시 생기기 때문입니다.

현재 구조를 감안하면 우선 recent locator는 core state에 평문 metadata로 두고, **DB 내부 민감 state만 `CompositeKey`로 암호화**하는 편이 가장 단순합니다.

기존 recent들은 모두 버릴 수 있다고 하셨으므로 `RECENT_STATE_VERSION = 2`로 올리고 v1은 빈 recent 목록으로 취급하면 됩니다.

---

# 15. mutation journal/cache도 raw key 제거

현재 unlock에서 raw key를:

```rust
MutationCoordinator::new(
    &raw_key,
    database_id,
    0,
)
```

로 넘기고 있습니다. ([GitHub][5])

이것도 같은 정책을 적용해야 합니다.

```rust
MutationCoordinator::new(
    &composite_key,
    database_id,
    0,
)
```

그리고 내부 root derivation:

```text
CompositeKey
    ↓ HKDF
MutationRoot
    ├─ JournalKey
    └─ CacheKey
```

예:

```text
keeless/mutations/root/v2
keeless/mutations/journal/v2
keeless/mutations/cache/v2
```

이렇게 해야 “raw key 제거”가 core 전체에서 완성됩니다.

이번 작업에서 `build_raw_key()` 사용처를 workspace 전체 grep해서 **KDB/KDBX reader와 credential derivation 외에는 0개**가 되게 만드는 것을 acceptance criterion으로 잡는 것이 좋습니다.

---

# 16. `CredentialVault`에는 transformed `CompositeKey` 저장

현재 unlock 성공 후에는 raw key를 `CredentialVault::wrap(&raw_key)`에 넣고 있습니다. ([GitHub][5])

이를:

```rust
CompositeKeyVault
```

혹은 기존 이름을 유지한다면:

```rust
CredentialVault::wrap(&composite_key)
```

로 변경합니다.

내부 encrypted payload는:

```text
32-byte transformed key
+
32-byte KDF fingerprint
```

정도가 됩니다.

restore:

```rust
fn restore_key(&self) -> Result<CompositeKey>
```

는 더 이상:

```rust
CompositeKey::from_raw_key(...)
```

를 호출하지 않고 바로 transformed key를 복구합니다.

AAD도:

```text
keeless-credential-v2
```

로 변경합니다.

paranoia mode에서는 지금과 마찬가지로 vault 자체를 만들지 않습니다.

---

# 17. `reveal_entry_fields` 경로 변경

현재 일반 mode에서는 vault에서 raw-key 기반 `CompositeKey`를 복구한 다음 protected field를 읽고, password를 직접 받은 경우에는 `verify_credentials()`로 DB를 다시 열어 credential을 확인합니다.

현재 `FileHandle::verify_credentials()` 자체도 checkpoint를 `open_database()`로 다시 여는 방식입니다. ([GitHub][7])

새 일반 mode:

```text
CompositeKeyVault
↓
restore transformed CompositeKey
↓
MemoryProtection HKDF
↓
field decrypt
```

여기에는 DB KDF가 한 번도 없습니다.

그래서 대략:

```rust
let key = core
    .credential
    .as_ref()
    .ok_or(...)?
    .restore_key()?;

database.reveal_entry_field(&key, ...)?;
```

가 됩니다.

### paranoia mode

사용자가 password를 매번 입력하는 경우에는:

```text
password
↓
CompositeCredentials
↓
checkpoint header
↓
DB KDF
↓
temporary CompositeKey
↓
field decrypt
```

가 됩니다.

paranoia mode가 DB KDF 비용을 지불하는 것은 의도된 trade-off입니다.

---

# 18. `FileHandle::verify_credentials()` 대신 derive API

현재:

```rust
pub fn verify_credentials(
    &self,
    key: &CompositeKey,
) -> Result<(), SyncError> {
    open_database(checkpoint, key).map(|_| ())
}
```

입니다. ([GitHub][7])

새 구조에서는:

```rust
pub fn derive_key(
    &self,
    credentials: &CompositeCredentials,
) -> Result<CompositeKey, SyncError>
```

로 바꾸는 게 좋습니다.

또는 이름을 명확히:

```rust
verify_and_derive_key()
```

로 합니다.

그렇게 하면 credential 검사 때문에 한번 수행한 KDF 결과를 버리지 않습니다.

---

# 19. `FileHandle`/sync API를 `CompositeKey` 기준으로 변경

현재 sync 전체가 `CompositeKey`를 credential처럼 넘기고 있고 `save_database()`도 동일 key를 사용합니다. ([GitHub][7])

리팩터링 후에는:

```rust
pub async fn sync(
    &mut self,
    key: &CompositeKey,
)
```

자체 signature는 유지할 수 있습니다.

다만 이제 key가 transformed key입니다.

remote DB를 열 때 먼저 outer header만 읽고:

```text
remote KDF fingerprint
        │
        ├─ == active CompositeKey fingerprint
        │      ↓
        │   KDF 생략
        │   remote decrypt
        │
        └─ !=
               ↓
        CredentialsRequired
```

로 처리합니다.

이를 위해 kdbx layer에 다음 API를 하나 추가하는 것이 좋습니다.

```rust
pub fn open_database_with_key<R: Read>(
    reader: R,
    key: &CompositeKey,
) -> DatabaseResult<Database>;
```

이 함수는 outer header를 읽은 후 fingerprint를 검사하고, 맞으면 KDF 없이 바로 payload key를 계산합니다.

---

# 20. 외부에서 KDF parameter가 바뀐 sync 처리

이것은 별도 error type을 만드는 것을 권합니다.

```rust
DatabaseError::KdfParametersMismatch
```

sync 레벨에서는:

```rust
SyncError::CredentialsRequired
```

로 매핑합니다.

예를 들어 다른 KeePass client가 같은 password를 사용하지만 다른 KDF salt를 가진 파일을 업로드하면 기존 transformed key로는 새 파일을 열 수 없습니다.

security-first 설계라면 **raw key를 보존하지 말고 credential 재입력을 요구**합니다.

background sync에서는 즉시 password prompt를 띄울 수 없으므로:

```text
remote KDF changed
↓
sync 중단
↓
SyncStatus::CredentialsRequired
↓
foreground UI에서 password 요청
↓
remote header 기준 새 CompositeKey derive
↓
sync 재개
```

로 처리하는 것이 깔끔합니다.

이 동작은 실제 UX에 영향이 있으므로 별도 integration test를 두어야 합니다.

만약 향후 “외부 KeePass가 KDF parameters를 바꿔도 background sync가 자동으로 계속되어야 한다”는 요구가 생기면 그때만 cold raw-key vault를 별도로 추가하는 것이 낫습니다.

이번 리팩터링에서는 저는 **raw key fallback을 넣지 않는 쪽**을 권합니다.

---

# 21. background sync에서 raw credential 제거

현재 unlock 성공 후:

```rust
core.start_background_sync(key);
```

에 기존 credential-containing `CompositeKey` 자체를 넘깁니다. ([GitHub][5])

새 구조에서는 오히려 원하는 형태가 됩니다.

```rust
core.start_background_sync(composite_key.clone());
```

여기 들어가는 것은 transformed key뿐입니다.

background thread/task에는 password, keyfile bytes, raw composite key가 존재하지 않습니다.

---

# 22. `KeelessCore`의 key 상태 정리

현재:

```rust
credential: Option<CredentialVault>
```

중심인데, 가능하면 active key의 의미를 조금 명확히 하는 것이 좋습니다.

예:

```rust
pub(crate) struct KeelessCore {
    ...
    key_vault: Option<CompositeKeyVault>,
    ...
}
```

혹은 vault abstraction을 유지한다면 기존 필드명을 유지해도 됩니다.

중요한 것은 core가 다음을 절대 보관하지 않는 것입니다.

```text
password
CompositeCredentials
raw composite key
```

unlock 함수 local variable에서만 존재해야 합니다.

---

# 23. unlock 전체 flow 재작성

최종 `unlock.rs` 흐름은 대략 다음이어야 합니다.

```text
1. selection 확인
2. recent이면 RecentState에서 descriptor 복원
3. provider/path 확보

4. remote/cache DB의 outer header 확보
5. CompositeCredentials(password)
6. KDF parameters 읽기
7. credentials.derive_key()
        ↓
   CompositeKey

8. DB open
9. CompositeCredentials drop
10. raw composite key는 이미 zeroize

11. MutationCoordinator::new(&CompositeKey)
12. cache/journal replay
13. EncryptedDatabaseStateStore::new(&CompositeKey)
14. MemoryProtection 사용
15. CompositeKeyVault::wrap(&CompositeKey)
16. extensions.unlock(..., &CompositeKey)
17. background sync(&CompositeKey)
```

현재 unlock에서는 6단계 이전에 raw key를 생성하고 recent/state/journal에 사용하는데, 이 순서를 뒤집는 것이 이번 core refactor의 중심입니다. ([GitHub][5])

---

# 24. extension API도 transformed key로 변경

현재:

```rust
core.extensions.unlock(
    handle.database(),
    &key,
)?;
```

도 기존 credential key를 받습니다. ([GitHub][5])

extension 내부에서 key가 필요한 경우 전부 transformed `CompositeKey`를 받게 하고, 자체 key가 필요한 extension은:

```text
CompositeKey
↓ HKDF("keeless/extensions/<name>/v1")
ExtensionKey
```

로 파생하게 합니다.

extension이 `build_raw_key()`에 접근할 수 없게 되는 것이 중요합니다.

---

# 25. HKDF domain separation을 공통 API로 제공

각 모듈에서 직접 HKDF 코드를 반복하지 않는 게 좋습니다.

`CompositeKey`에:

```rust
pub(crate) fn derive_key<const N: usize>(
    &self,
    salt: Option<&[u8]>,
    info: &[u8],
) -> DatabaseResult<SecureArray<N>>
```

를 만듭니다.

그러면:

```rust
key.derive_key(
    Some(database_id.as_bytes()),
    b"keeless/database-state/root/v2",
)
```

처럼 사용할 수 있습니다.

다만 arbitrary `info`를 public API로 노출하기 싫다면 더 강한 타입도 가능합니다.

```rust
enum KeyDomain {
    MemoryProtection,
    DatabaseState,
    MutationJournal,
    Cache,
}
```

개인적으로는 내부 crate 경계를 감안하면 처음에는 `derive_key()` 정도로 충분합니다.

---

# 26. 키 namespace 정리

이번 변경 때 한 번에 namespace를 정의해 두는 것이 좋습니다.

예:

```text
keeless/kdbx/memory/root/v2
keeless/kdbx/memory/entry/v2

keeless/core/state/root/v2
keeless/core/state/config/v2
keeless/core/state/wire/v2

keeless/core/mutation/root/v2
keeless/core/mutation/cache/v2
keeless/core/mutation/journal/v2

keeless/core/extensions/<name>/v1
```

서로 다른 subsystem이 같은 derived key를 절대 사용하지 않게 합니다.

---

# 27. migration 정책

기존 recent/state를 모두 버릴 수 있으므로 migration code는 최대한 안 만드는 게 좋습니다.

변경:

```text
RECENT_STATE_VERSION: 1 → 2
STATE_VERSION:        1 → 2
CredentialVault AAD: v1 → v2
MemoryProtection:     v1 → v2
Mutation cache:       기존 version → +1
Journal format:       기존 version → +1
```

기존 encrypted core/cache/journal은 decrypt fallback하지 않습니다.

새 버전을 발견하지 못하면:

```text
state → empty
cache → discard
journal → discard
recent → empty
```

로 처리합니다.

단, 실제 `.kdbx` 파일 자체는 당연히 migration 대상이 아닙니다. 기존 KDBX를 정상적으로 password로 열고, 그 순간 새 runtime key hierarchy가 만들어집니다.

---

# 28. 테스트는 단계별로 이렇게 추가

이번 변경은 cryptography와 persistence가 같이 움직이므로 unit test보다 invariant test를 강하게 두는 게 좋습니다.

### KDF / CompositeKey

```text
same raw + same params → same CompositeKey
same raw + different salt → different CompositeKey
different password → different CompositeKey
CompositeKey fingerprint mismatch → writer rejects
```

그리고 중요한 테스트:

```text
CompositeKey API에는 build_raw_key가 없다
```

이건 사실 컴파일 구조 자체가 보장합니다.

### KDBX4

```text
open(password) → CompositeKey 반환
save(CompositeKey) → reopen 가능
save 전/후 KDF parameters 동일
save 전/후 master_seed는 다름
save 전/후 IV는 다름
```

가 중요합니다.

특히:

```rust
assert_eq!(
    before.kdf_parameters,
    after.kdf_parameters
);
```

테스트를 반드시 두는 것을 권합니다.

### KDBX3.1

같이:

```text
transform_seed 유지
transform_rounds 유지
master_seed 변경
IV 변경
저장 후 재오픈 가능
```

을 검사합니다.

---

# 29. “일반 save에서는 KDF가 0회” 테스트

성능 문제의 재발을 막는 가장 좋은 테스트입니다.

test-only counting KDF를 만들거나 KDF engine에 instrumentation을 넣어서:

```text
unlock    → KDF count = 1
reveal    → +0
save      → +0
sync same KDF → +0
reveal 100회 → +0
rekey     → +1
```

를 검증합니다.

이 테스트가 있으면 나중에 누군가 `MemoryProtection`이나 save에 KDF를 다시 넣는 회귀를 바로 잡을 수 있습니다.

---

# 30. MemoryProtection 테스트

기존 테스트를 대부분 유지하면서 key source만 변경합니다.

추가로:

```text
same CompositeKey + same context → same root
same CompositeKey + different context → different root
different CompositeKey → verifier failure
entry A key != entry B key
```

를 둡니다.

그리고 기존 KDF-related memory protection test는 삭제합니다.

`MemoryProtectionContext`에 KDF 개념 자체가 남지 않는 것이 목표입니다.

---

# 31. core state 테스트

`EncryptedDatabaseStateStore`는 다음을 검증합니다.

```text
same CompositeKey + same DatabaseId → decrypt 성공
same CompositeKey + different DatabaseId → 실패
different CompositeKey → 실패
record A key != record B key
v1 state → 무시/초기화
```

---

# 32. recent DB integration test

이 부분은 꼭 별도로 테스트해야 합니다.

```text
1. DB 선택
2. unlock
3. recent 기록
4. core 재생성
5. recent id 선택
6. password 입력
7. descriptor를 credential 없이 복원
8. DB header 읽기
9. CompositeKey derive
10. 정상 unlock
```

여기서 step 7이 핵심입니다.

`EncryptedDatabaseStateStore`를 열기 전에 provider/path가 확보되는지를 테스트해야 합니다.

---

# 33. sync integration tests

적어도 두 케이스가 필요합니다.

### 동일 KDF parameters

```text
local key = K
remote DB = same KDF parameters
remote 내용 변경

→ sync
→ KDF 0회
→ merge 성공
```

### KDF parameters 변경

```text
local key = K1
remote DB = same password but new salt → K2

→ active CompositeKey로 sync
→ KdfParametersMismatch
→ SyncError::CredentialsRequired
→ remote 파일 변경 없음
→ local 파일 변경 없음
```

그 다음 password를 제공해 K2를 derive한 뒤 sync가 성공하는 것도 별도로 검증합니다.

---

# 34. password rotation 테스트 변경

현재 old/new credential을 writer에 동시에 넘기는 테스트는 새 API에 맞춰:

```text
open(old credentials)
→ old CompositeKey

rekey_database(
    old CompositeKey,
    new credentials
)
→ new CompositeKey

save(new CompositeKey)

open(old credentials) → failure
open(new credentials) → success

protected fields 그대로 유지
```

로 변경합니다.

---

# 35. 성능 benchmark 추가

이번 작업의 목적 중 하나가 Windows의 약 500ms operation 문제이므로 benchmark를 넣는 편이 좋습니다.

다음 세 가지를 따로 측정합니다.

```text
derive CompositeKey (Argon2)
reveal protected field
save database
```

목표는 절대 시간보다는 호출 패턴입니다.

```text
derive: 의도적으로 느림
reveal: HKDF + XChaCha만
save: DB KDF 없음
```

예를 들어 Criterion benchmark를 쓴다면:

```text
memory_protection/reveal
database/save_with_derived_key
```

를 추가합니다.

---

# 36. 실제 작업 순서

제가 구현한다면 PR/commit도 이 순서로 나누겠습니다.

1. **`CompositeCredentials` 도입**

   * 기존 `CompositeKey` credential 구현 이동
   * 아직 동작 변경 없음

2. **새 transformed `CompositeKey` 도입**

   * KDF fingerprint
   * HKDF derive helper
   * `derive_key()`

3. **KDBX4 reader 변경**

   * `OpenedDatabase`
   * KDF 결과 반환

4. **KDBX3.1 reader 변경**

   * KDF params normalize
   * `database.kdf_parameters` 보존

5. **MemoryProtection HKDF 전환**

   * KDF 제거
   * v2 namespace

6. **writer 변경**

   * KDBX4 KDF randomization 제거
   * KDBX31 transform seed regeneration 제거
   * save는 transformed key만 사용

7. **create/rekey API 분리**

   * 신규 DB KDF 초기화
   * password rotation

8. **sync 변경**

   * `open_database_with_key`
   * KDF fingerprint 검사
   * `CredentialsRequired`

9. **core recent bootstrap 변경**

   * descriptor recent metadata에 저장
   * recent v2

10. **EncryptedDatabaseStateStore v2**

    * raw → CompositeKey

11. **mutation/cache/journal v2**

    * raw → CompositeKey

12. **CredentialVault v2**

    * raw → transformed key

13. **unlock.rs 전체 정리**

    * `build_raw_key()` lifetime 최소화
    * active/background 모두 transformed key 사용

14. **reveal/extension/password-session 정리**

15. **workspace 전체 raw-key audit**

16. **성능/통합 테스트**

---

# 37. 마지막 raw-key audit의 acceptance criterion

마지막에 다음을 grep하는 것을 release gate로 두는 것이 좋습니다.

```text
build_raw_key
from_raw_key
SecureArray<32> raw_key
```

최종적으로 `build_raw_key()`는 대략 다음 계층에서만 나와야 합니다.

```text
CompositeCredentials implementation
DB KDF derivation
legacy format reader가 필요하다면 그 adapter
tests
```

다음에서는 **0건**이어야 합니다.

```text
core/
sync/
MemoryProtection
database_state
mutation journal
cache
extensions
reveal operations
background sync
save_database
```

이 기준이 이번 리팩터링이 제대로 끝났는지를 가장 명확하게 보여줍니다.

---

## 최종 API 모양

결과적으로 kdbx public API를 대략 이렇게 만드는 것을 추천합니다.

```rust
// credential composition
let credentials = CompositeCredentials::new()
    .with_password(password)?;

// DB open: 비싼 KDF 수행
let OpenedDatabase {
    database,
    key,
} = open_database(bytes, &credentials)?;

// 이후 operation: 전부 derived key
database.reveal_entry_field(&key, ...)?;

save_database(
    &mut output,
    &database,
    &key,
)?;

// credential 변경 때만 다시 KDF
let new_key = rekey_database(
    &mut database,
    &key,
    &new_credentials,
    RekeyOptions::default(),
)?;
```

core 쪽 unlock 이후 상태는:

```text
                 ┌─ Database
unlock ──────────┼─ CompositeKeyVault
                 ├─ EncryptedDatabaseStateStore
                 ├─ MutationCoordinator
                 └─ BackgroundSync
```

이고 이 네 컴포넌트 모두 같은 transformed `CompositeKey`를 출발점으로 삼되, 각각 **서로 다른 HKDF domain으로 파생된 키만 사용**하게 됩니다.

이 구조로 가면 `reveal_entry_fields` 같은 평상시 operation에서 DB KDF가 끼어들 여지가 구조적으로 없어지고, save 역시 Argon2/AES-KDF와 분리됩니다. 동시에 raw composite key는 unlock/rekey의 아주 짧은 구간을 제외하면 core나 background task에 남지 않게 됩니다. 현재 raw key가 unlock 이후 state store, mutation coordinator, vault와 background sync로 이어지는 구조를 한 번에 끊는 리팩터링이 되는 셈입니다. ([GitHub][5])

특히 구현 시에는 **`CompositeCredentials` 도입 → reader에서 `CompositeKey` 반환 → MemoryProtection 전환**까지를 첫 번째 컴파일 가능한 milestone으로 잡는 것을 권합니다. 여기까지 완료하면 `reveal_entry_fields` 성능 개선을 먼저 측정할 수 있고, 그 이후 state/sync/persistence 변경을 단계적으로 진행할 수 있습니다.

[1]: https://github.com/HelloWorld017/keeless/blob/master/packages/kdbx/src/model/db/database.rs "keeless/packages/kdbx/src/model/db/database.rs at master · HelloWorld017/keeless · GitHub"
[2]: https://github.com/HelloWorld017/keeless/blob/master/packages/kdbx/src/kdbx/file/kdbx4_writer.rs "keeless/packages/kdbx/src/kdbx/file/kdbx4_writer.rs at master · HelloWorld017/keeless · GitHub"
[3]: https://github.com/HelloWorld017/keeless/blob/master/packages/kdbx/src/kdbx/file/kdbx31_reader.rs "keeless/packages/kdbx/src/kdbx/file/kdbx31_reader.rs at master · HelloWorld017/keeless · GitHub"
[4]: https://github.com/HelloWorld017/keeless/blob/master/packages/core/src/database_state.rs "keeless/packages/core/src/database_state.rs at master · HelloWorld017/keeless · GitHub"
[5]: https://github.com/HelloWorld017/keeless/blob/master/packages/core/src/operations/unlock.rs "keeless/packages/core/src/operations/unlock.rs at master · HelloWorld017/keeless · GitHub"
[6]: https://github.com/HelloWorld017/keeless/blob/master/packages/core/src/recent.rs "keeless/packages/core/src/recent.rs at master · HelloWorld017/keeless · GitHub"
[7]: https://github.com/HelloWorld017/keeless/blob/master/packages/sync/src/sync.rs "keeless/packages/sync/src/sync.rs at master · HelloWorld017/keeless · GitHub"
