# Stable Custom Entry Field IDs

## 배경

KDBX는 standard field를 제외한 entry string에 식별자를 저장하지 않는다. 현재
`EntryFieldId::Custom(Uuid::new_v4())`를 import 시점마다 생성하므로, 같은 KDBX를
다시 열면 custom field ID가 바뀐다.

이 ID는 이미 RPC와 mutation journal에서 사용된다. 예를 들어 `getEntryTotp`,
`revealEntryFields`, `updateEntry`는 field ID를 받는다. 따라서 ID가 바뀌면 다음과
같은 문제가 생긴다.

* `saveDatabase`의 sync가 직렬화한 KDBX를 다시 열면, 화면에 남은 TOTP field ID가
  더 이상 존재하지 않는다.
* background sync 또는 cache 재오픈 뒤 protected field reveal, copy, popout, edit
  draft가 stale field ID를 사용한다.
* mutation journal의 `updateEntry`가 저장한 기존 custom field ID가 재오픈 뒤
  일치하지 않아 replay가 실패할 수 있다.

custom field ID는 KDBX에 별도 메타데이터로 저장하지 않는다. 대신 custom field의
이름과 같은 이름의 field 중 순서로부터 결정적으로 생성한다.

## 불변식

* standard field ID는 기존 `standard:<name>` 형식을 유지한다.
* custom field ID는 고정된 Keeless namespace UUID와 `name + occurrence`로 만든 UUID
  v5이다.
* occurrence는 같은 entry 안에서 같은 custom field 이름이 등장하는 0-based 순서다.
* field ID는 entry ID와 함께 RPC에 전달되므로 custom ID 생성 입력에 entry ID를
  포함하지 않는다. 이 규칙은 `EntryFields`의 serde deserialize 시 entry ID를 얻을 수
  없는 점도 피한다.
* 이름은 KDBX string key의 현재 의미와 일치하도록 exact, case-sensitive로 비교한다.
* Keeless가 수정하는 entry의 custom field 이름은 유일해야 한다.

UUID v5 입력은 모호하지 않게 custom field 이름의 UTF-8 bytes, 구분자, occurrence의
고정 폭 정수 bytes를 연결한다. namespace와 입력 형식은 private implementation
contract로 문서화하고 변경하지 않는다.

## Legacy Duplicate Fields

KeePass DB에는 같은 이름의 custom field가 존재할 수 있다. 현재 모델과 테스트도
이를 허용한다. 기존 DB 호환을 위해 import, 표시, reveal은 계속 지원한다.

* import와 deserialize는 같은 이름의 각 field에 occurrence 기반 ID를 부여한다.
* `updateEntry`의 완전한 desired field 목록에 duplicate custom name이 있으면
  `InvalidEntryUpdate`로 거부한다.
* 따라서 legacy duplicate entry는 읽기 전용으로 남는다. 사용자는 duplicate를
  제거하거나 이름을 바꾼 뒤 수정할 수 있다.

이 검증은 core 요청 처리보다 낮은 `Database::prepare_entry_update`에 둔다. 일반
update, journal replay, 향후 다른 mutation 경로가 같은 불변식을 공유해야 하기
때문이다.

## Model Changes

`packages/kdbx/src/model/entry/mod.rs`에 custom ID 생성 helper를 둔다.

* `EntryFields`를 입력 순서대로 구성할 때 standard field는 standard ID를, custom
  field는 이름별 occurrence를 센 결정적 ID를 사용한다.
* serde `EntryFields::deserialize`와 XML `Entry::add_imported_field`가 이 helper를
  사용한다.
* `Entry::add_custom_field`도 새 field의 현재 occurrence로 ID를 만든다.
* caller가 임의 UUID를 제공할 수 있는 `add_custom_field_with_id` 사용을 제거하거나
  결정적 ID 검증을 거치게 한다.
* UUID v5 기능을 사용하도록 `uuid` crate feature를 추가한다.

`EntryUpdate`는 새 custom field에 대한 랜덤 UUID 목록을 더 이상 받지 않는다.
`prepare_entry_update`는 final requested field 목록에서 custom ID를 다시 계산해
`IndexMap`을 만든다.

* 기존 field ID는 원본 field를 조회하고 plaintext를 복호화하는 데만 사용한다.
* rename된 custom field는 새 이름 기반 ID를 받는다.
* 값 또는 protection만 바뀐 custom field는 ID를 유지한다.
* duplicate custom name은 final map을 만들기 전에 거부한다.

## Mutation And Journal Changes

랜덤 custom ID를 mutation payload에 기록하는 경로를 제거한다.

* `update_entry::Mutation`과 KDBX `EntryUpdate`에서 `new_custom_field_ids`를 제거한다.
* passkey 등록의 update payload도 새 custom ID를 생성하지 않는다.
* template instantiation의 `link_field_id`는 field 이름으로부터 결정되므로 random UUID
  payload와 `TemplateInstantiationOptions`에서 제거한다.

아직 배포된 프로그램과 생성된 기존 journal이 없으므로 journal version은 올리지 않고
migration도 만들지 않는다. 새 payload 형식에 맞춰 journal serialization test만 갱신한다.

결정적 ID를 쓰면 cache 재오픈 뒤 journal replay가 base DB의 기존 custom field를 같은
ID로 조회할 수 있다. 새 custom field도 replay 중 final name으로 동일한 ID를 얻는다.

## Sync And UI

결정적 field ID는 DB 교체 후 stale identity 문제를 제거하지만, sync가 원격 변경을
반영할 수 있다는 사실은 그대로다.

`saveDatabase`는 `databaseStatus`뿐 아니라 `entry`, `group`, `tag`, `customIcon`을
mutate하는 operation으로 선언한다. 성공한 sync는 local write, three-way merge, remote
pull을 통해 이 resource들을 바꿀 수 있다.

이 변경으로 수동 save 뒤 active `getEntryDetail` query가 다시 로드된다. background
sync의 변경 알림과 화면 freshness는 별도 관심사다. 결정적 field ID 전환은 그 경우에도
stale ID 오류는 막지만, 원격 변경을 즉시 화면에 반영할 notification 또는 revision
boundary는 이 작업의 범위에 포함하지 않는다.

## Tests

다음 회귀 테스트를 추가하거나 갱신한다.

* 같은 DB를 save/open 또는 serde round-trip한 뒤 custom field ID가 유지된다.
* legacy duplicate custom field는 occurrence별로 결정적이고 서로 다른 ID를 갖는다.
* duplicate custom field를 포함한 `updateEntry`는 atomic하게 실패한다.
* custom field rename은 ID를 변경하고, 값과 protected memory binding은 유지한다.
* custom protected OTP를 update한 뒤 sync/save해도 기존 field ID로 `getEntryTotp`가
  성공한다.
* custom protected field update가 cache 재오픈 뒤 journal replay에 성공한다.
* 기존 duplicate-field update 테스트는 read compatibility와 update rejection을
  검증하도록 바꾼다.
* `saveDatabase` operation metadata가 모든 sync-affected query resource를
  invalidate하는지 검증한다.
