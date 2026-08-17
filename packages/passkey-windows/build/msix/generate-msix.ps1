[CmdletBinding()]
param(
    [switch]$Development
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..\..')).Path
$profile = if ($Development) { 'debug' } else { 'release' }
$publisher = if ($Development) { 'CN=Keeless Development' } else { $env:MSIX_PUBLISHER }
$version = if ($Development) {
    if ($env:MSIX_VERSION) { $env:MSIX_VERSION } else { '0.0.0.0' }
} else {
    $env:MSIX_VERSION
}

if (-not $publisher) {
    throw 'MSIX_PUBLISHER must match the SignPath certificate subject.'
}
if (-not $version) {
    throw 'MSIX_VERSION must advance for each release package.'
}
if ($version -notmatch '^\d{1,5}\.\d{1,5}\.\d{1,5}\.\d{1,5}$' -or ($version.Split('.') | Where-Object { [int]$_ -gt 65535 })) {
    throw 'MSIX_VERSION must use four parts from 0 through 65535.'
}

function Get-WindowsSdkTool {
    param([string]$Name)

    $cmd = Get-Command $Name -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Path }

    $sdkBase = "${env:ProgramFiles(x86)}\Windows Kits\10\bin"
    if (Test-Path -LiteralPath $sdkBase) {
        $found = Get-ChildItem -Path $sdkBase -Filter $Name -Recurse -File -ErrorAction SilentlyContinue |
            Where-Object { $_.FullName -match '\\x64\\' } |
            Sort-Object -Property LastWriteTime -Descending |
            Select-Object -First 1

        if ($found) { return $found.FullName }
    }
    return $null
}

$makeAppx = Get-WindowsSdkTool 'MakeAppx.exe'
if (-not $makeAppx) {
    throw 'MakeAppx.exe was not found. Install the Windows SDK before generating the MSIX.'
}

$template = Join-Path $PSScriptRoot 'AppxManifest.xml'
$icon = Join-Path $root 'packages\desktop\src\assets\icons\icon.png'
$output = Join-Path $root "target\$profile\keeless-passkey-windows.msix"
if (-not (Test-Path -LiteralPath $icon -PathType Leaf)) {
    throw "Missing MSIX icon: $icon"
}
if (-not (Test-Path -LiteralPath (Join-Path $root "target\$profile\keeless-passkey-windows.exe") -PathType Leaf)) {
    throw "Build keeless-passkey-windows for the $profile profile before generating the MSIX."
}

$stagingDirectory = Join-Path ([System.IO.Path]::GetTempPath()) "keeless-msix-$PID"
try {
    New-Item -ItemType Directory -Force -Path (Join-Path $stagingDirectory 'assets') | Out-Null
    Copy-Item -LiteralPath $icon -Destination (Join-Path $stagingDirectory 'assets\passkey.png')

    $manifest = Get-Content -LiteralPath $template -Raw
    $manifest = $manifest.Replace('__PUBLISHER__', [System.Security.SecurityElement]::Escape($publisher))
    $manifest = $manifest.Replace('__VERSION__', $version)
    [System.IO.File]::WriteAllText(
        (Join-Path $stagingDirectory 'AppxManifest.xml'),
        $manifest,
        [System.Text.UTF8Encoding]::new($false)
    )

    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $output) | Out-Null
    & $makeAppx pack /d $stagingDirectory /p $output /o /nv
    if ($LASTEXITCODE -ne 0) {
        throw "MakeAppx.exe failed with exit code $LASTEXITCODE."
    }

    if ($Development) {
        $certificate = Get-ChildItem Cert:\CurrentUser\My |
            Where-Object { $_.Subject -eq $publisher -and $_.HasPrivateKey } |
            Select-Object -First 1
        if (-not $certificate) {
            $certificate = New-SelfSignedCertificate `
                -Type Custom `
                -Subject $publisher `
                -CertStoreLocation Cert:\CurrentUser\My `
                -KeyAlgorithm RSA `
                -KeyLength 2048 `
                -KeyUsage DigitalSignature `
                -HashAlgorithm SHA256 `
                -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3') `
                -FriendlyName 'Keeless Passkey Development'
        }
        if (-not (Get-ChildItem Cert:\CurrentUser\TrustedPeople | Where-Object Thumbprint -eq $certificate.Thumbprint)) {
            $certificateFile = Join-Path $stagingDirectory 'keeless-development.cer'
            Export-Certificate -Cert $certificate -FilePath $certificateFile | Out-Null
            Import-Certificate -FilePath $certificateFile -CertStoreLocation Cert:\CurrentUser\TrustedPeople | Out-Null
        }

        $signTool = Get-WindowsSdkTool 'SignTool.exe'
        if (-not $signTool) {
            throw 'SignTool.exe was not found. Install the Windows SDK before self-signing the MSIX.'
        }
        & $signTool sign /fd SHA256 /sha $certificate.Thumbprint $output
        if ($LASTEXITCODE -ne 0) {
            throw "SignTool.exe failed with exit code $LASTEXITCODE."
        }
    }
}
finally {
    Remove-Item -LiteralPath $stagingDirectory -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Output "MSIX generated: $output"
