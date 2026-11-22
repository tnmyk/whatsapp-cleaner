$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $RepoRoot

$Version = $env:VERSION
if (-not $Version) {
    $match = Select-String -Path "Cargo.toml" -Pattern '^version = "([^"]+)"' | Select-Object -First 1
    if (-not $match) {
        throw "could not determine version; set VERSION or add a version to Cargo.toml"
    }
    $Version = $match.Matches[0].Groups[1].Value
}
$Version = $Version -replace '^v', ''

$Arch = switch ($env:PROCESSOR_ARCHITECTURE) {
    "AMD64" { "x86_64" }
    "ARM64" { "aarch64" }
    default { ($env:PROCESSOR_ARCHITECTURE).ToLowerInvariant() }
}

$Binary = Join-Path $RepoRoot "target\release\whatsapp-cleaner.exe"
if (-not (Test-Path $Binary)) {
    throw "missing release binary at $Binary. Build it first with: cargo build --release"
}

$DistDir = Join-Path $RepoRoot "dist"
New-Item -ItemType Directory -Force -Path $DistDir | Out-Null

$StageDir = Join-Path ([System.IO.Path]::GetTempPath()) "whatsapp-cleaner-package"
Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $StageDir
New-Item -ItemType Directory -Force -Path $StageDir | Out-Null

try {
    Copy-Item $Binary (Join-Path $StageDir "whatsapp-cleaner.exe")
    Copy-Item (Join-Path $RepoRoot "README.md") (Join-Path $StageDir "README.md")

    $ZipPath = Join-Path $DistDir "whatsapp-cleaner-$Version-windows-$Arch.zip"
    Remove-Item -Force -ErrorAction SilentlyContinue $ZipPath
    Compress-Archive -Path (Join-Path $StageDir "*") -DestinationPath $ZipPath

    Write-Output "Created release asset:"
    Write-Output "  $ZipPath"
}
finally {
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $StageDir
}
