## TODO

### Release Blocking
- [x] enforce_auto_lock을 dispatch 시에 구현하지말고 자체 timeout으로 작업하게
- [x] desktop용 cache + journal로 저장하는 storage wrapper
- [x] vhid 구현 (Linux)
- [x] DB 내보내기 기능
- [x] 설정 UI
- [x] Attachment 다운로드 가능
- [x] icon picker 수정
- [x] 엔트리 검색 로직 러스트로 일원화 [skim](https://github.com/skim-rs/skim)사용, `tag:` 쿼리, `in:` 쿼리
- [x] Trash에 전부 삭제 추가
- [x] auto save 및 dirty status 보여주기
- [x] desktop에서 register client가 blindly register 시키는 것 막기
- [ ] WebDAV 싱크 테스트 하기
- [ ] setuplayout에서 위에 <- Back으로 하게, router로 이동하게
- [ ] per-database config store, per-database lesswire key upgrade
- [ ] scoped approved lesswire key (app, passkey, extension)
- [ ] Windows WebAuthn plugin authenticator 구현
- [ ] extension + native messaging host 구현
- [ ] 화면 캡쳐 방어
- [ ] SetSecurityInfo (메인 프로세스 / 렌더러 프로세스 원격 스레드, 덤프 차단)
- [ ] config에 systemd 서비스 등록화면
- [ ] operation별 rate limiting 추가
- [ ] TOTP 구현

### Good to have
- [ ] Yubikey Support
- [ ] SSH Agent
- [ ] Field Reference (= spr)
  - kdbx 쪽에 파싱 기능 추가
  - entry detail build할 때 치환 (resolved_value로), View 시에만 작동하게
  - is_resolve_protected로 protected field가 있을 때 protected여부를 전파시키게
