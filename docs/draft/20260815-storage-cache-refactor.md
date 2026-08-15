# Storage Cache Refactor

## 목표

recent database를 local cache, mutation journal, 암호화된 Core state의 namespace로 사용한다. storage 연결 정보의 source of truth는 `StorageDescriptor` 하나만 유지한다.

```rust
pub struct StorageDescriptor {
    pub provider: String,
    pub path: String,
}
```

`DatabaseStorageConfig`, `StorageConfigurer`, `OpenRecentDatabase`는 제거한다. provider별 schema enum을 만들지 않아 host가 새 provider를 자유롭게 추가할 수 있게 한다.

## Core Storage Wrapper

`keeless_sync::StorageProvider`는 storage I/O만 담당한다. Core는 이를 한 번 감싼 `Storage` wrapper를 provider registry에 보관한다.

```rust
pub struct Storage {
    provider: Arc<dyn keeless_sync::StorageProvider>,
    // Provider가 정한 descriptor path의 stable identity 표현.
    get_normalized_path: Arc<dyn Fn(&str) -> Result<String>>,
    is_persistent: bool,
}
```

`Storage`는 다음 역할을 담당한다.

* `get_normalized_path(path)`는 `DatabaseId` derivation에 사용할 stable path를 반환한다.
* `is_persistent()`가 `false`이면 descriptor, encrypted config, recent record를 저장하지 않는다.
* Core는 provider 이름을 비교해 WebDAV나 browser local-file을 특별 취급하지 않는다.
* 실제 read/stat/write에는 descriptor의 raw `path`를 underlying `StorageProvider`에 전달한다.

Browser local-file은 session provider로 등록하고 `is_persistent = false`로 둔다. 현재처럼 process restart 뒤 File/FileSystemFileHandle을 복원할 수 없으므로 recent에 남기지 않는다.

## Database ID

Core가 다음 값을 한 번만 derive한다.

```text
DatabaseId = HKDF-SHA-256(provider, storage.get_normalized_path(path))
```

provider와 normalized path는 length-prefix 등으로 모호하지 않게 인코딩하고, 고정된 domain-separation info를 사용한다. `DatabaseId`의 raw HKDF bytes는 internal key material이며 recent ID와 host persistence key에는 base64url로 encode한다.

이 변경으로 `DatabasePersistence`는 descriptor를 받아 identity를 정하지 않는다.

```rust
trait DatabasePersistence {
    fn select(&self, database_id: &DatabaseId) -> HostFuture<'_, Result<()>>;
    fn purge(&self, database_id: &DatabaseId) -> HostFuture<'_, Result<()>>;
}
```

`select_by_id`는 제거한다. direct open과 recent open 모두 Core가 선택한 동일한 `DatabaseId`를 `select`에 전달한다.

Browser persistence의 `String::from_utf8_lossy(database_id)`와 desktop persistence의 ASCII 가정은 HKDF 결과와 호환되지 않는다. 두 host 모두 encoded database ID를 IndexedDB key 및 cache directory 이름으로 사용한다.

## Open API

`open`은 descriptor 또는 persisted database ID 중 하나를 받는다. schema에서는 mutually-exclusive target을 나타내는 union을 사용한다.

```ts
type OpenArgs =
  | { storage: StorageDescriptor }
  | { databaseId: string };
```

Direct open은 다음 순서로 동작한다.

1. descriptor의 `provider`로 Core storage wrapper를 조회한다.
2. wrapper의 normalized path로 `DatabaseId`를 derive하고 persistence namespace를 선택한다.
3. raw descriptor path로 `stat`을 수행하고 `Selection`에 descriptor와 wrapped provider를 저장한다.

Recent open은 다음 순서로 동작한다.

1. plaintext recent state에 존재하는 opaque `databaseId`인지 확인한다.
2. 해당 persistence namespace를 선택하고 descriptor/provider 없는 locked selection을 만든다.
3. unlock 뒤 encrypted config에서 descriptor를 읽고 static provider registry에서 wrapper를 조회한다.
4. descriptor의 normalized path에서 다시 derive한 ID가 selected ID와 같은지 확인한 뒤 descriptor와 provider를 selection에 채운다.

`OpenRecentDatabase` operation과 관련 schema/type/UI 호출은 제거한다. recent 목록에서는 `open({ databaseId })`를 호출한다.

## Persisted State와 Recent

database별 encrypted config에는 storage descriptor만 보관한다.

```rust
pub(crate) struct PersistedConfig {
    pub version: u8,
    pub settings: KeelessConfig,
    pub storage: Option<StorageDescriptor>,
}
```

plaintext global recent state에는 credential-bearing descriptor를 저장하지 않는다.

```rust
pub struct RecentDatabase {
    pub id: String,
    pub name: String,
    pub storage_type: String,
    pub last_opened_at_ms: i64,
}
```

recent state는 무제한으로 유지한다. `MAX_RECENT_DATABASES`와 length validation, record truncation을 제거한다. `getRecentDatabases`는 전체 목록을 반환하고 `OpenRecentFragment`만 최신 5개를 렌더링한다. 오래된 record도 cache, journal, encrypted state cleanup을 위해 보존한다.

recent state helper와 operation handler는 분리한다.

* `operations/recent.rs`: state load/save 및 `record_success` helper
* `operations/get_recent_databases.rs`: get handler
* `operations/delete_recent_database.rs`: delete handler
* `operations/open.rs`: direct/recent target을 모두 처리

