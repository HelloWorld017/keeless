# Fuzzy Search PRD

## Summary

엔트리 검색과 SearchCommand의 빠른 검색을 프런트엔드 `fuzzysort`와 Rust의 단순 부분 문자열 검색으로 나누지 않는다. 모든 fuzzy 매칭과 순위 계산은 Rust에서 수행하고, app은 결과를 표시하고 검색어를 조작하는 역할만 맡는다.

검색어는 일반 검색어 외에 `in:<group>`과 `tag:<tag>` 필터를 지원한다. 예를 들어 `tag:passkey in:소셜 insta`는 `소셜` 그룹의 직접 엔트리 중 `passkey` 태그를 가지며 `insta`와 fuzzy match되는 엔트리를 반환한다.

SearchCommand에서는 태그와 그룹 결과를 Enter로 선택하면 각각의 페이지로 이동한다. Tab으로 선택하면 태그 또는 그룹을 검색 필터로 확정하고 자유 검색어를 제거한다. 그룹 및 태그의 엔트리 목록 header에서도 현재 범위가 입력된 SearchCommand를 열 수 있어야 한다.

## Background

현재 검색은 두 개의 서로 다른 방식으로 동작한다.

| 위치 | 현재 방식 | 문제 |
| --- | --- | --- |
| `SearchCommand.tsx` | 브라우저에서 `fuzzysort`로 엔트리, 그룹, 태그, Trash를 각각 검색 | 실제 검색 결과와 순위가 다르고, 보호 필드 및 DB 가시성 정책을 공유하지 못한다. |
| `query/search.rs` | Rust에서 필드별 부분 문자열 점수 계산 | fuzzy match가 아니며 SearchCommand의 빠른 결과와 일관되지 않다. |

이 구조는 검색 문법을 추가할 때 프런트와 백엔드에 각각 parser와 matcher를 작성하게 만들며, 웹 WASM host와 네이티브 host 사이의 동작을 동일하게 보장하기 어렵다.

## Goals

1. 엔트리, 그룹, 태그, Trash의 fuzzy 후보 순위 계산을 Rust로 단일화한다.
2. `in:` 및 `tag:` 필터를 검색 결과 페이지와 SearchCommand에서 동일하게 해석한다.
3. 기존의 데이터베이스 잠금, 메모리 보호, 휴지통/템플릿 제외 정책을 유지한다.
4. SearchCommand의 태그 및 그룹 결과에서 Enter와 Tab이 서로 다른 의도를 명확히 수행하게 한다.
5. 그룹/태그 목록에서 해당 범위의 검색을 한 번의 클릭으로 시작하게 한다.
6. 브라우저 WASM과 네이티브 host에서 동일한 matcher와 parser가 컴파일되고 동작하게 한다.

## Non-Goals

1. 정규식 검색의 문법 또는 `SearchParameters`의 public 의미를 변경하지 않는다.
2. 필터 부정, OR, 괄호식, 필드별 검색(`url:`, `user:` 등)을 이번 범위에 포함하지 않는다.
3. 그룹의 모든 하위 그룹을 자동으로 검색하지 않는다. `in:`은 직접 엔트리만 대상으로 한다.
4. 검색어 하이라이트, 최근 검색어 저장, 검색 히스토리, 쿼리 URL 영속화는 포함하지 않는다.
5. 기존 태그/그룹 라우트의 목록 의미를 바꾸지 않는다.

## Product Decisions

### Matcher

- Rust 1.85와 WASM 빌드 호환성을 위해 `icu_normalizer` 1.5와 `nucleo-matcher` 0.3.1을 사용한다.
- `icu_normalizer`로 후보와 검색어를 NFKD로 정규화한다. 이는 한글 음절과 호환 자모 초성 입력을 같은 Jamo 시퀀스로 맞추고, 악센트 등 결합 문자를 fuzzy match에 활용하게 한다. 종성/초성 및 복합 모음의 대응은 locale-specific한 별도 규칙 없이는 포함하지 않는다.
- `nucleo-matcher`로 정규화된 문자열을 case-insensitive fuzzy match하고 점수를 얻는다.
- matcher는 큰 scratch buffer를 재사용하므로 요청 하나 안에서 후보마다 새 matcher를 만들지 않는다.
- `nucleo-matcher`의 다중 단어 패턴을 사용해 `insta gram`처럼 공백으로 나뉜 단어가 모두 일치해야 한다.

