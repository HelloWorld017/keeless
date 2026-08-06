import { Button } from '@/components/button';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/collapsible';
import { Empty, EmptyDescription, EmptyTitle } from '@/components/empty';
import { Field, FieldDescription, FieldLabel } from '@/components/field';
import { FileUpload, FileUploadDropzone } from '@/components/file-upload';
import { Input } from '@/components/input';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/select';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/tabs';
import {
  AdaptiveSheet,
  AdaptiveSheetContent,
  AdaptiveSheetDescription,
  AdaptiveSheetHeader,
  AdaptiveSheetOverlay,
  AdaptiveSheetTitle,
} from '@/fragments/_components/AdaptiveSheet';
import { useLatestRef } from '@/hooks/useLatestRef';
import { IconCamera, IconChevronRight, IconImages, IconMonitor, IconScanLine } from '@/icons';
import { cx } from '@/utils/css';
import jsQR from 'jsqr';
import { useEffect, useRef, useState } from 'react';

type Tab = 'scan' | 'upload' | 'paste';
type Source = 'camera' | 'capture';
type Tracker = { x: number; y: number; width: number; height: number };

const base32 = (value: string) => value.replace(/[\s-]/g, '').toUpperCase().replace(/=+$/, '');

const normalizeUri = (value: string) => {
  try {
    const uri = new URL(value.trim());
    if (uri.protocol !== 'otpauth:' || uri.hostname.toLowerCase() !== 'totp') {
      return undefined;
    }
    const secret = base32(uri.searchParams.get('secret') ?? '');
    if (!/^[A-Z2-7]+$/.test(secret)) {
      return undefined;
    }
    const algorithm = (uri.searchParams.get('algorithm') ?? 'SHA1').toUpperCase();
    const period = Number(uri.searchParams.get('period') ?? '30');
    const digits = Number(uri.searchParams.get('digits') ?? '6');
    if (
      !['SHA1', 'SHA256', 'SHA512'].includes(algorithm) ||
      !Number.isInteger(period) ||
      period <= 0 ||
      ![6, 7, 8].includes(digits)
    ) {
      return undefined;
    }
    uri.searchParams.set('secret', secret);
    uri.searchParams.set('algorithm', algorithm);
    uri.searchParams.set('period', String(period));
    uri.searchParams.set('digits', String(digits));
    return uri.toString();
  } catch {
    return undefined;
  }
};

const fromSecret = ({
  secret,
  algorithm,
  period,
  digits,
}: {
  secret: string;
  algorithm: string;
  period: string;
  digits: string;
}) => {
  const normalized = base32(secret);
  if (!/^[A-Z2-7]+$/.test(normalized)) {
    return undefined;
  }
  const uri = new URL('otpauth://totp/Keeless');
  uri.searchParams.set('secret', normalized);
  uri.searchParams.set('algorithm', algorithm);
  uri.searchParams.set('period', period);
  uri.searchParams.set('digits', digits);
  return normalizeUri(uri.toString());
};

const decodeImage = (source: CanvasImageSource, width: number, height: number) => {
  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext('2d', { willReadFrequently: true });
  if (!context || !width || !height) {
    return undefined;
  }
  context.drawImage(source, 0, 0, width, height);
  return jsQR(context.getImageData(0, 0, width, height).data, width, height, {
    inversionAttempts: 'dontInvert',
  });
};

