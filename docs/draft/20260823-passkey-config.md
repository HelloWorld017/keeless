https://github.com/HelloWorld017/keeless에서 설정에 Passkey 탭을 만들고 싶어.

1. desktop 패키지에 PasskeyConfig를 추가하고 설정 탭으로 노출시키기
2. passkey-linux에 `--enable` `--disable` 추가하기
   1. polkit 등으로부터 sudo 권한을 얻어서 systemd 서비스 설치, `--disable` 은 해당 서비스 비활성화
   2. `setup` 커맨드는 제거
   3. `doctor` 커맨드의 로직을 완전히 수정
      1. 현재 서비스가 잘 실행중인지 체크
      2. uhid device가 잘 만들어졌는지 체크
3. passkey-windows의 doctor에 현재 appx로 실행되고 있는지 추가하기 (`GetCurrentPackageFullName`)
4. 설정 탭에서 desktop으로부터 패스키 실행상태를 얻어와서 shadcn 카드 형식으로 보여주기
5. 패스키 활성화 / 비활성화 버튼 추가하기

자세한 구현계획을 세워줘

====


현재 `master` 구조를 기준으로 보면, 이번 작업은 **“Passkey 상태/제어를 각 플랫폼 CLI가 담당하고, desktop은 그 CLI를 안전한 IPC로 감싸서 설정 UI에 노출한다”**는 구조로 잡는 것이 가장 깔끔합니다.

특히 Linux는 기존에도 systemd의 `OpenFile=/dev/uhid:uhid`를 받아 사용하는 코드가 이미 있고, 문서에서도 daemon은 root가 아니라 **desktop과 동일 사용자로 실행되어야 한다**고 명시되어 있습니다. 따라서 기존 `setup`의 udev 그룹 권한 부여 방식을 제거하고, root인 systemd manager가 `/dev/uhid`를 열어 일반 사용자 daemon에 FD만 전달하도록 전환하는 것을 권장합니다. 

## 1. 먼저 상태 모델을 확정

가장 먼저 Linux/Windows doctor와 desktop UI 사이에 사용할 **공통 논리 모델**을 정하는 것이 좋습니다.

CLI의 사람이 읽는 출력 문자열을 desktop이 파싱하게 만들지 말고, 양쪽 `doctor`에 `--json`을 추가하는 방향을 권장합니다.

예를 들면 desktop에서 사용하는 타입은 다음 정도면 충분합니다.

```ts
type PasskeyState = {
  platform: 'linux' | 'windows';
  state: 'enabled' | 'disabled' | 'degraded' | 'unsupported';
  enabled: boolean;
  checks: PasskeyCheck[];
};

type PasskeyCheck = {
  id: string;
  label: string;
  status: 'ok' | 'warning' | 'error';
  detail?: string;
};
```

Linux라면 다음처럼 반환합니다.

```json
{
  "platform": "linux",
  "state": "enabled",
  "enabled": true,
  "checks": [
    {
      "id": "service",
      "label": "Passkey service",
      "status": "ok",
      "detail": "running"
    },
    {
      "id": "uhid",
      "label": "Virtual FIDO device",
      "status": "ok",
      "detail": "available"
    }
  ]
}
```

여기서 **`enabled`와 `healthy`에 해당하는 상태를 분리하는 것이 중요**합니다.

* systemd 서비스가 enable되어 있음 + active + UHID 존재 → `enabled`
* enable되어 있지만 service 또는 UHID에 문제 → `degraded`
* 서비스가 disable되어 있고 실행도 안 됨 → `disabled`
* systemd/OpenFile 등을 지원하지 않음 → `unsupported`

그래야 UI에서 “사용자가 비활성화한 정상 상태”와 “활성화했는데 망가진 상태”를 구분할 수 있습니다.

---

# 2. `passkey-linux`: setup을 systemd 기반 enable/disable로 교체

현재 CLI는 `setup`, `doctor`, `reset-pairing`, `run`을 직접 분기하고 있고, `setup`은 `/dev/uhid` 접근 권한을 주기 위한 udev rule 명령을 출력하는 구조입니다. 

