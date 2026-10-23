# 데스크톱 패키징 및 배포 초안

상태: 구현 전 설계 초안.

## 요구사항 및 결정 사항

1. Linux 데스크톱 앱을 deb와 Arch `.pkg.tar.zst`로 배포한다.
2. 앱 설치 시 패스키 서비스를 설치하거나 시작하지 않는다. 일반 Linux에서는 기존처럼 앱 설정에서 별도로 설치한다.
3. 앱 삭제 시 설정에서 설치한 패스키 서비스와 관련 시스템 파일도 삭제한다. 업그레이드에서는 서비스 설정을 유지한다.
4. Nix flake로 실제 Electron 데스크톱 앱을 빌드하고, systemd 서비스용 NixOS 모듈을 제공한다.
5. NixOS에서는 앱의 명령형 설치 기능 대신 모듈로 서비스를 선언적으로 관리한다.
6. Windows NSIS는 후순위로 둔다. 기존 구성을 활용하되 MSIX 서명 등 별도 배포 요건을 확인한다.
7. workflow 파일 확장자는 `.yaml`을 사용한다. `build-[platform].yaml`이 플랫폼별 아티팩트를 만들고, `build.yaml`은 전체 플랫폼 빌드 workflow를 호출한다. `release.yaml`은 `build.yaml`을 호출하여 GitHub Release를 생성한다. check와 test는 각각 검사와 테스트를 담당한다.
8. release는 수동 버전 입력과 `v*` 태그 push를 모두 지원한다.
9. 초기 배포 대상은 Linux x86_64다.
10. Nix 배포는 최종 앱 output 하나만 GitHub Release의 flat binary cache로 제공한다. Nix 관련 release asset은 정확히 세 개다.

## 현재 구조

- `packages/desktop`은 Electron 앱이며 `electron-builder.yml`에는 Linux AppImage와 Windows NSIS 설정이 있다.
- Vite가 N-API 모듈과 Rust 실행 파일을 `dist/main/assets`에 내보낸다.
- Linux 패스키 설치 로직은 `packages/passkey-linux/src/service.rs`에 있다.
  - 실행 파일: `/usr/libexec/keeless/keeless-passkey-linux`
  - 사용자별 unit: `/etc/systemd/system/keeless-passkey@<uid>.service`
  - 모듈 자동 로드 설정: `/etc/modules-load.d/keeless-uhid.conf`
  - systemd가 `/dev/uhid`를 열고 `OpenFile=/dev/uhid:uhid`로 일반 사용자 daemon에 전달한다.
- 현재 `--disable`은 서비스를 중지·비활성화하며 설치 파일을 제거하지 않는다.
- 현재 `flake.nix`의 기본 패키지는 존재하지 않는 Cargo 패키지 `keeless_desktop`을 빌드하도록 되어 있다. 실제 Electron 앱 빌드로 교체해야 한다.
- 기존 Windows NSIS include와 MSIX 생성 스크립트는 있으나 배포용 게시자·버전·서명 구성이 필요하다.

## 1. Electron 패키징

### 산출물

- Debian: `.deb`
- Arch Linux: `.pkg.tar.zst`
- Nix: flake package 및 별도 binary cache 메타데이터
- Windows: 가능하면 NSIS `.exe`; Linux/Nix 완료 후 진행

Electron Builder의 Arch 타깃이 실제로 zstd 압축 패키지를 만드는지 확인한다. 필요하면 Arch 패키징 단계를 별도로 두어 확장자와 내부 압축 형식이 일치하도록 한다.

### 리소스와 실행 경로

- `.node` 모듈은 ASAR에서 unpack한다.
- native UI와 패스키 실행 파일은 실행 가능한 실제 파일로 제공한다. ASAR 내부 경로를 프로세스 실행에 사용하지 않는다.
- 설치된 deb/Arch/Nix에서도 패스키 서비스에 올바른 데스크톱 실행 경로를 전달한다. 현재 AppImage만 처리하는 `desktopPath()`를 확장한다.
- Nix에서는 Electron 실행 래퍼를 데스크톱 경로로 사용한다. 이 래퍼가 필요한 환경 변수를 재현하도록 한다.
- 설치된 앱, 개발 환경, Nix 패키지 각각에서 리소스 경로가 맞는지 검증한다.

## 2. Linux 삭제 훅

### 정리 대상

앱 설정이 생성한 모든 사용자별 `keeless-passkey@<uid>.service`에 대해 다음을 수행한다.