### Filter Scope

- `in:<group>`은 이름이 일치하는 그룹의 **직접 엔트리**만 포함한다.
- 동일한 이름을 가진 그룹이 여러 개면 그 그룹들의 직접 엔트리를 모두 포함한다.
- `tag:<tag>`은 태그가 정확히 일치하는 엔트리만 포함한다. 엔트리에 공백이 포함된 태그가 있어도 기존 태그 조회와 동일하게 trim 후 비교한다.
- 같은 종류의 필터가 여러 개 있으면 모두 충족해야 한다. 예: `tag:work tag:urgent`는 두 태그를 모두 가져야 한다.
- `in:`과 `tag:`를 함께 쓰면 두 필터를 모두 충족해야 한다.
- 존재하지 않는 그룹 또는 태그 필터는 빈 엔트리 결과를 만든다.

## Query Language

### Syntax

| 입력 | 의미 |
| --- | --- |
| `insta` | 전체 가시 엔트리에서 `insta` fuzzy 검색 |
| `tag:passkey` | `passkey` 태그가 있는 엔트리 |
| `in:소셜` | `소셜` 그룹의 직접 엔트리 |
| `tag:passkey in:소셜 insta` | 두 필터를 적용한 뒤 `insta` fuzzy 검색 |
| `in:"Social Media" tag:"two factor"` | 공백이 포함된 그룹/태그 이름 필터 |

### Parsing Rules

1. 공백으로 토큰을 구분하되, 큰따옴표 안의 공백은 값의 일부로 유지한다.
2. 값이 비어 있지 않은 `in:`과 `tag:` 토큰만 필터로 해석한다.
3. 알려지지 않은 prefix와 값이 비어 있는 prefix는 일반 검색어로 유지한다. 예: `type:login`, `tag:`.
4. 인식된 필터를 제외한 토큰을 순서대로 합쳐 fuzzy 검색어를 만든다.
5. 인용부호가 닫히지 않은 경우 남은 문자열은 일반 검색어로 취급한다. 입력 중간 상태에서 검색이 실패하거나 예외가 나면 안 된다.
6. parser는 원본 filter 토큰과 자유 검색어 영역도 보존한다. SearchCommand가 Tab 완성 시 원래 필터를 보존하고 자유 검색어만 교체하는 데 사용한다.

### Empty Free Text

`tag:passkey` 또는 `in:소셜`처럼 자유 검색어가 없으면 필터에 맞는 엔트리를 반환한다. 이 결과는 엔트리 이름순이 아니라 기존 목록의 안정적인 순서를 유지한다. fuzzy 점수는 자유 검색어가 있을 때만 결과 순위에 사용한다.

## Search Semantics

### Entry Candidates

검색 대상은 현재 웹 SearchCommand가 조합하는 필드와 정확히 동일한 title, username, URL, tags다. notes, custom fields, password는 fuzzy 검색에 포함하지 않는다.

보호된 문자열은 credential이 있을 때에만 현재 요청의 `MemoryUnlockSession`에서 평문으로 접근한다. credential이 없으면 unsealed 필드만 검색한다. 결과 summary는 현재와 같이 보호된 title, username, URL을 노출하지 않는다.

각 필드는 독립적으로 fuzzy match하고 가장 적합한 필드 점수를 사용한다. 같은 fuzzy 점수일 때는 다음 우선순위를 적용해 현재 검색의 title 우선 동작을 유지한다.

| 필드 | 상대 우선순위 |
| --- | --- |
| title | 가장 높음 |
| username | 높음 |
| URL, tag | 보통 |

동점은 데이터베이스에 이미 정의된 안정적인 엔트리 순서로 결정한다.

### Visibility

- recycle bin 하위 엔트리는 일반 fuzzy 엔트리 검색에서 제외한다.
- entry template 그룹 및 그 하위 엔트리는 제외한다.
- DB 그래프가 비정상인 경우에도 기존 `all_entries(database, true)`와 같은 방식으로 중복 없이 가시 엔트리만 처리한다.
- SearchCommand의 그룹 후보에서는 recycle bin을 제외하고 root group은 기존 UI와 동일하게 노출하지 않는다.
- SearchCommand의 태그 후보는 가시 엔트리에 연결된 태그만 포함한다.