최종 CLI 인터페이스는 다음 형태를 추천합니다.

```text
keeless-passkey-linux [run] [--desktop <path>]
keeless-passkey-linux --enable [--desktop <path>]
keeless-passkey-linux --disable
keeless-passkey-linux doctor [--json]
keeless-passkey-linux reset-pairing
```

### 내부적으로는 `Command` enum으로 바꾸기

현재의 문자열 기반 분기보다 아래처럼 명시적인 enum으로 정리합니다.

```rust
enum Command {
    Run(RunOptions),
    Enable(EnableOptions),
    Disable,
    Doctor(DoctorOptions),
    ResetPairing,
}
```

이 과정에서:

* `setup` 완전 제거
* `pub mod setup` 제거
* `setup_command()` 제거
* `uhid.rs`의 `"run keeless-passkey-linux setup"` 에러 문구 제거
* README의 Setup 섹션 교체
* 기존 setup 관련 테스트 제거

현재 `uhid.rs`는 systemd로 전달된 `uhid` FD를 먼저 사용하고, 없으면 `/dev/uhid`를 직접 여는 fallback을 이미 구현하고 있습니다. 따라서 daemon 쪽 핵심 데이터 경로를 크게 건드릴 필요는 없습니다. 

---

# 3. Linux `--enable`: privileged installer 구조

여기서는 **daemon 자체를 root로 실행하면 안 됩니다.**

현재 README에서도 daemon은 desktop과 같은 사용자여야 하고 root가 되어서는 안 된다고 명시되어 있습니다. 또한 기존 구현은 systemd가 `/dev/uhid`를 열어 descriptor로 전달하는 구조를 이미 지원합니다. 

따라서 다음 구조가 적절합니다.

```text
Electron / CLI
   │
   │ keeless-passkey-linux --enable
   ▼
unprivileged process
   │
   │ pkexec
   ▼
privileged install phase
   │
   ├─ modprobe uhid
   ├─ modules-load.d 설치
   ├─ daemon binary를 stable path에 복사
   ├─ systemd unit 설치
   ├─ systemctl daemon-reload
   └─ systemctl enable --now
                │
                ▼
       daemon process
       User=<desktop user>
                │
       systemd OpenFile=/dev/uhid:uhid
```

즉 **root 권한은 설치 및 FD open에만 사용**하고 Passkey daemon은 계속 일반 사용자로 돌립니다.

### systemd unit

대략 다음 구조를 목표로 합니다.

```ini
[Unit]
Description=Keeless Passkey Provider
After=user@1000.service
Requires=user@1000.service

[Service]
Type=simple
User=1000
Environment=XDG_RUNTIME_DIR=/run/user/1000
ExecStart=/usr/libexec/keeless/keeless-passkey-linux run --desktop ...
OpenFile=/dev/uhid:uhid
Restart=on-failure
RestartSec=1

[Install]
WantedBy=multi-user.target
```

실제 구현에서는 사용자별 instance 서비스로 두는 것을 추천합니다.

```text
keeless-passkey@1000.service
```

이렇게 하면 여러 Linux user account가 Keeless를 사용해도 충돌하지 않습니다.

기존 프로젝트가 이미 systemd 253+의 `OpenFile=` 방식을 문서화하고 있으므로 그 전제를 그대로 공식 installation path로 승격시키면 됩니다. 

### `/dev/uhid` 준비

기존 `setup.rs`에서 다음 부분만 유지할 가치가 있습니다.

```text
/etc/modules-load.d/keeless-uhid.conf
```

내용:

```text
uhid
```

`--enable` privileged phase에서:

1. `/etc/modules-load.d/keeless-uhid.conf` 생성
2. `modprobe uhid`
3. `/dev/uhid` 존재 확인
4. service 설치

순으로 처리합니다.

반대로 기존의:

* `keeless-uhid` 그룹
* `/etc/udev/rules.d/70-keeless-uhid.rules`
* `/dev/uhid`에 0660 권한 부여
* `gpasswd`

는 모두 제거하는 것을 권장합니다.

