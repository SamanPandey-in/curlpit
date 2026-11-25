<#
.SYNOPSIS
    Install api-tester on Windows from a GitHub release.

.DESCRIPTION
    Downloads the Windows x64 release asset and its .sha256 file into a temporary
    directory, verifies the SHA-256 checksum (aborts on mismatch), and copies
    api-tester.exe to a per-user directory. Needs no administrator rights and does
    not change PATH; it prints the command to do that instead.

    irm https://raw.githubusercontent.com/SamanPandey-in/curlpit/main/scripts/install.ps1 | iex
    & ([scriptblock]::Create((irm .../install.ps1))) -Version 0.1.0

.PARAMETER Version
    Install this release (tag vX.Y.Z) instead of the latest. "v0.1.0" and "0.1.0" both work.

.PARAMETER InstallDir
    Where to put api-tester.exe. Default: $env:LOCALAPPDATA\Programs\api-tester\bin,
    or $env:API_TESTER_INSTALL_DIR when set.

.PARAMETER Repo
    GitHub repository to download from (OWNER/NAME).
#>
[CmdletBinding()]
param(
    [string]$Version = '',
    [string]$InstallDir = '',
    [string]$Repo = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
# Invoke-WebRequest is many times slower with the progress bar on Windows PowerShell 5.1.
$ProgressPreference = 'SilentlyContinue'
# Windows PowerShell 5.1 may default to TLS 1.0; GitHub requires 1.2+.
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$Bin = 'api-tester'
if (-not $Repo) { $Repo = if ($env:API_TESTER_REPO) { $env:API_TESTER_REPO } else { 'SamanPandey-in/curlpit' } }
if (-not $InstallDir) {
    $InstallDir = if ($env:API_TESTER_INSTALL_DIR) { $env:API_TESTER_INSTALL_DIR }
                  else { Join-Path $env:LOCALAPPDATA "Programs\$Bin\bin" }
}

# --- Which asset? -----------------------------------------------------------
# Only x64 is built. Windows on ARM runs x64 binaries under emulation, so allow it.
$arch = $env:PROCESSOR_ARCHITECTURE
if ($env:PROCESSOR_ARCHITEW6432) { $arch = $env:PROCESSOR_ARCHITEW6432 }
switch ($arch) {
    'AMD64' { }
    'ARM64' { Write-Warning 'No native ARM64 build yet; installing the x64 build, which runs under emulation.' }
    default { throw "Unsupported CPU architecture: $arch (only x64 is built)." }
}
$target = 'x86_64-pc-windows-msvc'
$asset = "$Bin-$target.zip"

if ($Version) {
    $tag = 'v' + $Version.TrimStart('v')
    $base = "https://github.com/$Repo/releases/download/$tag"
} else {
    $tag = 'latest'
    $base = "https://github.com/$Repo/releases/latest/download"
}

# --- Download and verify ----------------------------------------------------
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("$Bin-install-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    $zip = Join-Path $tmp $asset
    $sumFile = "$zip.sha256"

    Write-Host "Downloading $asset ($tag) from $Repo..."
    Invoke-WebRequest -UseBasicParsing -Uri "$base/$asset" -OutFile $zip
    Invoke-WebRequest -UseBasicParsing -Uri "$base/$asset.sha256" -OutFile $sumFile

    $expected = ((Get-Content -Raw -Path $sumFile).Trim() -split '\s+')[0].ToLowerInvariant()
    if ($expected -notmatch '^[0-9a-f]{64}$') { throw "Malformed checksum file for $asset." }
    $actual = (Get-FileHash -Algorithm SHA256 -Path $zip).Hash.ToLowerInvariant()
    if ($expected -ne $actual) {
        throw "Checksum mismatch for ${asset}: expected $expected, got $actual."
    }
    Write-Host "Checksum OK (sha256 $actual)."

    # --- Install ------------------------------------------------------------
    $extract = Join-Path $tmp 'x'
    Expand-Archive -Path $zip -DestinationPath $extract
    $src = Join-Path $extract "$Bin-$target\$Bin.exe"
    if (-not (Test-Path -LiteralPath $src -PathType Leaf)) {
        throw "Archive did not contain $Bin-$target\$Bin.exe."
    }

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    $dest = Join-Path $InstallDir "$Bin.exe"
    Copy-Item -LiteralPath $src -Destination $dest -Force
    Write-Host "Installed $Bin to $dest"
}
finally {
    Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
}

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$onPath = @(($userPath, $env:Path) -join ';' -split ';' | Where-Object { $_ -and ($_.TrimEnd('\') -ieq $InstallDir.TrimEnd('\')) }).Count -gt 0
if ($onPath) {
    Write-Host "Run it with: $Bin"
} else {
    Write-Host ''
    Write-Host "$InstallDir is not on your PATH. To add it for your user account, run:"
    Write-Host "  [Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path', 'User') + ';$InstallDir', 'User')"
    Write-Host 'then open a new terminal.'
}
