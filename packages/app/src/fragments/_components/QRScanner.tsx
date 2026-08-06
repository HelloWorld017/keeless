import { Button } from '@/components/button';
import { Empty, EmptyDescription, EmptyTitle } from '@/components/empty';
import { FileUpload, FileUploadDropzone } from '@/components/file-upload';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/select';
import { useLatestRef } from '@/hooks/useLatestRef';
import { IconCamera, IconImages, IconMonitor, IconScanLine } from '@/icons';
import jsQR from 'jsqr';
import { useEffect, useRef, useState } from 'react';

type Source = 'camera' | 'capture';
type Tracker = { x: number; y: number; width: number; height: number };

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

export const QRScanner = ({
  active,
  mode,
  onScan,
}: {
  active: boolean;
  mode: 'scan' | 'upload';
  onScan: (value: string) => string | undefined;
}) => {
  const videoRef = useRef<HTMLVideoElement>(null);
  const animationRef = useRef<number | undefined>(undefined);
  const scanTimeoutRef = useRef<number | undefined>(undefined);
  const streamRef = useRef<MediaStream | undefined>(undefined);
  const [stream, setStream] = useState<MediaStream>();
  const [source, setSource] = useState<Source>();
  const [cameras, setCameras] = useState<MediaDeviceInfo[]>([]);
  const [selectedCamera, setSelectedCamera] = useState<string>();
  const [tracker, setTracker] = useState<Tracker>();
  const [error, setError] = useState<string>();
  const [files, setFiles] = useState<File[]>([]);
  const onScanRef = useLatestRef(onScan);

  const stopStream = () => {
    cancelAnimationFrame(animationRef.current ?? 0);
    window.clearTimeout(scanTimeoutRef.current);
    streamRef.current?.getTracks().forEach(track => track.stop());
    streamRef.current = undefined;
    setStream(undefined);
    setSource(undefined);
    setTracker(undefined);
  };

  const handleScan = (value: string) => {
    const nextError = onScanRef.current(value);
    if (nextError) {
      setError(nextError);
      return false;
    }
    stopStream();
    return true;
  };
  const handleScanRef = useLatestRef(handleScan);

  const attachStream = async (nextStream: MediaStream, nextSource: Source) => {
    stopStream();
    streamRef.current = nextStream;
    setStream(nextStream);
    setSource(nextSource);
    setError(undefined);
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
      setError('Camera access could not be started. Check browser and system permissions.');
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
      setError('Screen capture was cancelled or is unavailable.');
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
            if (!handleScanRef.current(result.data)) {
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
  }, [stream, handleScanRef]);

  useEffect(() => {
    if (!active) {
      stopStream();
    }
  }, [active]);

  useEffect(() => () => stopStream(), []);

  const upload = async (file: File) => {
    let url: string | undefined;
    try {
      setError(undefined);
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
        setError('No QR code was found in this image.');
        return;
      }
      handleScan(result.data);
    } catch {
      setError('The image could not be read.');
    } finally {
      if (url) {
        URL.revokeObjectURL(url);
      }
    }
  };

  if (mode === 'upload') {
    return (
      <>
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
            <p className="text-sm text-muted-foreground">Drop, paste, or choose an image file.</p>
          </FileUploadDropzone>
        </FileUpload>
        {error && (
          <p className="mt-3 text-sm text-destructive" role="alert">
            {error}
          </p>
        )}
      </>
    );
  }

  return (
    <>
      {!stream ? (
        <Empty>
          <IconScanLine className="size-8" />
          <EmptyTitle>Scan a QR code</EmptyTitle>
          <EmptyDescription>
            Use a camera or share a screen containing the QR code.
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
              source === 'capture' ? 'capture' : selectedCamera ? `camera:${selectedCamera}` : null
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
      {error && (
        <p className="mt-3 text-sm text-destructive" role="alert">
          {error}
        </p>
      )}
    </>
  );
};