현재 코드 자체도 `/dev/uhid` write 권한이 사실상 arbitrary virtual keyboard 생성 권한이라는 점을 강조하고 있습니다. systemd FD 전달 방식이면 일반 사용자에게 그 광범위한 권한을 영구적으로 줄 필요가 없습니다. 

---

# 4. polkit 권한 상승 구현

`--enable`이나 `--disable`이 실행되었다고 해서 처음부터 전체 프로그램을 root 프로세스로 만들지 말고 **2단계 self-elevation** 방식으로 구현하는 것이 좋습니다.

외부 인터페이스:

```text
keeless-passkey-linux --enable
```

내부적으로:

```text
keeless-passkey-linux --internal-install-service ...
```

같은 숨겨진 privileged action을 두는 방식입니다.

흐름은:

```text
--enable
  │
  ├─ 이미 root인가?
  │    └─ privileged phase
  │
  └─ 일반 사용자
       └─ pkexec <current-exe> --internal-install-service ...
```

`pkexec`로 상승한 경우 원래 호출 사용자의 UID를 반드시 보존해서 그 사용자를 systemd 서비스의 `User=`로 사용해야 합니다.

### 보안상 주의점

desktop renderer가 다음 값을 넘길 수 있게 만들면 안 됩니다.

```text
service name
install path
실행할 arbitrary command
systemctl args
```

renderer는 오직:

```ts
setPasskeyEnabled(true)
setPasskeyEnabled(false)
```

만 요청해야 합니다.

또 root phase에서 현재 바이너리를 설치할 때는 가능하면 전달받은 arbitrary path를 복사하지 말고 Linux의:

```text
/proc/self/exe
```

를 stable location으로 복사하는 것을 추천합니다.

예:

```text
/usr/libexec/keeless/keeless-passkey-linux
```

이렇게 하면 privilege escalation 도중 “복사할 파일 경로가 바뀌는” 공격면도 줄일 수 있습니다.

---

# 5. `--disable`

`--disable`은 uninstall과 구분하는 것을 추천합니다.

구현은:

```text
systemctl disable --now keeless-passkey@<uid>.service
```

까지만 수행합니다.

즉 남겨두는 것:

* `/usr/libexec/.../keeless-passkey-linux`
* systemd unit
* modules-load config

제거하는 것:

* 없음

서비스만 **stop + disable**합니다.

이렇게 해야 설정 화면에서 다시 “활성화”를 눌렀을 때 자연스럽게 돌아옵니다. `--enable` 실행 때 현재 앱에 포함된 최신 binary를 다시 복사하면 버전 갱신도 가능합니다.

---

# 6. Linux `doctor`는 완전히 새로 작성

현재 doctor는 사실상 `setup::readiness()`와 desktop pairing/reachability를 확인합니다. 즉 이번 요구사항과는 구조 자체가 맞지 않으므로 기존 함수에 조건 몇 개를 추가하는 방식보다 별도 `doctor.rs`를 만드는 것이 좋습니다. 

추천 검사 항목은 세 가지입니다.

### A. 서비스 enable 여부

```text
systemctl show keeless-passkey@<uid>.service
```

에서 최소한:

```text
LoadState
UnitFileState
ActiveState
SubState
```

를 읽습니다.

사람이 보는 `systemctl status` 문자열을 파싱하지 않는 편이 좋습니다.

이를 통해:

```text
installed
enabled
active/running
```

을 각각 알 수 있습니다.

### B. 서비스가 실제 실행 중인지

요구사항의 첫 번째 doctor 항목입니다.

판정:

```text
ActiveState=active
SubState=running
```

이면 성공.

서비스가 enable되어 있지만 crashed 상태라면:

```text
service: failed
```

로 별도 표시합니다.

### C. Keeless UHID device가 실제 생성됐는지

여기서 `/dev/uhid` 존재 여부만 보면 안 됩니다.

`/dev/uhid`는 **가상 HID를 만드는 control device**이고, 요구하신 것은 Keeless가 만든 **실제 virtual FIDO device가 존재하는지** 확인하는 것입니다.