1. 서비스를 중지하고 비활성화한다.
2. 생성된 unit과 활성화 링크를 삭제한다.
3. `/usr/libexec/keeless/keeless-passkey-linux`를 삭제한다.
4. `/etc/modules-load.d/keeless-uhid.conf`를 삭제한다.
5. systemd 설정을 reload하고 필요한 failed 상태를 정리한다.

빈 설치 디렉터리는 제거할 수 있으나 다른 파일이 있는 디렉터리를 재귀 삭제하지 않는다. 사용자 데이터베이스와 사용자별 앱 데이터는 정리 대상이 아니다. `uhid` 커널 모듈을 강제로 unload할 필요는 없다.

### 패키지 생명주기

- Debian과 Arch의 삭제/업그레이드 구분을 각각 명시적으로 처리한다.
- Debian에서는 remove/purge에서만 정리하고 upgrade/deconfigure 등의 경로에서는 실행하지 않는다. 사용하는 Electron Builder/FPM 훅이 받는 실제 인자를 확인한다.
- Arch에서는 remove 훅에 정리를 연결하며 upgrade 훅에서는 삭제하지 않는다.
- 서비스 미설치 상태와 반복 실행은 성공해야 한다.
- systemd manager가 실행 중이지 않은 환경에서는 파일 정리를 수행할 수 있어야 한다. 반대로 실행 중인 서비스를 중지하지 못했는데 정상 삭제로 처리하지 않도록 오류 정책을 정한다.
- UID 형식과 앱이 생성한 unit 경로를 기준으로 정리 범위를 제한한다.

업그레이드 때 서비스 설치 파일은 유지한다. 현재 서비스 실행 파일은 앱 리소스에서 복사한 별도 파일이므로, 앱 업데이트 이후 복사본을 갱신하는 정책도 구현 중 확인한다. 최소 요구사항은 업그레이드 중 서비스를 삭제하거나 비활성화하지 않는 것이다.

## 3. Nix 패키지

### flake outputs

예정 인터페이스:

```text
packages.x86_64-linux.default
packages.x86_64-linux.keeless
packages.x86_64-linux.keeless-passkey-linux
nixosModules.default
devShells.x86_64-linux.default
```

기존 개발 셸의 Rust/wasm 개발 기능을 유지하면서 실제 패키지 빌드를 추가한다. 다른 시스템도 노출할 경우 초기 CI 배포 대상과 구분한다.

### 빌드 구조

- 고정된 pnpm lockfile과 Cargo lockfile로 JS 및 Rust 의존성을 준비한다.
- Rust 네이티브 모듈, native UI, Linux 패스키 실행 파일과 프런트엔드를 빌드한다.
- Nixpkgs Electron을 사용하는 래퍼, desktop entry, 아이콘을 설치한다.
- pnpm workspace와 N-API 산출물 경로를 기존 빌드와 일치시킨다.
- JS 패키지 설치·빌드 단계에서 Electron 배포 바이너리를 별도로 다운로드하지 않는다.

### 최종 output 하나로 합치기

최종 앱 output에는 다음을 실제 파일로 복사하여 포함한다.

- 데스크톱 JS와 renderer
- Rust N-API 모듈
- native UI 실행 파일
- Linux 패스키 실행 파일
- 실행 래퍼, 아이콘, desktop entry

프로젝트의 중간 derivation을 가리키는 런타임 store reference를 남기지 않는다. 복사만으로 reference가 사라진다고 가정하지 않고 RPATH, 래퍼, 생성 코드 등을 검사한다. 필요한 프로젝트 내부 경로는 최종 output 내부 경로로 정리한다.

Electron과 시스템 라이브러리는 Nixpkgs store reference를 유지한다. 이 참조들은 공식 `cache.nixos.org`에서 받을 수 있어야 한다. 공식 캐시에 없는 런타임 의존성이 생기면 이를 최종 output에 포함하는 등 패키지 구성을 수정한다. 의존성 NAR을 추가로 업로드하여 세 파일 제한을 우회하지 않는다.

별도 `keeless-passkey-linux` output은 모듈 없이 CLI를 사용할 때를 위한 인터페이스로 제공할 수 있다. 세 파일 캐시로 모듈까지 사용할 수 있도록, 모듈의 기본 서비스 실행 파일은 최종 `keeless` output에 포함된 바이너리를 사용한다.

## 4. NixOS 모듈