export const RegisterOtpFragment = ({
  open,
  onOpenChange,
  onConfirm,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onConfirm: (uri: string) => void;
}) => {
  const videoRef = useRef<HTMLVideoElement>(null);
  const animationRef = useRef<number | undefined>(undefined);
  const scanTimeoutRef = useRef<number | undefined>(undefined);
  const streamRef = useRef<MediaStream | undefined>(undefined);
  const [tab, setTab] = useState<Tab>('scan');
  const [stream, setStream] = useState<MediaStream>();
  const [source, setSource] = useState<Source>();
  const [cameras, setCameras] = useState<MediaDeviceInfo[]>([]);
  const [selectedCamera, setSelectedCamera] = useState<string>();
  const [tracker, setTracker] = useState<Tracker>();
  const [scanError, setScanError] = useState<string>();
  const [files, setFiles] = useState<File[]>([]);
  const [paste, setPaste] = useState('');
  const [secret, setSecret] = useState('');
  const [advanced, setAdvanced] = useState(false);
  const [algorithm, setAlgorithm] = useState('SHA1');
  const [period, setPeriod] = useState('30');
  const [digits, setDigits] = useState('6');
  const [pasteError, setPasteError] = useState<string>();

  const stopStream = () => {
    cancelAnimationFrame(animationRef.current ?? 0);
    window.clearTimeout(scanTimeoutRef.current);
    streamRef.current?.getTracks().forEach(track => track.stop());
    streamRef.current = undefined;
    setStream(undefined);
    setSource(undefined);
    setTracker(undefined);
  };

  const finish = (value: string) => {
    const uri = normalizeUri(value);
    if (!uri) {
      setScanError('The QR code does not contain a valid TOTP setup.');
      return false;
    }
    stopStream();
    onConfirm(uri);
    onOpenChange(false);
    return true;
  };
  const finishRef = useLatestRef(finish);

  const attachStream = async (nextStream: MediaStream, nextSource: Source) => {
    stopStream();
    streamRef.current = nextStream;
    setStream(nextStream);
    setSource(nextSource);
    setScanError(undefined);
    const devices = await navigator.mediaDevices.enumerateDevices();
    setCameras(devices.filter(device => device.kind === 'videoinput'));
  };

  const startCamera = async (deviceId?: string) => {
    try {
      const nextStream = await navigator.mediaDevices.getUserMedia({
        audio: false,
        video: deviceId
          ? { deviceId: { exact: deviceId } }
          : { facingMode: { ideal: 'environment' } },
      });
      setSelectedCamera(deviceId ?? nextStream.getVideoTracks()[0]?.getSettings().deviceId);
      await attachStream(nextStream, 'camera');
    } catch {
      setScanError('Camera access could not be started. Check browser and system permissions.');
    }
  };

  const startCapture = async () => {
    try {
      const nextStream = await navigator.mediaDevices.getDisplayMedia({
        audio: false,
        video: true,
      });
      nextStream.getVideoTracks()[0]?.addEventListener('ended', stopStream, { once: true });
      await attachStream(nextStream, 'capture');
    } catch {
      setScanError('Screen capture was cancelled or is unavailable.');
    }
  };

  useEffect(() => {
    if (!stream || !videoRef.current) {
      return undefined;
    }
    const video = videoRef.current;
    video.srcObject = stream;
    void video.play();
    const scan = () => {
      if (video.readyState >= HTMLMediaElement.HAVE_CURRENT_DATA) {
        const result = decodeImage(video, video.videoWidth, video.videoHeight);
        if (result) {
          const points = Object.values(result.location);
          const xs = points.map(point => point.x);
          const ys = points.map(point => point.y);
          setTracker({
            x: (Math.min(...xs) / video.videoWidth) * 100,
            y: (Math.min(...ys) / video.videoHeight) * 100,
            width: ((Math.max(...xs) - Math.min(...xs)) / video.videoWidth) * 100,
            height: ((Math.max(...ys) - Math.min(...ys)) / video.videoHeight) * 100,
          });
          window.clearTimeout(scanTimeoutRef.current);
          scanTimeoutRef.current = window.setTimeout(() => {
            if (!finishRef.current(result.data)) {
              animationRef.current = requestAnimationFrame(scan);
            }
          }, 450);
          return;
        }
      }
      animationRef.current = requestAnimationFrame(scan);
    };
    animationRef.current = requestAnimationFrame(scan);
    return () => cancelAnimationFrame(animationRef.current ?? 0);
  }, [stream, finishRef]);

  useEffect(() => {
    if (open) {
      return undefined;
    }
    stopStream();
    return undefined;
  }, [open]);

  useEffect(() => () => stopStream(), []);

  const upload = async (file: File) => {
    let url: string | undefined;
    try {
      const image = new Image();
      const objectUrl = URL.createObjectURL(file);
      url = objectUrl;
      await new Promise<void>((resolve, reject) => {
        image.onload = () => resolve();
        image.onerror = () => reject(new Error('Invalid image'));
        image.src = objectUrl;
      });
      const result = decodeImage(image, image.naturalWidth, image.naturalHeight);
      if (!result) {
        setScanError('No QR code was found in this image.');
        return;
      }
      finish(result.data);
    } catch {
      setScanError('The image could not be read.');
    } finally {
      if (url) {
        URL.revokeObjectURL(url);
      }
    }
  };

  const submitPaste = () => {
    const uri = paste.trim()
      ? normalizeUri(paste)
      : fromSecret({ secret, algorithm, period, digits });
    if (!uri) {
      setPasteError('Enter a valid TOTP URI or Base32 secret.');
      return;
    }
    onConfirm(uri);
    onOpenChange(false);
  };

  return (
    <AdaptiveSheet open={open} onOpenChange={onOpenChange}>
      <AdaptiveSheetOverlay forceRender />
      <AdaptiveSheetContent className="max-h-[min(44rem,calc(100dvh-1rem))] overflow-y-auto p-4 sm:p-6">
        <AdaptiveSheetHeader className="p-0">
          <AdaptiveSheetTitle>Register OTP</AdaptiveSheetTitle>
          <AdaptiveSheetDescription>
            Add a TOTP setup from a QR code or secret key.
          </AdaptiveSheetDescription>
        </AdaptiveSheetHeader>
        <Tabs>
          <TabsList>
            {(['scan', 'upload', 'paste'] as const).map(value => (
              <TabsTrigger key={value} active={tab === value} onClick={() => setTab(value)}>
                {value[0].toUpperCase() + value.slice(1)}
              </TabsTrigger>
            ))}
          </TabsList>
          {tab === 'scan' && (
            <TabsContent>
              {!stream ? (
                <Empty>
                  <IconScanLine className="size-8" />
                  <EmptyTitle>Scan a QR code</EmptyTitle>
                  <EmptyDescription>
                    Use a camera or share a screen containing the setup QR code.
                  </EmptyDescription>
                  <div className="flex flex-wrap justify-center gap-2">
                    <Button type="button" variant="outline" onClick={() => void startCamera()}>
                      <IconCamera />
                      Camera
                    </Button>
                    <Button type="button" variant="outline" onClick={() => void startCapture()}>
                      <IconMonitor />
                      Capture
                    </Button>
                  </div>
                </Empty>
              ) : (
                <div className="relative overflow-hidden rounded-lg bg-black">
                  <video
                    ref={videoRef}
                    autoPlay
                    muted
                    playsInline
                    className="aspect-video w-full object-contain"
                  />
                  {tracker && (
                    <div
                      className="pointer-events-none absolute border-2 border-primary shadow-[0_0_0_9999px_rgb(0_0_0_/_0.15)]"
                      style={{
                        left: `${tracker.x}%`,
                        top: `${tracker.y}%`,
                        width: `${tracker.width}%`,
                        height: `${tracker.height}%`,
                      }}
                    />
                  )}
                  <Select
                    value={
                      source === 'capture'
                        ? 'capture'
                        : selectedCamera
                          ? `camera:${selectedCamera}`
                          : null
                    }
                    onValueChange={value => {
                      if (value === 'capture') {
                        void startCapture();
                      } else if (value?.startsWith('camera:')) {
                        void startCamera(value.slice('camera:'.length));
                      }
                    }}
                  >
                    <SelectTrigger className="absolute right-3 bottom-3 w-44 bg-background/90 backdrop-blur">
                      <SelectValue placeholder="Choose source" />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="capture">Screen capture</SelectItem>
                      {cameras.map(camera => (
                        <SelectItem key={camera.deviceId} value={`camera:${camera.deviceId}`}>
                          {camera.label || 'Camera'}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
              )}
              {scanError && (
                <p className="mt-3 text-sm text-destructive" role="alert">
                  {scanError}
                </p>
              )}
            </TabsContent>
          )}
          {tab === 'upload' && (
            <TabsContent>
              <FileUpload
                value={files}
                accept="image/*"
                maxFiles={1}
                onValueChange={setFiles}
                onAccept={acceptedFiles => acceptedFiles[0] && void upload(acceptedFiles[0])}
              >
                <FileUploadDropzone>
                  <IconImages className="size-8 text-muted-foreground" />
                  <p className="font-medium text-foreground">Upload a QR image</p>
                  <p className="text-sm text-muted-foreground">
                    Drop, paste, or choose an image file.
                  </p>
                </FileUploadDropzone>
              </FileUpload>
              {scanError && (
                <p className="mt-3 text-sm text-destructive" role="alert">
                  {scanError}
                </p>
              )}
            </TabsContent>
          )}
          {tab === 'paste' && (
            <TabsContent className="space-y-4">
              <Field>
                <FieldLabel htmlFor="otp-uri">TOTP URI</FieldLabel>
                <Input
                  id="otp-uri"
                  value={paste}
                  placeholder="otpauth://totp/..."
                  autoComplete="off"
                  spellCheck={false}
                  onChange={event => {
                    setPaste(event.target.value);
                    setPasteError(undefined);
                  }}
                />
                <FieldDescription>
                  Paste the full provisioning URI, or enter a Base32 secret below.
                </FieldDescription>
              </Field>
              <Field>
                <FieldLabel htmlFor="otp-secret">Base32 secret</FieldLabel>
                <Input
                  id="otp-secret"
                  value={secret}
                  placeholder="JBSWY3DPEHPK3PXP"
                  autoComplete="off"
                  spellCheck={false}
                  onChange={event => {
                    setSecret(event.target.value);
                    setPasteError(undefined);
                  }}
                />
              </Field>
              {!paste.trim() && (
                <Collapsible open={advanced} onOpenChange={setAdvanced}>
                  <CollapsibleTrigger render={<Button type="button" variant="ghost" size="sm" />}>
                    <IconChevronRight
                      className={cx('transition-transform', advanced && 'rotate-90')}
                    />
                    Advanced options
                  </CollapsibleTrigger>
                  <CollapsibleContent className="grid gap-3 pt-3 sm:grid-cols-3">
                    <Field>
                      <FieldLabel>Algorithm</FieldLabel>
                      <Select
                        value={algorithm}
                        onValueChange={value => value && setAlgorithm(value)}
                      >
                        <SelectTrigger>
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="SHA1">SHA-1</SelectItem>
                          <SelectItem value="SHA256">SHA-256</SelectItem>
                          <SelectItem value="SHA512">SHA-512</SelectItem>
                        </SelectContent>
                      </Select>
                    </Field>
                    <Field>
                      <FieldLabel htmlFor="otp-period">Time step</FieldLabel>
                      <Input
                        id="otp-period"
                        type="number"
                        min="1"
                        value={period}
                        onChange={event => setPeriod(event.target.value)}
                      />
                    </Field>
                    <Field>
                      <FieldLabel>Code size</FieldLabel>
                      <Select value={digits} onValueChange={value => value && setDigits(value)}>
                        <SelectTrigger>
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="6">6 digits</SelectItem>
                          <SelectItem value="7">7 digits</SelectItem>
                          <SelectItem value="8">8 digits</SelectItem>
                        </SelectContent>
                      </Select>
                    </Field>
                  </CollapsibleContent>
                </Collapsible>
              )}
              {pasteError && (
                <p className="text-sm text-destructive" role="alert">
                  {pasteError}
                </p>
              )}
              <Button type="button" className="w-full" onClick={submitPaste}>
                Use OTP setup
              </Button>
            </TabsContent>
          )}
        </Tabs>
      </AdaptiveSheetContent>
    </AdaptiveSheet>
  );
};
