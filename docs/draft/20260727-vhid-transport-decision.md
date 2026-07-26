# vhid 전송 계층 결정: soft-fido2 미채택

`docs/draft/20260716-structure.md`의 `keeless_vhid` 항목은 `soft-fido2-transport` 사용을 전제했다.
구현 착수 전 해당 크레이트(v0.15.0, 2026-07-22)를 평가한 결과 **채택하지 않고 자체 구현**하기로 한다.

## 평가 대상

crates.io에서 `soft-fido2 0.15.0`, `soft-fido2-transport 0.15.0` 소스를 받아 검토했다.

## 판단 근거

### 1. 장시간 동의 대기를 지원할 수 없다 (결정적)

`soft_fido2_transport::CommandHandler`는 동기 트레이트다.

```rust
pub trait CommandHandler {
    fn handle_command(&mut self, cmd: Cmd, data: &[u8]) -> Result<Vec<u8>>;
}
```

`CtapHidHandler::process_packet`이 이 메서드를 인라인 호출하고 그 반환값으로 즉시 응답 패킷을 만든다.
크레이트 전체(`soft-fido2`, `soft-fido2-transport`)에 `async`/`tokio`/`futures` 사용이 **한 곳도 없다**.

keeless의 CTAP 처리는 다음을 요구한다.

- DB 잠금 해제 프롬프트 + 사용자 동의 다이얼로그 대기 (수 초 ~ 2분)
- 그 동안 100ms 주기 **CTAPHID KEEPALIVE**(`STATUS_UPNEEDED`) 방출 — 없으면 브라우저가 장치를 죽은 것으로 간주
- 대기 중 **CTAPHID CANCEL** 수신 시 진행 중인 작업 중단
- 대기 중에도 다른 채널의 INIT/PING 응답

이 크레이트는 세 가지 모두 불가능하다.

- KEEPALIVE는 `Cmd::Keepalive` enum 값으로만 존재하고 **한 번도 전송되지 않는다**. 수신 시에는 `InvalidCmd` 에러를 반환한다.
- `Cmd::Cancel`은 `ChannelManager::cancel_channel`로 패킷 조립 상태만 비운다. 실행 중인 핸들러에는 전달되지 않으며, 동기 호출이라 전달할 방법도 없다.
- 핸들러가 블로킹하는 동안 패킷 루프 자체가 멈추므로 다른 채널도 응답 불가.

우회하려면 `CommandHandler` 안에서 채널을 통해 작업을 다른 스레드로 넘기고 즉시 반환한 뒤 KEEPALIVE와 지연 응답을 직접 써야 하는데, 그 시점에 `CtapHidHandler`가 하는 일이 남지 않는다.

### 2. 라이선스가 AGPL-3.0이다 (승인 범위와 다름)

계획 승인 시점의 정보는 GPL-3.0이었으나, 두 크레이트의 `LICENSE` 파일 실물은 **GNU Affero General Public License v3**이다
(`Cargo.toml`은 `license-file = "LICENSE"`만 지정해 crates.io 메타데이터에 정확히 드러나지 않는다).

AGPL은 네트워크 상호작용까지 소스 제공 의무를 확장한다. keeless는 MIT이고 vhid 데몬은 IPC로 로컬 앱과만 통신하므로
실무상 위험은 제한적이지만, 승인받은 조건(GPL-3.0 바이너리 1개)과 다른 조건을 임의로 수용할 수 없다.

### 3. `uhid` 모듈만 떼어 쓸 실익이 없다

`UhidDevice`는 단독 사용 가능하지만:

- 논블로킹 read + `Ok(None)` 반환 방식이라 호출자가 busy-poll해야 한다. tokio `AsyncFd` 연동을 위해 어차피 감싸야 한다.
- `UHID_GET_REPORT`/`UHID_SET_REPORT` 이벤트를 무시한다. 커널이 응답을 기다리는 이벤트라 처리하지 않으면 클라이언트가 멈출 수 있다.
- 이벤트 상수 `UHID_OPEN = 2`, `UHID_START = 4`가 커널 UAPI와 뒤바뀌어 있다
  (`include/uapi/linux/uhid.h`: `UHID_START = 2`, `UHID_STOP = 3`, `UHID_OPEN = 4`, `UHID_CLOSE = 5`).
  둘 다 받아들이는 코드라 동작은 하지만 정확하지 않다.
- 실질 내용은 `/dev/uhid`에 UAPI 구조체를 write/read하는 것뿐이며 구조체 정의는 커널 헤더가 곧 사양이다.

즉 200줄 남짓을 얻자고 바이너리 전체에 AGPL을 지우는 셈이다.

## 결정

**`packages/vhid`에서 uhid 접근과 CTAPHID 프레이밍을 직접 구현한다.** 저장소 전체가 MIT로 유지된다.

- `uhid.rs` — `include/uapi/linux/uhid.h`(v6.12 확인) 기준 `#[repr(C, packed)]` 구조체, `CREATE2`/`INPUT2`/`OUTPUT`/`GET_REPORT`/`SET_REPORT`/`DESTROY` 처리, tokio `AsyncFd` 기반 논블로킹 I/O
- `ctaphid.rs` — 64바이트 리포트 프레이밍(init/cont, BCNT, seq), CID 할당, INIT/PING/CBOR/CANCEL/KEEPALIVE/ERROR, 채널별 트랜잭션 상태
- CTAP2 명령 처리는 `packages/ctap`(`keeless_ctap`)에서 Linux/Windows 공용으로 구현

참고 자료(구현에 사용, 코드 복사 없음): FIDO CTAP 2.1 사양의 USB HID 섹션, Linux `include/uapi/linux/uhid.h`.

## 재평가 조건

soft-fido2가 비동기 핸들러 + KEEPALIVE + CANCEL 전달을 지원하고 라이선스가 완화되면 전송 계층 교체를 재검토할 수 있다.
그 전까지 `20260716-structure.md`의 "soft-fido2-transport 이용하여 구현" 문구는 본 문서로 대체된다.