예정 사용 예시이며 옵션 이름은 구현 시 확정한다.

```nix
{
  imports = [ inputs.keeless.nixosModules.default ];

  services.keeless-passkey = {
    enable = true;
    users = [ "alice" ];
  };
}
```

- 서비스 활성화는 기본값 false로 둔다.
- 지정한 사용자와 동일한 권한으로 daemon을 실행한다. root daemon이나 일반 사용자의 `/dev/uhid` 직접 접근 권한 부여 방식으로 바꾸지 않는다.
- `boot.kernelModules`로 `uhid`를 로드한다.
- systemd `OpenFile=/dev/uhid:uhid`를 사용한다.
- 사용자 runtime directory와 세션 의존성, 데스크톱 자동 실행 경로를 설정한다.
- 기존 doctor가 사용하는 UID 기반 unit 이름과 일치하도록 구성하거나, 진단 로직을 함께 조정한다.
- root 사용자 지정과 알 수 없는 사용자에 대한 설정 검증을 제공한다.
- 모듈을 비활성화하거나 사용자 목록에서 제거하면 `nixos-rebuild`가 서비스를 중지하고 unit을 제거한다. store 파일의 실제 회수는 Nix GC가 담당한다.

앱 설정에서는 상태 확인을 계속 제공하되 NixOS의 서비스가 선언적으로 관리된다는 안내를 표시한다. 명령형 설치/비활성화 버튼은 사용할 수 없게 하고 모듈 설정 변경을 안내한다. 모듈을 사용하지 않는 NixOS에서도 `/usr`에 복사하는 기존 설치 경로를 실행하지 않도록 구분한다.

## 5. GitHub Release를 Nix binary cache로 사용

### 배포 계약

`build-nixos.yaml`은 최종 `keeless` output 하나만 export한다. Nix 관련 release asset은 다음 세 개만 생성한다.

```text
<store-path-hash>.nar.zst
<store-path-hash>.narinfo
nix-cache-info
```

여기서 hash는 최종 output의 `/nix/store/<hash>-keeless-...` 중 store path hash다. NAR 내용 해시와 혼동하지 않는다. `.nar.zst`는 같은 hash로 이름을 정하고 `.narinfo`의 상대 URL에서 참조한다. `nar/` 하위 디렉터리 없이 GitHub Release asset과 직접 대응시키는 flat 구조다.

`nix-cache-info`의 기본 내용:

```text
StoreDir: /nix/store
WantMassQuery: 1
Priority: 50
```

`.narinfo`에는 다음 필드를 생성한다.

```text
StorePath: /nix/store/<hash>-keeless-<version>
URL: <store-path-hash>.nar.zst
Compression: zstd
FileHash: sha256:<compressed-file-hash>
FileSize: <compressed-file-size>
NarHash: sha256:<uncompressed-nar-hash>
NarSize: <uncompressed-nar-size>
References: <runtime-store-references>
Sig: keeless-1:<signature>
```

필요하면 `Deriver` 등 표준 필드를 추가한다. 해시 인코딩과 signature fingerprint는 Nix가 요구하는 형식에 맞추며, 가능하면 Nix의 store signing 및 메타데이터 도구를 활용한다. 서명은 압축 파일 자체가 아니라 store path의 NAR 메타데이터에 대한 Nix 서명이다.

### 생성 순서

1. 릴리스 대상 revision의 최종 앱 output을 빌드한다.
2. 런타임 참조를 검사하고 프로젝트 중간 output이 남아 있지 않은지 확인한다.
3. 최종 store path를 캐시 개인키로 서명한다.
4. 최종 output 하나의 NAR만 생성하고 zstd로 압축한다. closure를 재귀 export하지 않는다.
5. NAR 메타데이터와 압축 파일 메타데이터로 `.narinfo`를 생성한다.
6. `nix-cache-info`를 생성한다.
7. 산출물 디렉터리에 정확히 세 파일만 있는지 검사한다.
8. 이 세 파일을 Actions 아티팩트로 업로드한다.

`release.yaml`이 `build.yaml`을 통해 생성된 아티팩트를 받아 동일한 세 파일을 release asset으로 업로드한다. deb/Arch 파일은 같은 release의 별도 asset이다.

### 사용자 설정 및 키 관리

```nix
nixConfig = {
  extra-substituters = [
    "https://github.com/HelloWorld017/keeless/releases/latest/download/"
  ];
  extra-trusted-public-keys = [
    "keeless-1:<public-key>"
  ];
};
```

