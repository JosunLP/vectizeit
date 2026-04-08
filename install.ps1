[CmdletBinding()]
param(
    [switch]$Update,
    [switch]$Uninstall,
    [string]$Version = "latest",
    [string]$InstallDir = $(if ($env:VECTIZEIT_INSTALL_DIR) { $env:VECTIZEIT_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "Programs\VectizeIt\bin" })
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$Owner = "JosunLP"
$Repo = "vectizeit"
$ProjectName = "vectizeit"
$PrimaryBinary = "trace.exe"
$AliasBinary = "vectizeit.exe"
$ChecksumsName = "checksums.txt"

function Get-ReleaseUrl {
    param([Parameter(Mandatory = $true)][string]$AssetName)

    if ($Version -eq "latest") {
        return "https://github.com/$Owner/$Repo/releases/latest/download/$AssetName"
    }

    $Tag = if ($Version.StartsWith("v")) { $Version } else { "v$Version" }
    return "https://github.com/$Owner/$Repo/releases/download/$Tag/$AssetName"
}

function Get-PlatformAssetInfo {
    $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
    switch ($arch) {
        "X64" {
            return @{
                Target = "x86_64-pc-windows-msvc"
                AssetName = "$ProjectName-x86_64-pc-windows-msvc.zip"
            }
        }
        default {
            throw "No prebuilt Windows release is available for architecture '$arch'."
        }
    }
}

function Ensure-UserPathContains {
    param([Parameter(Mandatory = $true)][string]$PathEntry)

    $currentUserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $entries = @()
    if ($currentUserPath) {
        $entries = $currentUserPath.Split(";", [System.StringSplitOptions]::RemoveEmptyEntries)
    }

    if ($entries -contains $PathEntry) {
        return
    }

    $newEntries = @($entries + $PathEntry)
    [Environment]::SetEnvironmentVariable("Path", ($newEntries -join ";"), "User")
}

function Remove-UserPathEntry {
    param([Parameter(Mandatory = $true)][string]$PathEntry)

    $currentUserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (-not $currentUserPath) {
        return
    }

    $newEntries = $currentUserPath.Split(";", [System.StringSplitOptions]::RemoveEmptyEntries) |
        Where-Object { $_ -ne $PathEntry }

    [Environment]::SetEnvironmentVariable("Path", ($newEntries -join ";"), "User")
}

function Remove-InstalledFiles {
    $primaryPath = Join-Path $InstallDir $PrimaryBinary
    $aliasPath = Join-Path $InstallDir $AliasBinary

    Remove-Item $primaryPath -Force -ErrorAction SilentlyContinue
    Remove-Item $aliasPath -Force -ErrorAction SilentlyContinue

    if ((Test-Path $InstallDir) -and -not (Get-ChildItem $InstallDir -Force -ErrorAction SilentlyContinue)) {
        Remove-Item $InstallDir -Force -ErrorAction SilentlyContinue
        Remove-UserPathEntry -PathEntry $InstallDir
    }
}

if ($Uninstall) {
    Remove-InstalledFiles
    Write-Host "[vectizeit] Removed $PrimaryBinary and $AliasBinary from $InstallDir."
    exit 0
}

$asset = Get-PlatformAssetInfo
$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString("N"))
$archivePath = Join-Path $tempRoot $asset.AssetName
$checksumsPath = Join-Path $tempRoot $ChecksumsName
$extractDir = Join-Path $tempRoot "extract"

New-Item -ItemType Directory -Path $extractDir -Force | Out-Null
New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null

try {
    $action = if ($Update) { "Updating" } else { "Installing" }
    Write-Host "[vectizeit] $action $PrimaryBinary for $($asset.Target)..."

    Invoke-WebRequest -Uri (Get-ReleaseUrl -AssetName $asset.AssetName) -OutFile $archivePath
    Invoke-WebRequest -Uri (Get-ReleaseUrl -AssetName $ChecksumsName) -OutFile $checksumsPath

    $expected = Select-String -Path $checksumsPath -Pattern ([regex]::Escape($asset.AssetName)) |
        Select-Object -First 1 |
        ForEach-Object { ($_ -split '\s+')[0] }

    if (-not $expected) {
        throw "Could not find a checksum for $($asset.AssetName)."
    }

    $actual = (Get-FileHash -Path $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $expected.ToLowerInvariant()) {
        throw "Checksum verification failed for $($asset.AssetName). Expected $expected but received $actual."
    }

    Expand-Archive -Path $archivePath -DestinationPath $extractDir -Force
    $binary = Get-ChildItem -Path $extractDir -Recurse -Filter $PrimaryBinary | Select-Object -First 1
    if (-not $binary) {
        throw "The downloaded archive did not contain $PrimaryBinary."
    }

    Copy-Item $binary.FullName (Join-Path $InstallDir $PrimaryBinary) -Force
    Copy-Item $binary.FullName (Join-Path $InstallDir $AliasBinary) -Force
    Ensure-UserPathContains -PathEntry $InstallDir

    Write-Host "[vectizeit] Installed $PrimaryBinary and $AliasBinary in $InstallDir."
    Write-Host "[vectizeit] Restart your shell if the commands are not immediately available on PATH."
}
finally {
    Remove-Item $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}
