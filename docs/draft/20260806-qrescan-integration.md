# qrescan QR Scanner 통합 계획

## 목표

- 직접 구현한 `jsqr` 기반 `QRScanner`를 `qrescan@0.4.0`으로 교체한다.
- 현재 OTP 등록 화면의 탭과 업로드 드롭존 디자인을 유지한다.
- QR 스캔, 이미지 디코딩, 카메라/화면 공유 lifecycle, QR highlight는 qrescan이 소유한다.
- 기본 public mobile companion을 활성화한다.

## 의존성

- `packages/app/package.json`에 `qrescan@0.4.0`을 추가한다.
- 더 이상 직접 사용하지 않는 `jsqr`를 제거한다.
- `pnpm-lock.yaml`을 갱신한다.
- `qrescan/style.css`를 전역 stylesheet에 Tailwind import보다 먼저 추가한다.
- 앱의 light/dark 토큰을 `--qrescan-background`, `--qrescan-foreground`, `--qrescan-card`, `--qrescan-border`, `--qrescan-primary` 등의 qrescan CSS 변수로 매핑한다.

## QRScanner API

```ts
type AdditionalTab = {
  value: string;
  label: ReactNode;
  children: ReactNode;
};

type QRScannerProps = {
  active: boolean;
  onScan: (value: string) => string | undefined;
  additionalTabs?: AdditionalTab[];
};
```

- `active`가 `false`면 qrescan tree를 렌더링하지 않는다. Dialog가 닫힐 때 media stream과 decoder worker가 정리된다.
- `onScan`의 반환 문자열은 OTP URI 검증 오류다. qrescan의 `onScan`은 void이므로 wrapper가 결과를 상태로 저장해 공통 destructive alert로 표시한다.
- `wechatDecoder`를 사용한다.
- `options={{ mobile: { enabled: true } }}`를 전달한다. `companionURL`은 생략하여 qrescan의 기본 public companion을 사용한다.

## 탭 오버라이드

qrescan 0.4.0의 render props를 이용하되, 실제 DOM은 앱의 `@/components/tabs`로 치환한다.

```tsx
<QRescan.Tabs render={({ children }) => <Tabs>{children}</Tabs>}>
  <QRescan.TabList
    render={({ children }) => (
      <TabsList style={{ gridTemplateColumns: `repeat(${tabCount}, minmax(0, 1fr))` }}>
        {children}
      </TabsList>
    )}
  >
    <QRescan.TabTrigger
      value="scan"
      render={({ children, disabled, isActive, onSelect }) => (
        <TabsTrigger active={isActive} disabled={disabled} onClick={onSelect}>
          {children}
        </TabsTrigger>
      )}
    />
  </QRescan.TabList>

  <QRescan.Tab
    value="scan"
    render={({ children, isActive }) =>
      isActive ? <TabsContent>{children}</TabsContent> : null
    }
  >
    <QRescan.Scan />
  </QRescan.Tab>
</QRescan.Tabs>
```

- 탭 순서는 `scan`, `upload`, `mobile`, `additionalTabs`다.
- `TabsList`의 column 수는 전체 탭 수에 맞춰 동적으로 설정한다.
- qrescan의 `TabTrigger` render prop은 `children`, `isActive`, `disabled`, `onSelect`를 제공한다.
- qrescan의 `Tab` render prop은 `children`, `value`, `isActive`를 제공한다. 현재 앱 탭은 inactive panel을 렌더링하지 않으므로 `isActive`가 false면 `null`을 반환한다.
- qrescan render API는 DOM props나 방향키 navigation handler를 전달하지 않는다. 앱의 현재 탭 컴포넌트와 동일한 탭 상호작용을 유지한다.

## Scan 패널

```tsx
<QRescan.Tab value="scan" render={renderTab}>
  <QRescan.Scan>
    <QRescan.ViewFinder sourceType="stream">
      <QRescan.ViewFinderHighlight />
      <QRescan.CameraSelect render={renderCameraSelect} />
    </QRescan.ViewFinder>
    <QRescan.ScanInitialize>
      <Empty>
        <IconScanLine className="size-8" />
        <EmptyTitle>Scan a QR code</EmptyTitle>
        <EmptyDescription>
          Use a camera or share a screen containing the QR code.
        </EmptyDescription>
        <div className="flex flex-wrap justify-center gap-2">
          <QRescan.ScanInitializeCamera className={outlineButtonClass}>
            <IconCamera />
            Camera
          </QRescan.ScanInitializeCamera>
          <QRescan.ScanInitializeScreen className={outlineButtonClass}>
            <IconMonitor />
            Capture
          </QRescan.ScanInitializeScreen>
        </div>
      </Empty>
    </QRescan.ScanInitialize>
  </QRescan.Scan>
</QRescan.Tab>
```

- `ViewFinder`와 `ViewFinderHighlight`는 qrescan 기본 구현을 사용한다. video element, preview, QR highlight 좌표 및 decode lifecycle은 qrescan이 소유한다.
- `ScanInitialize`는 기존 `Empty` 레이아웃, 문구 및 아이콘을 사용한다. 카메라와 화면 공유 action은 qrescan의 `ScanInitializeCamera`와 `ScanInitializeScreen`으로 연결하고, 기존 outline `Button`과 동일한 class를 적용한다.
- `ViewFinder`의 `sourceType="stream"`은 0.4.0에서 추가된 조건부 렌더링 API다. stream이 없을 때 ViewFinder가 렌더링되지 않으므로 별도 CSS 숨김 규칙은 추가하지 않는다.