현재 코드에서 UHID 생성 후 kernel이 `/dev/hidraw*`를 노출하며, Keeless는 자체 VID/PID `0x1209 / 0x5031`을 사용합니다. 

따라서:

```text
/sys/class/hidraw/hidraw*/device/uevent
```

를 순회해서 다음 identity를 확인하는 방식을 추천합니다.

```text
HID_NAME=Keeless
VID=1209
PID=5031
```

가능하면 `phys/uniq`까지 함께 확인합니다.

그리고 이 identity 상수는 doctor에서 새로 하드코딩하지 말고 `uhid.rs`에서 공통으로 노출합니다.

예:

```rust
pub(crate) const DEVICE_NAME: &str = "Keeless";
pub(crate) const VENDOR_ID: u32 = 0x1209;
pub(crate) const PRODUCT_ID: u32 = 0x5031;
```

### doctor 출력

사람용:

```text
Passkey service: enabled
Service process: running
Virtual FIDO device: available
```

비활성 상태:

```text
Passkey service: disabled
Service process: stopped
Virtual FIDO device: not present
```

문제 상태:

```text
Passkey service: enabled
Service process: running
Virtual FIDO device: missing
```

마지막 케이스만 `degraded`입니다.

`--enable` 직후에는 systemd가 active가 되고 hidraw 생성까지 작은 race가 있을 수 있으므로 enable 완료 후 **짧게 몇 차례 doctor를 retry**한 다음 desktop에 최종 상태를 반환하도록 하는 것도 좋습니다.

---

# 7. Windows doctor에 AppX identity 검사 추가

현재 Windows CLI에는 이미 `--enable`, `--disable`, `doctor`가 있고, doctor는 WebAuthn Plugin API load 여부와 provider enabled 여부를 출력합니다. 

여기에:

```rust
current_package_full_name() -> Result<Option<String>, String>
```

같은 helper를 추가합니다.

Microsoft 문서상 `GetCurrentPackageFullName`은 **호출 프로세스 자체의 package identity**를 반환하며, package identity가 없으면 `APPMODEL_ERROR_NO_PACKAGE`, buffer 크기를 먼저 알아낼 때는 `ERROR_INSUFFICIENT_BUFFER`를 반환합니다. ([Microsoft Learn][1])

구현 순서는:

```text
length = 0
GetCurrentPackageFullName(&length, null)

APPMODEL_ERROR_NO_PACKAGE
    → Not packaged

ERROR_INSUFFICIENT_BUFFER
    → Vec<u16>(length)
    → GetCurrentPackageFullName(...)
    → package full name 획득

그 외
    → doctor error
```

doctor 출력은 예를 들면:

```text
Windows WebAuthn plugin APIs: available
Keeless provider: enabled
App package: packaged (dev.nenw.keeless.passkey_...)
```

또는:

```text
App package: unpackaged
```

현재 AppX manifest의 package identity는 `dev.nenw.keeless.passkey`이고 packaged executable은 `resources\bin\keeless-passkey-windows.exe`입니다. 

---

# 8. Desktop에 Passkey 관리 API 추가

현재 `DesktopBridge`에는 client IPC, file picker, window control 등이 있고 preload가 각각 `ipcRenderer.invoke()`로 노출하고 있습니다. 

여기에 다음 두 개 정도만 추가하는 것을 추천합니다.

```ts
interface DesktopBridge {
  // existing...

  getPasskeyState(): Promise<PasskeyState>;
  setPasskeyEnabled(enabled: boolean): Promise<PasskeyState>;
}
```

IPC는:

```text
desktop:get-passkey-state
desktop:set-passkey-enabled
```

두 개면 충분합니다.

`setPasskeyEnabled()`는 작업이 끝난 후 다시 doctor를 실행해 **새로운 PasskeyState를 반환**하게 합니다.

즉 renderer는:

```ts
await window.keelessDesktop.setPasskeyEnabled(true);
```

한 번만 호출하면 됩니다.

---

# 9. Desktop main에서 플랫폼별 CLI 실행

별도 `PasskeyService` 모듈을 만드는 편이 좋습니다.

