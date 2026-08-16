[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$Publisher,

    [Parameter(Mandatory)]
    [string]$Version,

    [Parameter(Mandatory)]
    [ValidateSet('x64', 'arm64')]
    [string]$Architecture,

    [string]$OutputPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if ($Version -notmatch '^\d{1,5}\.\d{1,5}\.\d{1,5}\.\d{1,5}$') {
    throw 'Version must use the four-part MSIX format, with each part from 0 through 65535.'
}
if ($Version.Split('.') | Where-Object { [int]$_ -gt 65535 }) {
    throw 'Version must use the four-part MSIX format, with each part from 0 through 65535.'
}

$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..\..')).Path
$template = Join-Path $PSScriptRoot 'AppxManifest.xml.in'
$icon = Join-Path $root 'packages\desktop\src\assets\icons\icon.png'

if (-not (Test-Path -LiteralPath $icon -PathType Leaf)) {
    throw "Missing package icon: $icon"
}

if (-not $OutputPath) {
    $OutputPath = Join-Path $root "packages\desktop\resources\passkey-windows\keeless-passkey-identity-$Architecture-unsigned.msix"
}
$OutputPath = [System.IO.Path]::GetFullPath($OutputPath)

$makeAppx = Get-Command MakeAppx.exe -ErrorAction SilentlyContinue
if (-not $makeAppx) {
    throw 'MakeAppx.exe was not found. Install the Windows SDK before building the identity package.'
}

$outputDirectory = Split-Path -Parent $OutputPath
New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null
$stagingDirectory = Join-Path ([System.IO.Path]::GetTempPath()) "keeless-passkey-identity-$PID"

try {
    New-Item -ItemType Directory -Force -Path (Join-Path $stagingDirectory 'assets') | Out-Null
    Copy-Item -LiteralPath $icon -Destination (Join-Path $stagingDirectory 'assets\passkey.png')

    $manifest = Get-Content -LiteralPath $template -Raw
    $manifest = $manifest.Replace('__PUBLISHER__', [System.Security.SecurityElement]::Escape($Publisher))
    $manifest = $manifest.Replace('__VERSION__', $Version)
    $manifest = $manifest.Replace('__ARCHITECTURE__', $Architecture)
    [System.IO.File]::WriteAllText(
        (Join-Path $stagingDirectory 'AppxManifest.xml'),
        $manifest,
        [System.Text.UTF8Encoding]::new($false)
    )

    & $makeAppx.Path pack /d $stagingDirectory /p $OutputPath /o
    if ($LASTEXITCODE -ne 0) {
        throw "MakeAppx.exe failed with exit code $LASTEXITCODE."
    }
}
finally {
    Remove-Item -LiteralPath $stagingDirectory -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Output "Unsigned identity package created: $OutputPath"
