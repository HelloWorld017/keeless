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
