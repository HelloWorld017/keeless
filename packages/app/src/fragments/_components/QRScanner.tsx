import { Button, buttonVariants } from '@/components/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/dropdown-menu';
import { Empty, EmptyDescription, EmptyTitle } from '@/components/empty';
import { FileUpload, FileUploadDropzone } from '@/components/file-upload';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/tabs';
import { IconCamera, IconImages, IconMonitor, IconScanLine } from '@/icons';
import { QRescan, wechatDecoder } from 'qrescan';
import { type ReactNode, useState } from 'react';

type AdditionalTab = {
  value: string;
  label: ReactNode;
  children: ReactNode;
};

const outlineButtonClass = buttonVariants({ variant: 'outline' });

export const QRScanner = ({
  active,
  onScan,
  additionalTabs = [],
}: {
  active: boolean;
  onScan: (value: string) => string | undefined;
  additionalTabs?: AdditionalTab[];
}) => {
  const [error, setError] = useState<string>();
  const [files, setFiles] = useState<File[]>([]);
  const tabCount = 3 + additionalTabs.length;

  if (!active) {
    return null;
  }

  return (
    <QRescan
      decoderClient={wechatDecoder}
      options={{ mobile: { enabled: true } }}
      onScan={value => setError(onScan(value))}
    >
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
          <QRescan.TabTrigger
            value="upload"
            render={({ children, disabled, isActive, onSelect }) => (
              <TabsTrigger active={isActive} disabled={disabled} onClick={onSelect}>
                {children}
              </TabsTrigger>
            )}
          />
          <QRescan.TabTrigger
            value="mobile"
            render={({ children, disabled, isActive, onSelect }) => (
              <TabsTrigger active={isActive} disabled={disabled} onClick={onSelect}>
                {children}
              </TabsTrigger>
            )}
          />
          {additionalTabs.map(tab => (
            <QRescan.TabTrigger
              key={tab.value}
              value={tab.value}
              render={({ children, disabled, isActive, onSelect }) => (
                <TabsTrigger active={isActive} disabled={disabled} onClick={onSelect}>
                  {children}
                </TabsTrigger>
              )}
            >
              {tab.label}
            </QRescan.TabTrigger>
          ))}
        </QRescan.TabList>

        <QRescan.Tab
          value="scan"
          render={({ children, isActive }) =>
            isActive ? <TabsContent>{children}</TabsContent> : null
          }
        >
          <QRescan.Scan>
            <QRescan.ViewFinder sourceType="stream">
              <QRescan.ViewFinderHighlight />
              <QRescan.CameraSelect
                render={({ items, selectedItem, onSelectItem }) => (
                  <div className="absolute right-3 bottom-3">
                    <DropdownMenu>
                      <DropdownMenuTrigger
                        render={<Button type="button" variant="outline" size="sm" />}
                      >
                        {selectedItem?.kind === 'screen'
                          ? 'Screen capture'
                          : (selectedItem?.label ?? 'Camera')}
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="end">
                        {items.map(item => (
                          <DropdownMenuItem
                            key={item.value}
                            onClick={() => onSelectItem(item.value)}
                          >
                            {item.kind === 'screen' ? 'Screen capture' : item.label}
                          </DropdownMenuItem>
                        ))}
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </div>
                )}
              />
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
                <QRescan.ScanInitializeError className="text-sm text-destructive" />
              </Empty>
            </QRescan.ScanInitialize>
          </QRescan.Scan>
        </QRescan.Tab>

        <QRescan.Tab
          value="upload"
          render={({ children, isActive }) =>
            isActive ? <TabsContent>{children}</TabsContent> : null
          }
        >
          <QRescan.Upload>
            <QRescan.Dropzone
              render={({ error: uploadError, processFiles }) => (
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
                  {uploadError ? (
                    <p className="mt-3 text-sm text-destructive" role="alert">
                      {uploadError}
                    </p>
                  ) : null}
                </>
              )}
            />
          </QRescan.Upload>
        </QRescan.Tab>

        <QRescan.Tab
          value="mobile"
          render={({ children, isActive }) =>
            isActive ? <TabsContent>{children}</TabsContent> : null
          }
        >
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

        {additionalTabs.map(tab => (
          <QRescan.Tab
            key={tab.value}
            value={tab.value}
            render={({ children, isActive }) =>
              isActive ? <TabsContent className="space-y-4">{children}</TabsContent> : null
            }
          >
            {tab.children}
          </QRescan.Tab>
        ))}
      </QRescan.Tabs>
      {error ? (
        <p className="mt-3 text-sm text-destructive" role="alert">
          {error}
        </p>
      ) : null}
    </QRescan>
  );
};