### SearchCommand Candidates

SearchCommand는 하나의 Rust operation으로 다음 결과를 받는다.

| 종류 | 후보 텍스트 | 선택 동작 |
| --- | --- | --- |
| Entries | title, username, URL, tags | 엔트리 detail을 연다. |
| Groups | group name | Enter는 그룹 페이지를 열고, Tab은 `in:` filter를 완성한다. |
| Tags | tag name | Enter는 태그 페이지를 열고, Tab은 `tag:` filter를 완성한다. |
| Navigation | `Trash` / `Recycle Bin` | Trash 페이지를 연다. |

각 종류는 최대 8개를 반환한다. 그룹과 태그 후보는 parser가 분리한 자유 검색어로 매칭한다. 따라서 `in:aaa passk`에서도 `passkey` 태그 후보가 표시되어 Tab으로 `tag:passkey`를 추가할 수 있다.

## User Experience

### SearchCommand 기본 흐름

1. 사용자는 sidebar Search 버튼 또는 `Ctrl+P`로 command dialog를 연다.
2. 검색어를 입력하면 app은 짧은 debounce 후 `searchFuzzy` 요청을 수행한다.
3. 반환된 Entries, Search, Groups, Tags, Navigation 섹션을 현재 순서로 표시한다.
4. Search 항목에서 Enter를 누르면 전체 fuzzy 엔트리 결과 페이지로 이동한다.
5. Entries, Groups, Tags, Trash 항목에서 Enter를 누르면 각각의 현재 라우팅 동작을 수행한다.

### Group and Tag Tab Completion

| 현재 입력 | 선택 항목 | 키 | 결과 |
| --- | --- | --- | --- |
| `aa` | Groups / `aaa` | Enter | `/group/<aaa id>`로 이동 |
| `aa` | Groups / `aaa` | Tab | 입력이 `in:aaa`가 됨 |
| `passkey` | Tags / `passkey` | Enter | `/tag/passkey`로 이동 |
| `passkey` | Tags / `passkey` | Tab | 입력이 `tag:passkey`가 됨 |
| `in:aaa passk` | Tags / `passkey` | Tab | 입력이 `in:aaa tag:passkey`가 됨 |
| `tag:passkey insta` | Tags / `instagram` | Tab | 입력이 `tag:passkey tag:instagram`이 됨 |

Tab은 현재 선택된 command item이 group 또는 tag일 때만 가로챈다. 그 외의 경우에는 브라우저의 기본 focus 이동 동작을 유지한다.

Tab 완성은 다음 순서로 수행한다.

1. parser가 인식한 기존 `in:`/`tag:` 토큰을 그대로 유지한다.
2. 자유 검색어 토큰 전체를 제거한다.
3. 선택한 group 또는 tag를 안전하게 quote한 `in:<name>` 또는 `tag:<name>` 토큰을 추가한다.
4. 입력 focus는 유지하고 backend 검색 결과를 새 query로 갱신한다.

그룹과 태그 결과에는 `Tab` shortcut을, 전체 Search 결과에는 `Enter` shortcut을 표시한다. 그룹 및 태그의 Enter 동작은 변경하지 않는다.

### Entry List Header Search

`EntryListHeader`는 현재 `EntryQuery` 안에 있는 다음 UI를 독립 컴포넌트로 가진다.

- title과 엔트리 수
- 새 엔트리 추가 버튼
- 템플릿으로부터 추가 메뉴
- 새 검색 버튼

검색 버튼은 다음 화면에만 표시한다.

| 목록 화면 | SearchCommand 초기 입력 |
| --- | --- |
| Group | `in:<group name>` |
| Tag | `tag:<tag name>` |

All Entries와 Trash에는 이 버튼을 표시하지 않는다. 그룹명 또는 태그명에 공백이나 query 문법 문자가 있으면 parser가 읽을 수 있도록 quote/escape한다.

SearchCommand는 열릴 때 optional initial query를 받는다. sidebar에서 연 경우는 빈 문자열, header에서 연 경우는 위 token으로 초기화한다. dialog가 닫힐 때만 입력 상태를 폐기하며, Tab 완료 중에는 dialog를 닫지 않는다.

## Technical Design

### KDBX Query Layer

