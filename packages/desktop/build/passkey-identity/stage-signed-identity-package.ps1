[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$PackagePath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..\..')).Path
$package = Get-Item -LiteralPath $PackagePath -ErrorAction Stop
Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [System.IO.Compression.ZipFile]::OpenRead($package.FullName)
try {
    if (-not ($archive.Entries.FullName -contains 'AppxSignature.p7x')) {
        throw 'The identity package is unsigned. Submit it to SignPath before staging it.'
    }
}
finally {
    $archive.Dispose()
}

$destinationDirectory = Join-Path $root 'packages\desktop\resources\passkey-windows'
$destination = Join-Path $destinationDirectory 'keeless-passkey-identity.msix'
New-Item -ItemType Directory -Force -Path $destinationDirectory | Out-Null
Copy-Item -LiteralPath $package.FullName -Destination $destination -Force

Write-Output "Signed identity package staged: $destination"