공개키는 저장소와 사용 예시에 게시하고, 개인키는 GitHub Actions secret에 보관한다. 개인키는 임시 파일로만 제공하고 산출물·로그에 포함하지 않는다. 키 생성과 secret 등록 절차를 문서화한다. 사용자의 Nix 설정에 따라 flake의 `nixConfig` 수락 또는 daemon의 신뢰 설정이 필요할 수 있다.

공개키가 없는 PR에서도 빌드는 가능해야 한다. build workflow는 신뢰된 호출에서만 secret을 받아 서명하고, release workflow는 서명된 캐시만 게시하도록 구분한다. 서명 없는 독립 빌드 아티팩트는 배포용으로 취급하지 않는다.

`latest/download`는 최신 정식 릴리스의 output만 제공한다. 과거 revision을 고정한 사용자는 그 output이 최신 릴리스에 없으면 소스 빌드하거나 해당 릴리스의 `/releases/download/<tag>/` URL을 사용한다. prerelease는 GitHub의 latest 정식 릴리스 URL에 자동 반영되지 않으므로 버전별 URL을 사용한다.

### 재현 가능한 output 경로

릴리스 CI와 사용자의 flake 평가가 같은 store path를 계산해야 캐시 적중이 가능하다. 패키지 version 등 derivation 입력을 CI 환경 변수나 현재 시각에서만 주입하지 않는다. 동일한 source revision과 lockfile에 대해 동일한 최종 output 경로가 나오도록 한다. 수동 릴리스 버전과 소스에 선언된 버전의 일치 정책도 함께 정의한다.

## 6. GitHub Actions 구성

| Workflow | 트리거 | 역할 |
| --- | --- | --- |
| `build.yaml` | 수동 실행, `workflow_call` | 전체 플랫폼별 build workflow 호출 및 성공 여부 집계 |
| `build-linux.yaml` | 수동 실행, `workflow_call` | deb/Arch 패키지 생성 및 Actions 아티팩트 업로드 |
| `build-nixos.yaml` | 수동 실행, `workflow_call` | Nix 앱 빌드, 세 파일 캐시 생성 및 Actions 아티팩트 업로드 |
| `build-windows.yaml` | 수동 실행, `workflow_call` | NSIS 구현 시 추가할 Windows 빌드 및 Actions 아티팩트 업로드 |
| `release.yaml` | 수동 버전 입력, `v*` 태그 push | `build.yaml` 호출 후 Release 생성 및 asset 게시 |
| `check.yaml` | push, PR, 수동 실행 | 포맷·린트·타입 검사와 Nix 설정 평가 |
| `test.yaml` | push, PR, 수동 실행 | 테스트 수행 |

모든 workflow는 `.github/workflows/` 아래에 `.yaml` 확장자로 작성한다. 플랫폼별 빌드 파일명은 `build-[platform].yaml`로 통일한다.

```text
release.yaml
└── build.yaml
    ├── build-linux.yaml
    ├── build-nixos.yaml
    └── build-windows.yaml (NSIS 구현 시 추가)
```

### build

- `build.yaml`은 패키징 명령을 직접 실행하지 않고 재사용 가능한 플랫폼별 workflow를 job 단위로 호출한다. 서로 독립적인 플랫폼 빌드는 병렬 실행한다.
- `build.yaml`에 전달된 대상 커밋 SHA와 버전을 각 플랫폼 workflow에 동일하게 전달한다. Nix 서명 secret은 필요한 `build-nixos.yaml` 호출에만 전달한다.
- 플랫폼 workflow는 각자 Actions 아티팩트를 업로드하며 이름이 서로 충돌하지 않도록 한다. `build.yaml` 완료는 활성화된 모든 플랫폼 빌드의 성공을 의미한다.
- Windows 빌드가 준비되면 `build-windows.yaml`을 추가하고 `build.yaml`의 호출 대상에 포함한다.
- 모든 빌드 호출은 대상 커밋 SHA를 명시적으로 checkout한다.
- 동일한 릴리스 버전을 각 패키지 형식에 맞게 전달한다.
- 일반 Linux 패키지는 지원할 배포판의 libc 기준을 고려한 환경에서 빌드한다.
- 플랫폼별 workspace 의존성을 선택하여 Linux 빌드에서 Windows MSIX 생성 단계가 실행되지 않도록 한다.
- Nix 빌드에서 만든 바이너리를 그대로 일반 deb/Arch에 넣어 `/nix/store` 의존성이 생기는 구성을 피한다.
- build workflow 자체는 Release를 생성하거나 asset을 게시하지 않는다.