`packages/kdbx/src/kdbx/query/search.rs`

- 기존 `SearchParameters`, regex search, 단순 검색 helper를 유지한다.
- `SearchQuery`와 token parser를 추가한다.
- parser는 `search_fuzzy`가 import할 수 있는 public query type을 제공한다.

`packages/kdbx/src/kdbx/query/search_fuzzy.rs`

- 새 `FuzzySearchHelper`를 둔다.
- `icu_normalizer`와 `nucleo-matcher`를 감싼 공용 fuzzy score helper를 제공한다.
- 엔트리용 검색은 memory unlock session을 이용해 필드를 안전하게 읽고 score 및 entry ID를 반환한다.
- 공용 문자열 후보 검색 API는 core가 그룹, 태그, Trash 후보에 동일 matcher를 적용할 수 있게 한다.
- 결과는 score 내림차순, 안정적인 원본 순서 오름차순으로 정렬한다.

`packages/kdbx/src/kdbx/query/mod.rs`

- 새 module과 필요한 public type을 export한다.

### Core and Schema

기존 `searchEntries`는 fuzzy 엔트리 검색으로 전환한다. 전체 검색 페이지는 operation 이름과 request shape을 유지하므로 app route state와 기존 caller의 호환성을 유지한다.

SearchCommand에는 별도의 `searchFuzzy` operation을 추가한다. `EntriesResult`에 관계없는 데이터를 억지로 추가하지 않고 전용 result를 사용한다.

제안 request/result shape:

```ts
type SearchFuzzyArgs = {
  query: string;
};

type SearchFuzzyResult = {
  entries: EntrySummary[];
  groups: GroupHierarchyItem[];
  tags: TagSummary[];
  trashMatches: boolean;
};
```

`SearchFuzzyResult`에는 각 category별 최대 8개만 담는다. `searchEntries`는 제한 없이 검색 결과 페이지에 필요한 엔트리를 반환한다.

변경 대상:

- `packages/schema/src/lib.rs`: operation args, result, request/response enum 추가
- `packages/schema/src/bin/export-types.rs`: TypeScript export 추가
- `packages/schema/index.d.ts`: generate 명령으로 재생성
- `packages/core/src/operations/search_entries.rs`: `FuzzySearchHelper`로 전환
- `packages/core/src/operations/search_fuzzy.rs`: command 결과 조합 및 가시성 정책 적용
- `packages/core/src/operations/mod.rs`: operation dispatch 등록

### App

`SearchCommand.tsx`는 `fuzzysort` import와 로컬 `fuzzySearch` 함수를 제거한다. 입력 query를 기준으로 `searchFuzzy` request를 사용하고 pending/error 상태를 기존 message 스타일로 표시한다.

`DatabaseFragment.tsx`는 다음을 관리한다.

- dialog open state
- SearchCommand initial query state
- 전체 검색 route에 저장할 submitted query map
- sidebar와 `EntryList`가 공통으로 호출하는 `openSearch(initialQuery)` callback

`EntryList.tsx`는 header JSX를 제거하고 `EntryListHeader.tsx`를 사용한다. group/tag route는 해당 scope query를 만들어 `onOpenSearch`로 전달한다.

`packages/app/package.json`과 `pnpm-lock.yaml`에서는 `fuzzysort`를 제거한다.

## Error Handling

- parser는 사용자 입력에 대해 error를 반환하지 않는다. 불완전한 quote나 빈 filter도 일반 검색어로 안전하게 처리한다.
- DB가 잠긴 상태의 request는 기존 operation과 같은 `DatabaseLocked` error를 사용한다.
- 보호 문자열을 열 수 없는 경우에는 해당 필드를 건너뛰며, 잘못된 credential으로 인한 unlock 실패는 현재 search operation과 동일하게 request 전체를 실패시킨다.
- SearchCommand request가 pending인 동안에는 이전 결과를 표시하지 않고 loading message를 표시한다.
- backend error일 때 command dialog는 열린 상태로 유지하고 검색 실패 안내를 표시한다.

## Performance Requirements

1. matcher는 request 안에서 재사용하며 엔트리/필드마다 생성하지 않는다.
2. SearchCommand는 입력마다 즉시 모든 후보를 요청하지 않고 debounce한다.
3. command 결과는 category별 8개만 정렬/직렬화한다. 구현은 전체 후보의 정확한 상위 8개를 보장해야 한다.
4. `searchEntries`는 결과 페이지의 완전한 결과를 보존하며 command limit을 적용하지 않는다.
5. 새로운 의존성은 `wasm32-unknown-unknown` target에서 빌드되어야 한다.

