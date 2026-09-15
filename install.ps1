<#
.SYNOPSIS
  Installs alnair-router and enables auto-start.

.DESCRIPTION
  From a checkout the script prefers a local build (target\release, then
  target\debug). Run remotely and it downloads the latest GitHub Release for
  Windows x64, verifies SHA256SUMS.txt, copies the binary, and asks the binary
  to create its config and register auto-start so the router comes back after a
  restart.

  Remote install:
    irm https://raw.githubusercontent.com/xFlawlessDev/alnair-router/main/install.ps1 | iex

.PARAMETER Binary
  Explicit path to an alnair-router.exe; skips both the local lookup and the download.

.PARAMETER Version
  Release tag to download (default: the latest release).

.PARAMETER Repo
  GitHub repo slug (default: xFlawlessDev/alnair-router).

.PARAMETER InstallDir
  Where the binary is placed (default: %LOCALAPPDATA%\alnair-router\bin).

.PARAMETER NoAutoStart
  Install the binary without registering auto-start.

.PARAMETER NoPathUpdate
  Do not add the install directory to your user PATH.
#>
[CmdletBinding()]
param(
  [string]$Binary = '',
  [string]$Version = $(if ($env:ALNAIR_ROUTER_VERSION) { $env:ALNAIR_ROUTER_VERSION } else { '' }),
  [string]$Repo = $(if ($env:ALNAIR_ROUTER_REPO) { $env:ALNAIR_ROUTER_REPO } else { 'xFlawlessDev/alnair-router' }),
  [string]$InstallDir = $(if ($env:ALNAIR_ROUTER_INSTALL_DIR) { $env:ALNAIR_ROUTER_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'alnair-router\bin' }),
  [switch]$NoAutoStart,
  [switch]$NoPathUpdate
)

$ErrorActionPreference = 'Stop'

$Target = 'x86_64-pc-windows-msvc'
$ArchiveBinary = 'alnair-router.exe'

function Find-LocalBinary {
  if (-not $PSScriptRoot) { return '' }

  foreach ($candidate in @(
      (Join-Path $PSScriptRoot 'target\release\alnair-router.exe'),
      (Join-Path $PSScriptRoot 'target\debug\alnair-router.exe')
    )) {
    if (Test-Path -LiteralPath $candidate) { return (Resolve-Path -LiteralPath $candidate).Path }
  }

  return ''
}

function Get-LatestTag {
  try {
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" `
      -Headers @{ 'User-Agent' = 'alnair-router-install'; 'Accept' = 'application/vnd.github+json' }
    if ($release.tag_name) { return $release.tag_name }
  }
  catch {
    Write-Verbose "GitHub API lookup failed: $($_.Exception.Message)"
  }

  $response = Invoke-WebRequest -Uri "https://github.com/$Repo/releases/latest" `
    -Method Head -MaximumRedirection 0 -SkipHttpErrorCheck
  $location = [string]$response.Headers.Location
  if ($location -match '/tag/(.+)$') { return $Matches[1] }

  throw "could not resolve the latest release for $Repo (set -Version or ALNAIR_ROUTER_VERSION to pin one)"
}

function Assert-Checksum {
  param([string]$ArchivePath, [string]$ArchiveName, [string]$SumsUrl)

  $sumsPath = Join-Path (Split-Path -Parent $ArchivePath) 'SHA256SUMS.txt'
  try {
    Invoke-WebRequest -Uri $SumsUrl -OutFile $sumsPath
  }
  catch {
    Write-Warning 'SHA256SUMS.txt is unavailable; skipping checksum verification'
    return
  }

  $line = Get-Content -LiteralPath $sumsPath |
    Where-Object { $_ -match [regex]::Escape($ArchiveName) } |
    Select-Object -First 1
  if (-not $line) {
    Write-Warning "no checksum entry for $ArchiveName; skipping verification"
    return
  }

  $expected = ($line -split '\s+')[0].ToLowerInvariant()
  $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $ArchivePath).Hash.ToLowerInvariant()
  if ($expected -ne $actual) {
    throw "checksum mismatch for $ArchiveName — refusing to install"
  }
  Write-Host 'Checksum verified'
}

$source = ''
$TempDir = $null

try {
  if ($Binary) {
    if (-not (Test-Path -LiteralPath $Binary)) { throw "Binary not found: $Binary" }
    $source = (Resolve-Path -LiteralPath $Binary).Path
  }
  else {
    $source = Find-LocalBinary
  }

  if (-not $source) {
    if (-not $Version) { $Version = Get-LatestTag }
    $archiveName = "alnair-router-$Version-$Target.zip"
    $TempDir = Join-Path ([IO.Path]::GetTempPath()) ("alnair-router-" + [Guid]::NewGuid().ToString('N').Substring(0, 8))
    New-Item -ItemType Directory -Force -Path $TempDir | Out-Null

    Write-Host "Downloading $archiveName"
    $archivePath = Join-Path $TempDir $archiveName
    Invoke-WebRequest -Uri "https://github.com/$Repo/releases/download/$Version/$archiveName" -OutFile $archivePath
    Assert-Checksum -ArchivePath $archivePath -ArchiveName $archiveName -SumsUrl "https://github.com/$Repo/releases/download/$Version/SHA256SUMS.txt"

    Expand-Archive -LiteralPath $archivePath -DestinationPath $TempDir -Force
    $source = Join-Path $TempDir $ArchiveBinary
    if (-not (Test-Path -LiteralPath $source)) { throw "archive did not contain $ArchiveBinary" }
  }

  New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
  Get-Process -Name 'alnair-router' -ErrorAction SilentlyContinue | Stop-Process -Force

  $installed = Join-Path $InstallDir 'alnair-router.exe'
  Copy-Item -LiteralPath $source -Destination $installed -Force

  Write-Host "Installed alnair-router to $installed"

  if ($NoAutoStart) {
    Write-Host 'auto-start registration skipped (-NoAutoStart)'
  }
  else {
    & $installed install
  }

  if (-not $NoPathUpdate) {
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (($userPath -split ';') -notcontains $InstallDir) {
      $updated = (($userPath.TrimEnd(';') + ';' + $InstallDir).TrimStart(';'))
      [Environment]::SetEnvironmentVariable('Path', $updated, 'User')
      Write-Host "Added $InstallDir to your user PATH (new terminals pick it up)."
    }
  }

  Write-Host "Uninstall later with: $installed uninstall"
}
finally {
  if ($TempDir -and (Test-Path -LiteralPath $TempDir)) {
    Remove-Item -Recurse -Force -LiteralPath $TempDir -ErrorAction SilentlyContinue
  }
}