### release

1. 수동 입력 또는 태그에서 버전을 결정하고 유효성을 검사한다.
2. 태그와 대상 커밋을 확인한다. 수동 실행은 성공한 대상 커밋에 태그를 생성한다.
3. 대상 커밋과 버전을 전달하여 `build.yaml`을 호출한다. `release.yaml`에서 플랫폼별 workflow를 직접 호출하지 않는다.
4. `build.yaml`을 통한 모든 플랫폼 빌드가 성공한 뒤 아티팩트를 내려받는다.
5. 버전과 파일 목록, Nix 서명을 확인한다.
6. GitHub Release를 생성하고 패키지 및 Nix 캐시 세 파일을 첨부한다.

동일 태그 동시 실행을 제한한다. 재실행할 때 기존 asset 처리 정책을 명확히 하고 다른 커밋의 같은 버전으로 바뀌지 않도록 한다. 수동으로 태그를 생성하는 실행은 그 태그 push에 의한 별도 실행에 의존하지 않고 자체적으로 릴리스까지 완료한다.

### check / test

- check는 포맷, 린트, 타입 검사 및 Nix 평가를 담당한다.
- test는 Rust 및 프로젝트에 정의된 테스트를 담당한다.
- 필요한 생성물은 준비하되 배포 패키지와 Release를 생성하지 않는다.
- 권한 및 `/dev/uhid`가 필요한 device 테스트는 일반 runner에서 자동 실행하는 테스트와 구분한다.

## 7. Windows NSIS

- 기존 `build/windows/installer.nsh`와 MSIX 생성 경로를 재사용한다.
- MSIX 게시자·버전, 코드 서명 및 설치 가능한 package identity를 확인한다.
- Linux/Nix 배포를 완료한 후 Windows runner에서 NSIS 빌드 경로를 검증한다.
- unsigned MSIX를 포함한 설치 파일이 실제 설치 가능한 정식 산출물인 것처럼 릴리스되지 않도록 한다. 서명 환경이 준비되지 않으면 Windows 릴리스는 후속 작업으로 남긴다.

## 8. 검증 및 완료 기준

### Linux

- deb와 실제 zstd 압축의 Arch 패키지를 생성하고 파일 목록과 메타데이터를 확인한다.
- 앱 설치 직후 패스키 서비스가 생성되지 않는다.
- 설정에서 서비스 설치 후 앱을 다시 실행할 수 있다.
- 패키지 업그레이드 시 서비스 unit과 활성화 상태가 유지된다.
- 앱 삭제 시 모든 대상 사용자 서비스와 시스템 설치 파일이 제거된다.
- 서비스 미설치, 복수 사용자, 반복 정리, systemd 비실행 환경을 검증한다.
- native UI와 패스키 실행 파일이 패키징된 앱에서 실행된다.

### Nix / NixOS

- flake의 최종 앱 패키지를 실제 빌드한다.
- NixOS 모듈을 평가하고 사용자·서비스·UHID 설정을 확인한다.
- 모듈 활성화와 비활성화에 따른 서비스 생명주기를 검증한다.
- 앱에서 NixOS 선언적 서비스 상태와 관리 안내를 표시한다.
- 최종 output에 프로젝트 중간 output의 런타임 참조가 없다.
- 캐시 산출물은 정확히 세 파일이다.
- `.narinfo` URL, 압축 형식, 크기, 두 종류의 해시와 서명이 유효하다.
- 별도 store 또는 격리된 검증 환경에서 빌드한 앱 output을 미리 제공하지 않고, 공식 캐시와 생성한 캐시만 사용해 다운로드한다. 검증 중 로컬 빌드로 fallback하여 캐시 실패가 숨겨지지 않도록 한다.
- 동일 flake revision에서 실제 대체 다운로드가 성공하고 필요한 공식 의존성을 모두 받을 수 있다.

### 문서

- README에 deb/Arch 설치 및 삭제 동작을 설명한다.
- flake, NixOS 모듈, 공개키와 substituter 설정 예시를 추가한다.
- 키 생성·secret 등록·수동 릴리스·태그 릴리스 절차를 기록한다.
- 검증한 플랫폼 및 NSIS 구현 여부를 명시한다.