예:

```text
packages/desktop/src/passkey/
  index.ts
  types.ts
  runner.ts
```

역할:

```text
getPasskeyState()
    ├ Linux   → keeless-passkey-linux doctor --json
    └ Windows → keeless-passkey-windows doctor --json

setPasskeyEnabled(true)
    ├ Linux   → keeless-passkey-linux --enable --desktop ...
    └ Windows → keeless-passkey-windows --enable

setPasskeyEnabled(false)
    ├ Linux   → keeless-passkey-linux --disable
    └ Windows → keeless-passkey-windows --disable
```

프로세스 실행에는 `shell: true`나 command string을 쓰지 말고 `spawn`/`execFile` + argument array를 사용합니다.

### Linux `--desktop` 경로

서비스가 앱을 필요할 때 다시 켤 수 있으려면 enable 과정에서 안정적인 desktop executable path를 전달해야 합니다.

Linux 배포물이 현재 AppImage이므로 production에서는 가능하면:

```text
process.env.APPIMAGE
```

의 원본 AppImage 경로를 사용하고, 존재하지 않거나 안정적인 경로를 판단할 수 없는 개발 환경에서는 `--desktop`을 생략하는 fallback을 두는 것이 좋습니다. 현재 daemon 자체도 `--desktop`이 없으면 이미 실행 중인 desktop과 연결하는 동작을 지원합니다. 

---

# 10. Linux binary를 desktop package에 포함

이 부분도 이번 작업 범위에 사실상 들어가야 합니다.

현재 desktop 패키지에는 `@keeless/passkey-windows`만 devDependency로 있고 `passkey-linux`는 들어 있지 않습니다. 

따라서:

```json
"@keeless/passkey-linux": "workspace:*"
```

추가가 필요합니다.

현재 binary Vite plugin도 Windows passkey binary에 대해서만 non-Windows skip 처리를 하고 있으므로 Linux binary에 대해서도 반대쪽 guard를 추가합니다. 

예:

```ts
if (platform !== 'linux' && name.startsWith('keeless-passkey-linux')) {
  return 'export default undefined;';
}
```

그리고 main bundle에서:

```ts
import passkeyLinuxBinary from 'binary:keeless-passkey-linux';
```

형태로 asset을 build하도록 합니다.

### electron-builder

현재 Windows binary/MSIX는 `extraResources`에 있지만 Linux에는 passkey binary가 없습니다. 

Linux에도:

```yaml
linux:
  extraResources:
    - from: dist/main/assets/keeless-passkey-linux
      to: bin/keeless-passkey-linux
```

를 추가합니다.

production desktop은:

```text
process.resourcesPath/bin/keeless-passkey-linux
```

를 실행하고, `--enable`이 그 자신을 `/usr/libexec/keeless/...`로 복사하도록 하면 AppImage mount 경로 문제도 피할 수 있습니다.

---

# 11. `PasskeyConfig`를 desktop에 추가

이 부분은 현재 앱 구조와 잘 맞습니다.

`ConfigDialog`는 built-in `General`, `Database` 뒤에 `useExtraConfig()`의 config들을 그대로 붙여 탭을 만듭니다. 

그리고 desktop renderer에는 현재:

```ts
const integration: AppIntegration = {
  hostOverride: desktopHost,
  hasNativePasswordInput: true,
};
```

가 있으므로 여기에 desktop 전용 Passkey 설정을 주입하면 됩니다. 

추천 위치:

```text
packages/desktop/src/renderer/components/PasskeyConfig.tsx
```

그리고:

```ts
const integration: AppIntegration = {
  hostOverride: desktopHost,
  hasNativePasswordInput: true,
  extraConfig: [
    {
      category: 'Passkey',
      component: PasskeyConfig,
    },
  ],
};
```

이렇게 하면 browser/extension 쪽에는 Passkey 탭이 생기지 않고 **desktop에만 노출**됩니다.

---

# 12. shadcn Card UI

현재 app 패키지에는 이미 `card.tsx`, `button.tsx`, `badge.tsx` 등의 shadcn primitive가 있습니다. ([GitHub][2])

