$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent $PSScriptRoot
$resourcesRoot = Join-Path $projectRoot "src-tauri\resources"
$zapretDestination = Join-Path $resourcesRoot "zapret"
$tgwsDestination = Join-Path $resourcesRoot "tgws"
$prerequisitesDestination = Join-Path $resourcesRoot "prerequisites"

$zapretVersion = "1.9.9d"
$tgwsVersion = "v1.8.1"
$zapretUrl = "https://sourceforge.net/projects/flowseal.mirror/files/$zapretVersion/zapret-discord-youtube-$zapretVersion.zip/download"
$vcRedistUrl = "https://aka.ms/vc14/vc_redist.x64.exe"

function Get-ReleaseFile([string]$url, [string]$output) {
  Write-Host "Downloading $url"
  & curl.exe --fail --location --retry 4 --retry-delay 2 --connect-timeout 30 --output $output $url
  if ($LASTEXITCODE -ne 0 -or !(Test-Path $output)) {
    throw "Download failed: $url"
  }
}

New-Item -ItemType Directory -Force $zapretDestination, $tgwsDestination, $prerequisitesDestination | Out-Null

$winws = Get-ChildItem $zapretDestination -Recurse -Filter "winws.exe" -File -ErrorAction SilentlyContinue | Select-Object -First 1
if (!$winws) {
  $tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("fn-resources-" + [Guid]::NewGuid().ToString("N"))
  $archive = Join-Path $tempRoot "zapret.zip"
  $extracted = Join-Path $tempRoot "zapret"
  try {
    New-Item -ItemType Directory -Force $tempRoot, $extracted | Out-Null
    Get-ReleaseFile $zapretUrl $archive
    Expand-Archive -LiteralPath $archive -DestinationPath $extracted -Force
    $winws = Get-ChildItem $extracted -Recurse -Filter "winws.exe" -File | Select-Object -First 1
    if (!$winws) {
      throw "The Zapret archive does not contain winws.exe"
    }
    $sourceRoot = if ($winws.Directory.Name -eq "bin") { $winws.Directory.Parent.FullName } else { $winws.Directory.FullName }
    Copy-Item -Path (Join-Path $sourceRoot "*") -Destination $zapretDestination -Recurse -Force
  }
  finally {
    if (Test-Path $tempRoot) {
      Remove-Item -LiteralPath $tempRoot -Recurse -Force
    }
  }
}

$bundledWinws = Get-ChildItem $zapretDestination -Recurse -Filter "winws.exe" -File -ErrorAction SilentlyContinue | Select-Object -First 1
if (!$bundledWinws) {
  throw "Zapret resources are incomplete"
}

$tgwsExe = Join-Path $tgwsDestination "TgWsProxy_headless.exe"
if (!(Test-Path $tgwsExe) -or (Get-Item $tgwsExe).Length -lt 1MB) {
  & (Join-Path $PSScriptRoot "build-tgws-headless.ps1") -OutputPath $tgwsExe
  if (!(Test-Path $tgwsExe) -or (Get-Item $tgwsExe).Length -lt 1MB) {
    throw "Headless TGWS build failed: $tgwsExe"
  }
}

$vcRedist = Join-Path $prerequisitesDestination "VC_redist.x64.exe"
if (!(Test-Path $vcRedist) -or (Get-Item $vcRedist).Length -lt 10MB) {
  Get-ReleaseFile $vcRedistUrl $vcRedist
}
$signature = Get-AuthenticodeSignature -LiteralPath $vcRedist
if ($signature.Status -ne "Valid" -or $signature.SignerCertificate.Subject -notmatch "Microsoft Corporation") {
  throw "Visual C++ Redistributable has an invalid Microsoft signature"
}

Write-Host "Bundled Zapret $zapretVersion, TGWS $tgwsVersion and Microsoft VC++ Runtime"