## Acceptance Criteria

### Query and Results

- [ ] `tag:passkey in:소셜 insta`가 parser에서 태그, 그룹, 자유 검색어로 올바르게 분리된다.
- [ ] 공백이 포함된 quoted group/tag 이름이 동작한다.
- [ ] 같은 이름의 그룹이 여러 개이면 해당 그룹들의 직접 엔트리가 모두 검색된다.
- [ ] `in:`이 하위 그룹의 엔트리를 포함하지 않는다.
- [ ] 두 개 이상의 `tag:` 및 `in:` filter가 AND로 동작한다.
- [ ] fuzzy 결과는 title match를 username-only match보다 우선한다.
- [ ] NFKD 정규화로 악센트와 한글 호환 자모 입력도 fuzzy match된다.
- [ ] 보호 필드, recycle bin, templates의 현재 검색 정책이 유지된다.
- [ ] `fuzzysort`가 app dependency와 코드에서 제거된다.

### SearchCommand

- [ ] command의 엔트리, 그룹, 태그, Trash 후보가 Rust `searchFuzzy` 결과만 사용한다.
- [ ] group result에서 Enter는 그룹 페이지로 이동한다.
- [ ] group result에서 Tab은 dialog를 닫지 않고 `in:<name>` filter를 추가하며 자유 검색어를 제거한다.
- [ ] tag result에서 Enter는 태그 페이지로 이동한다.
- [ ] tag result에서 Tab은 dialog를 닫지 않고 `tag:<name>` filter를 추가하며 자유 검색어를 제거한다.
- [ ] `aa`에서 group `aaa`를 Tab으로 선택하면 `in:aaa`가 된다.
- [ ] `in:aaa passk`에서 tag `passkey`를 Tab으로 선택하면 `in:aaa tag:passkey`가 된다.
- [ ] 다른 item이 선택된 상태의 Tab은 기본 focus 동작을 유지한다.
- [ ] 후보가 없을 때는 Enter로 전체 검색을 수행할 수 있다.

### Entry List Header

- [ ] group header 검색 버튼은 `in:<group>`을 입력한 command dialog를 연다.
- [ ] tag header 검색 버튼은 `tag:<tag>`를 입력한 command dialog를 연다.
- [ ] All Entries와 Trash header에는 검색 버튼이 없다.
- [ ] 기존 entry 추가 및 template 추가 동작과 loading/error 처리가 유지된다.

## Test Plan

### Rust Unit Tests

`keeless_kdbx`:

- parser의 일반/필터/quoted/incomplete input case
- fuzzy ranking과 다중 단어 matching
- NFKD normalization 및 한글 호환 자모 case
- tag 및 direct-group filter 조합
- title, username, URL, tags 외의 필드를 fuzzy 검색에서 제외하는지 확인
- protected/unsealed field handling

`keeless_core`:

- `searchEntries`의 relevance, visibility, protection policy regression
- `searchFuzzy`의 entries/groups/tags/Trash category result
- command result의 category별 limit과 recycle bin 제외

### App Verification

- TypeScript typecheck와 lint로 새 schema operation, header props, cmdk keyboard event를 검증한다.
- 브라우저에서 Ctrl+P, tag Enter, tag Tab, scoped header search를 수동 확인한다.

### Required Commands

```sh
pnpm --filter @keeless/schema generate
cargo test -p keeless_kdbx
cargo test -p keeless_core --lib
pnpm --filter @keeless/host-browser typecheck
pnpm --filter @keeless/app check
```

## Rollout and Compatibility

- `searchEntries` request shape은 유지한다. 저장된 search route의 UUID-to-query map 동작도 유지한다.
- fuzzy ranking은 기존 부분 문자열 점수와 다르므로 결과 순서 변경은 의도된 product change다.
- 새로운 `searchFuzzy` operation은 app과 schema를 같은 release에서 배포한다. 이전 host와 새 app을 섞어 쓰는 compatibility layer는 추가하지 않는다.
- feature flag나 데이터 migration은 필요하지 않다.