`PasskeyConfig`가 desktop 패키지에 있어야 하므로 desktop 패키지에도 shadcn을 추가합니다.

UI는 한 장의 큰 Card면 충분합니다.

```text
┌──────────────────────────────────────────┐
│ Passkey                         Enabled  │
│ Use Keeless as a system passkey provider│
│                                          │
│ Passkey service                  Running │
│ Virtual FIDO device              Ready   │
│                                          │
│                         [ Disable ]      │
└──────────────────────────────────────────┘
```

Windows:

```text
┌──────────────────────────────────────────┐
│ Passkey                         Enabled  │
│                                          │
│ Windows WebAuthn API             Ready   │
│ Keeless provider               Enabled   │
│ App package                   Packaged   │
│                                          │
│                         [ Disable ]      │
└──────────────────────────────────────────┘
```

`degraded`라면 상단 badge만:

```text
Needs attention
```

으로 바꾸고 실패한 check에 detail을 표시하면 됩니다.

---

# 13. 활성화 / 비활성화 버튼 동작

컴포넌트 상태는 복잡하게 만들 필요가 없습니다.

```ts
const [state, setState] = useState<PasskeyState>();
const [pending, setPending] = useState(false);
const [error, setError] = useState<string>();
```

mount:

```text
getPasskeyState()
```

Enable:

```text
pending=true
→ setPasskeyEnabled(true)
→ 반환된 state 저장
→ pending=false
```

Disable도 동일합니다.

버튼은:

* disabled 상태 → `Enable passkey`
* enabled/degraded → `Disable passkey`
* 실행 중 → spinner + disabled

로 두면 됩니다.

Linux에서는 `Enable`을 누르면 이 시점에 `pkexec` 인증창이 뜨게 됩니다.

사용자가 polkit 인증을 취소한 경우에는 이를 “Passkey 고장”으로 취급하지 말고:

```text
Authentication was cancelled.
```

정도의 action error만 보여준 뒤 기존 status를 유지하는 것이 좋습니다.

---

# 14. 파일별 변경 예상

핵심 변경 파일을 정리하면 다음과 같습니다.

### `packages/passkey-linux`

```text
src/lib.rs
  - setup 제거
  - --enable / --disable
  - doctor --json
  - command enum 정리

src/setup.rs
  - 삭제

src/service.rs          [신규]
  - privilege escalation
  - unit 생성
  - install/enable/disable
  - uhid module setup

src/doctor.rs           [신규]
  - systemd state 검사
  - sysfs UHID 검사
  - JSON/human 출력용 diagnostic model

src/uhid.rs
  - device identity 상수 공유
  - setup 안내 문구 제거

README.md
  - Setup 문서 제거
  - --enable/--disable 문서
```

### `packages/passkey-windows`

```text
src/com.rs
  - doctor에 package check 추가
  - 가능하면 doctor JSON 지원

src/package_identity.rs [신규 권장]
  - GetCurrentPackageFullName wrapper
```

### `packages/desktop`

```text
package.json
  - @keeless/passkey-linux dependency

vite.config.ts
  - Linux passkey binary 처리

electron-builder.yml
  - Linux binary extraResources

src/types/DesktopBridge.ts
  - PasskeyState
  - getPasskeyState()
  - setPasskeyEnabled()

src/renderer/preload.ts
  - IPC bridge

src/index.ts
  - IPC handlers
  - 또는 passkey/index.ts로 분리

src/passkey/index.ts    [신규 권장]
  - 플랫폼별 binary 선택
  - doctor
  - enable/disable

src/renderer/components/PasskeyConfig.tsx [신규]
  - shadcn Card
  - 상태 표시
  - enable/disable
```

---

# 15. 테스트 계획

Linux 쪽은 root/systemd가 개입하므로 **순수 로직과 실제 integration을 분리**하는 것이 중요합니다.

### Linux unit test

