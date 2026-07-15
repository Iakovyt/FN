param(
  [string]$OutputPath
)

$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($OutputPath)) {
  $OutputPath = Join-Path $projectRoot "src-tauri\resources\tgws\TgWsProxy_headless.exe"
}
$OutputPath = [System.IO.Path]::GetFullPath($OutputPath)

$python = Get-Command "py.exe" -ErrorAction SilentlyContinue
$pythonArgs = @()
if ($python) {
  $pythonArgs = @("-3.12")
}
else {
  $python = Get-Command "python.exe" -ErrorAction SilentlyContinue
}
if (!$python) {
  throw "Python 3.12 is required to build the headless TGWS module"
}

$workRoot = Join-Path $projectRoot ".build\tgws-headless"
$venvRoot = Join-Path $workRoot "venv"
$venvPython = Join-Path $venvRoot "Scripts\python.exe"
$archive = Join-Path $workRoot "source.zip"
$sourceRoot = Join-Path $workRoot "source"
$vendoredSource = Join-Path $projectRoot "src-tauri\resources\tgws\source"
$distRoot = Join-Path $workRoot "dist"
$buildRoot = Join-Path $workRoot "pyinstaller-build"
$specRoot = Join-Path $workRoot "pyinstaller-spec"
$sourceUrl = "https://sourceforge.net/projects/tg-ws-proxy.mirror/files/v1.8.1/source.zip/download"

New-Item -ItemType Directory -Force $workRoot | Out-Null
if (!(Test-Path $venvPython)) {
  & $python.Source @pythonArgs -m venv $venvRoot
  if ($LASTEXITCODE -ne 0) {
    throw "Failed to create the Python build environment"
  }
}

& $venvPython -m pip install --disable-pip-version-check "pyinstaller==6.16.0" "cryptography==46.0.5"
if ($LASTEXITCODE -ne 0) {
  throw "Failed to install TGWS build dependencies"
}

if (Test-Path (Join-Path $vendoredSource "proxy\tg_ws_proxy.py")) {
  $tgwsSource = $vendoredSource
}
else {
  if (!(Test-Path $archive)) {
    & curl.exe --fail --location --retry 4 --retry-delay 2 --connect-timeout 30 --output $archive $sourceUrl
    if ($LASTEXITCODE -ne 0 -or !(Test-Path $archive)) {
      throw "Failed to download TGWS v1.8.1 source"
    }
  }

  if (!(Test-Path $sourceRoot)) {
    New-Item -ItemType Directory -Force $sourceRoot | Out-Null
    Expand-Archive -LiteralPath $archive -DestinationPath $sourceRoot -Force
  }

  $entryPoint = Get-ChildItem $sourceRoot -Recurse -Filter "tg_ws_proxy.py" -File |
    Where-Object { $_.Directory.Name -eq "proxy" } |
    Select-Object -First 1
  if (!$entryPoint) {
    throw "TGWS source archive does not contain proxy\tg_ws_proxy.py"
  }
  $tgwsSource = $entryPoint.Directory.Parent.FullName
}
$wrapper = Join-Path $PSScriptRoot "tgws_headless.py"

New-Item -ItemType Directory -Force $distRoot, $buildRoot, $specRoot | Out-Null
& $venvPython -m PyInstaller `
  --noconfirm `
  --clean `
  --onefile `
  --console `
  --name "TgWsProxy_headless" `
  --paths $tgwsSource `
  --distpath $distRoot `
  --workpath $buildRoot `
  --specpath $specRoot `
  $wrapper
if ($LASTEXITCODE -ne 0) {
  throw "PyInstaller failed to build TGWS"
}

$builtExe = Join-Path $distRoot "TgWsProxy_headless.exe"
if (!(Test-Path $builtExe) -or (Get-Item $builtExe).Length -lt 1MB) {
  throw "Built TGWS executable is missing or incomplete"
}

New-Item -ItemType Directory -Force (Split-Path -Parent $OutputPath) | Out-Null
Copy-Item -LiteralPath $builtExe -Destination $OutputPath -Force
Write-Host "Built headless TGWS: $OutputPath"