selected recent는 unlocked 상태에서 삭제하지 못하게 유지한다. locked 상태에서는 delete가 selection을 비운 뒤 cache, journal, encrypted state, recent record를 purge한다. 이 동작은 root mismatch 뒤 recent를 지우고 다시 direct open하는 recovery 경로를 제공한다.

## Storage Provider API

`getStorageDescriptor`는 credential-bearing path를 반환할 수 없으므로 `getStorageProvider`로 교체한다.

```ts
type GetStorageProviderResult = {
  provider: string | null;
};
```

Sidebar는 provider 문자열로 host storage label을 찾는다. `OperationResource::StorageDescriptor`도 `StorageProvider`로 이름을 맞춘다.

## WebDAV

WebDAV provider는 BrowserCore 생성 시 한 번만 static registry에 넣는다. `configureWebDav`, `BrowserStorageConfigurer`, `WebDavProvider::new(base_url, auth)` 형태의 runtime provider 생성은 제거한다.

WebDAV descriptor path는 파일을 가리키는 full HTTP(S) URL이다.

```text
https://user:pa%3AssW0rD!@host.example:443/dav/vault.kdbx
```

`WebDavSetup`은 browser `URL` API를 사용한다.

1. base URL을 HTTP(S) URL로 검증한다.
2. base URL의 userinfo, query, fragment은 거부한다.
3. resource path를 base URL에 안전하게 붙이고 `.` 및 `..` segment를 거부한다.
4. `URL.username`과 `URL.password` setter로 credential을 설정한다.
5. 결과 `url.href`를 `StorageDescriptor.path`로 전달한다.

Static `WebDavProvider`는 매 I/O에서 full URL을 parse한다. userinfo는 percent-decoding한 Basic auth credential으로 사용하고, 실제 request URL에서는 userinfo를 지운다. `get_normalized_path` implementation은 userinfo, query, fragment을 제거한 canonical URL을 반환한다. 따라서 password 또는 username 변경은 `DatabaseId`와 local cache namespace를 바꾸지 않는다.

credential이 path에 포함되므로 다음 위치에서 raw path를 출력하지 않는다.

* WebDAV status, size, parse error
* `SyncError`의 remote path 포함 variant
* `FileHandle`의 `Debug` implementation
* host 및 Core error context

WebDAV-specific redaction helper는 URL을 parse한 뒤 userinfo, query, fragment을 제거한 표시용 URL만 반환한다.

## Root Group Identity

같은 storage identity가 다른 KDBX 파일을 가리키게 된 경우, sync가 unrelated database를 replace하거나 merge하면 안 된다.

`FileHandle::sync_from_remote`에서 remote bytes를 decrypt/parse한 뒤 local database와 remote database의 `root_group_id`를 비교한다.

* clean local handle이 remote를 다운로드하기 전 비교한다.
* dirty local handle이 three-way merge를 시작하기 전 비교한다.
* ID가 다르면 dedicated `SyncError::RootGroupMismatch`를 반환한다.
* 실패 시 handle, cache, journal은 변경하지 않는다.

Core는 이를 별도 operation error code로 map하고 sync status를 error로 유지한다. 사용자는 database를 lock하고 selected recent를 delete한 뒤, 새 database를 direct open한다.

Unlock 화면에는 recent 선택 화면으로 이동하는 경로를 추가한다. `/open`이 locked selection을 항상 unlock 화면으로 redirect하면 locked selected recent를 삭제할 UI 경로가 없으므로, explicit database-change 경로에서는 이 redirect를 피한다.

## Host 변경

### Browser

* startup registry에 persistent IndexedDB와 static WebDAV wrapper를 등록한다.
* local-file은 `configureLocalFile`이 session wrapper를 등록한다.
* `BrowserDatabasePersistence`는 Core가 derive한 ID를 선택하고 encoded ID로 IndexedDB state key를 만든다.

### Desktop

* `DesktopStorageConfigurer`를 제거한다.
* picker가 반환한 canonical absolute local path는 descriptor path로 유지한다.
* `DesktopDatabasePersistence`는 Core가 derive한 encoded ID만으로 cache directory를 선택한다.
* `LocalFileStorage`는 persistent wrapper로 등록한다.

## 검증

* schema JSON 및 generated TypeScript에서 `OpenArgs` union, `getStorageProvider`, 제거된 config/recent-open operation을 검증한다.
* direct descriptor open과 recent ID open이 같은 persistence namespace를 선택하는지 검증한다.
* WebDAV password에 `:` 등 reserved character가 있어도 URL encoding과 Basic auth가 정확한지 검증한다.
* WebDAV credential 변경 후 normalized path와 `DatabaseId`가 유지되는지 검증한다.
* WebDAV error/debug output에 password, username, query credential이 나타나지 않는지 검증한다.
* Browser local-file이 encrypted storage config 및 recent state에 기록되지 않는지 검증한다.
* 6개 이상 recent가 저장되고 API는 전체를 반환하지만 UI는 5개만 표시하는지 검증한다.
* locked selected recent delete가 selection과 local persistence를 함께 제거하는지 검증한다.
* root group ID mismatch가 clean pull과 dirty merge 모두에서 cache/journal을 보존하며 실패하는지 검증한다.