* CLI가 `setup` 거부
* `--enable`, `--disable` parsing
* systemd unit 생성 결과
* path/newline 등의 unit injection 방지
* `systemctl show` 결과 → state 변환
* 임시 sysfs fixture → UHID 검출
* `enabled + active + HID` → enabled
* `enabled + active + no HID` → degraded
* `disabled + inactive + no HID` → disabled
* privileged phase에서 호출 UID 검증

특히 sysfs root를 코드에 바로 `/sys`로 박지 말고:

```rust
fn find_uhid_device(sysfs_root: &Path)
```

형태로 만들어 테스트 가능하게 하는 것을 추천합니다.

### Windows test

`GetCurrentPackageFullName` FFI 자체보다는 return code를 해석하는 부분을 분리해:

* `APPMODEL_ERROR_NO_PACKAGE`
* `ERROR_INSUFFICIENT_BUFFER`
* 성공
* 예상하지 못한 오류

를 테스트합니다.

### Desktop

프로세스 실행기를 하나의 함수/객체로 추상화해서:

* Linux doctor JSON
* Windows doctor JSON
* malformed JSON
* process failure
* polkit cancel
* enable 후 상태 refresh

를 테스트할 수 있게 합니다.

---

# 16. 구현 순서

의존성을 고려하면 이 순서가 가장 안전합니다.

1. **Passkey 상태 JSON contract 정의**
2. **Linux service installer + `--enable/--disable`**
3. **Linux doctor 완전 교체**
4. **기존 setup/udev-group 코드 제거**
5. **Windows doctor AppX 검사 + JSON**
6. **desktop에 Linux binary packaging 추가**
7. **DesktopBridge + main IPC 구현**
8. **PasskeyConfig + extraConfig 등록**
9. **Card 상태 UI**
10. **Enable/Disable 버튼**
11. **Linux AppImage + Windows NSIS/MSIX 실제 설치 테스트**
12. **README/CLI usage 정리**

CLI부터 끝낸 후 UI를 붙이는 편이 좋습니다. UI와 권한 상승을 동시에 개발하면 문제가 “renderer ↔ IPC ↔ child process ↔ polkit ↔ systemd ↔ UHID” 중 어디에 있는지 찾기 어려워집니다.

## 최종 완료 기준

이번 작업은 다음 상태가 되면 완료됐다고 볼 수 있습니다.

* Desktop 설정에 **Passkey** 탭이 desktop에서만 나타남.
* Linux에서 `setup` 명령이 더 이상 존재하지 않음.
* Linux `--enable` → polkit 인증 → systemd service 설치/enable/start.
* 해당 daemon은 **root가 아닌 desktop 사용자**로 실행됨.
* `/dev/uhid` 권한을 사용자에게 직접 부여하지 않고 systemd `OpenFile=`로 전달함.
* Linux `doctor`가 **systemd service running 여부 + 실제 Keeless virtual HID 존재 여부**를 검사함.
* Linux `--disable`이 service를 stop/disable함.
* Windows doctor에 `GetCurrentPackageFullName` 결과가 포함됨.
* desktop이 두 플랫폼의 doctor 결과를 구조화된 상태로 읽음.
* 설정 Card에서 `Enabled / Disabled / Needs attention`을 구분함.
* 설정에서 Enable/Disable을 누르면 완료 후 상태가 즉시 갱신됨.
* renderer가 임의 command/path를 privileged process에 전달할 수 없음.

구조적으로 가장 중요한 결정 하나를 꼽으면 **Linux에서 “사용자에게 `/dev/uhid` 권한을 주는 방식”을 폐기하고 “root systemd가 FD만 열어 동일 사용자 daemon에 전달하는 방식”으로 완전히 통일하는 것**입니다. 기존 코드가 이미 그 FD-passing을 구현하고 있어서 변경량 대비 보안성과 사용성이 가장 좋습니다. 

[1]: https://learn.microsoft.com/en-us/windows/win32/api/appmodel/nf-appmodel-getcurrentpackagefullname "GetCurrentPackageFullName function (appmodel.h) - Win32 apps | Microsoft Learn"
[2]: https://github.com/HelloWorld017/keeless/tree/master/packages/app/src/components "keeless/packages/app/src/components at master · HelloWorld017/keeless · GitHub"