- `CameraSelect.render`는 qrescan의 `items`, `selectedItem`, `onSelectItem`만 사용하고 실제 메뉴 DOM은 앱의 `DropdownMenu`로 렌더링한다.

```tsx
const renderCameraSelect = ({ items, selectedItem, onSelectItem }: CameraSelectRenderProps) => (
  <DropdownMenu>
    <DropdownMenuTrigger render={<Button type="button" variant="outline" size="sm" />}>
      {selectedItem?.kind === 'screen' ? 'Screen capture' : (selectedItem?.label ?? 'Camera')}
    </DropdownMenuTrigger>
    <DropdownMenuContent align="end">
      {items.map(item => (
        <DropdownMenuItem key={item.value} onClick={() => onSelectItem(item.value)}>
          {item.kind === 'screen' ? 'Screen capture' : item.label}
        </DropdownMenuItem>
      ))}
    </DropdownMenuContent>
  </DropdownMenu>
);
```

- 기존 `QRScanner`의 video ref, canvas, requestAnimationFrame loop, `jsqr` decoding, QR tracker, 카메라 선택 `Select`는 삭제한다.
- qrescan provider가 stream과 이미지 decode를 수행하므로 ViewFinder의 mount 여부와 decode lifecycle이 분리된다.

## Upload 패널

```tsx
<QRescan.Tab value="upload" render={renderTab}>
  <QRescan.Upload>
    <QRescan.Dropzone
      render={({ error, processFiles }) => (
        <>
          <FileUpload
            value={files}
            accept="image/*"
            maxFiles={1}
            onValueChange={setFiles}
            onAccept={processFiles}
          >
            <FileUploadDropzone>
              <IconImages className="size-8 text-muted-foreground" />
              <p className="font-medium text-foreground">Upload a QR image</p>
              <p className="text-sm text-muted-foreground">
                Drop, paste, or choose an image file.
              </p>
            </FileUploadDropzone>
          </FileUpload>
          {error ? <p role="alert">{error}</p> : null}
        </>
      )}
    />
  </QRescan.Upload>
</QRescan.Tab>
```

- `Dropzone.render`의 `processFiles`가 기존 `FileUpload`의 선택, 드롭, 붙여넣기 결과를 qrescan provider에 전달한다.
- `error`는 qrescan의 파일 형식, 파일 개수, 이미지 QR decode 오류를 기존 destructive alert 스타일로 표시한다.
- provider가 이미지 decode를 수행하므로 숨긴 `ViewFinder`나 별도 image decoder는 필요 없다.

## Mobile 패널

```tsx
<QRescan.Tab value="mobile" render={renderTab}>
  <QRescan.Mobile>
    <QRescan.MobileDescription />
    <QRescan.MobileError />
    <QRescan.MobileConnection
      render={() => (
        <div className="flex items-center gap-3 rounded-lg border bg-card p-4 text-card-foreground">
          <span className="size-2 shrink-0 rounded-full bg-emerald-500" aria-hidden />
          <div>
            <p className="text-sm font-medium">Connected</p>
            <p className="text-sm text-muted-foreground">
              Your mobile device is ready to scan QR codes.
            </p>
          </div>
        </div>
      )}
    />
    <QRescan.MobileLink />
    <QRescan.MobileQR />
  </QRescan.Mobile>
</QRescan.Tab>
```

- qrescan의 기본 public companion 페이지를 사용하므로 앱에 route나 `QRescanMobileCompanion`을 추가하지 않는다.
- `Mobile`, `MobileDescription`, `MobileError`, `MobileLink`, `MobileQR`은 qrescan 기본 구성을 사용한다.
- `MobileConnection.render`는 연결 완료 상태에서만 호출된다. qrescan의 기본 connection card 대신 앱의 shadcn 스타일인 `rounded-lg border bg-card p-4` 카드, 초록 상태 indicator, 기존 typography token으로 렌더링한다.
- qrescan theme CSS variables로 앱의 색상 모드에 맞춘다.

## OTP 등록 화면

- `RegisterOtpFragment`에서 `Tabs`, `TabsList`, `TabsTrigger`, `TabsContent` import와 `tab` state를 제거한다.
- 기존 paste form 전체를 아래처럼 전달한다.

```tsx
<QRScanner
  active={open}
  onScan={finish}
  additionalTabs={[{ value: 'paste', label: 'Paste', children: pasteForm }]}
/>
```

- `paste`, `secret`, advanced options, validation state 및 submit 동작은 그대로 유지한다.

## 검증

```sh
pnpm --filter @keeless/app check
pnpm --filter @keeless/app build
```

- 카메라 권한 거부와 허용
- 화면 공유 시작, 종료
- camera/screen source 전환
- 유효 및 유효하지 않은 TOTP QR
- 이미지 선택, 드롭, 붙여넣기
- QR가 없는 이미지와 잘못된 파일 형식
- public mobile companion QR 연결 및 결과 수신
