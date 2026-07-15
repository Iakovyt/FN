param(
  [string]$OutputPath
)

$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent $PSScriptRoot
$version = (Get-Content (Join-Path $projectRoot "package.json") -Raw | ConvertFrom-Json).version
if ([string]::IsNullOrWhiteSpace($OutputPath)) {
  $OutputPath = Join-Path $projectRoot "FN_${version}_GitHub_Source.zip"
}
$OutputPath = [System.IO.Path]::GetFullPath($OutputPath)

$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("fn-source-" + [Guid]::NewGuid().ToString("N"))
$packageRoot = Join-Path $tempRoot "FN-$version-source"
$excludedPrefixes = @(
  ".git\",
  ".build\",
  ".pnpm-store\",
  ".vite\",
  "dist\",
  "node_modules\",
  "src-tauri\gen\",
  "src-tauri\target\",
  "src-tauri\resources\zapret\bin\",
  "src-tauri\resources\zapret\lists\"
)
$excludedExtensions = @(".exe", ".dll", ".sys", ".pdb", ".lib", ".exp", ".msi", ".zip")

function Get-RelativePath([string]$basePath, [string]$fullPath) {
  $baseUri = [Uri]($basePath.TrimEnd("\") + "\")
  $fullUri = [Uri]$fullPath
  return [Uri]::UnescapeDataString($baseUri.MakeRelativeUri($fullUri).ToString()).Replace("/", "\")
}

try {
  New-Item -ItemType Directory -Force $packageRoot | Out-Null
  $files = Get-ChildItem $projectRoot -File -Recurse -Force | Where-Object {
    $relative = Get-RelativePath $projectRoot $_.FullName
    $excludedByPrefix = $false
    foreach ($prefix in $excludedPrefixes) {
      if ($relative.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        $excludedByPrefix = $true
        break
      }
    }
    !$excludedByPrefix -and $excludedExtensions -notcontains $_.Extension.ToLowerInvariant()
  }

  foreach ($file in $files) {
    $relative = Get-RelativePath $projectRoot $file.FullName
    $destination = Join-Path $packageRoot $relative
    New-Item -ItemType Directory -Force (Split-Path -Parent $destination) | Out-Null
    Copy-Item -LiteralPath $file.FullName -Destination $destination
  }

  if (Test-Path $OutputPath) {
    Remove-Item -LiteralPath $OutputPath -Force
  }
  Compress-Archive -Path $packageRoot -DestinationPath $OutputPath -CompressionLevel Optimal

  $archive = Get-Item $OutputPath
  Write-Host ("Created {0} ({1:N2} MB)" -f $archive.FullName, ($archive.Length / 1MB))
}
finally {
  if (Test-Path $tempRoot) {
    Remove-Item -LiteralPath $tempRoot -Recurse -Force
  }
}
