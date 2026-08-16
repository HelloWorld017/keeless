[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateSet('Install', 'Uninstall')]
    [string]$Action,

    [Parameter(Mandatory)]
    [string]$PackageName,

    [string]$PackagePath,

    [string]$ExternalLocation
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if ($Action -eq 'Install') {
    if (-not $PackagePath -or -not $ExternalLocation) {
        throw 'PackagePath and ExternalLocation are required when installing package identity.'
    }
    if (-not (Test-Path -LiteralPath $PackagePath -PathType Leaf)) {
        throw "Missing identity package: $PackagePath"
    }
    if (-not (Test-Path -LiteralPath $ExternalLocation -PathType Container)) {
        throw "Missing external package location: $ExternalLocation"
    }

    # AppX deployment verifies the package signature and trust chain.
    Add-AppxPackage -Stage -Path $PackagePath -ExternalLocation $ExternalLocation -ForceUpdateFromAnyVersion
    Add-AppxProvisionedPackage -Online -PackagePath $PackagePath | Out-Null
    exit 0
}

$provisionedPackages = @(
    Get-AppxProvisionedPackage -Online |
        Where-Object DisplayName -eq $PackageName
)
foreach ($package in $provisionedPackages) {
    Remove-AppxProvisionedPackage -Online -PackageName $package.PackageName | Out-Null
}

$packages = @(Get-AppxPackage -AllUsers -Name $PackageName)
foreach ($package in $packages) {
    Remove-AppxPackage -Package $package.PackageFullName -AllUsers
}
