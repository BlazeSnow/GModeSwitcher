# 打包发布产物：把 target\release\GModeSwitcher.exe 压成可直接分发的 zip
# 用法:
#   pwsh -File ci/package.ps1 -Version 2026.10.10.0
# 说明:
#   - 产物落在 dist\GModeSwitcher-<版本>-windows-amd64.zip
#   - zip 内为同名目录，含 GModeSwitcher.exe 与 LICENSE
param(
    [Parameter(Mandatory = $true)]
    [string]$Version
)

$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot
$ExePath = Join-Path $RepoRoot "target\release\GModeSwitcher.exe"
$DistDir = Join-Path $RepoRoot "dist"
$PackageName = "GModeSwitcher-$Version-windows-amd64"
$StageDir = Join-Path $DistDir $PackageName
$ZipPath = Join-Path $DistDir "$PackageName.zip"

if (-not (Test-Path $ExePath)) {
    Write-Error "找不到构建产物：$ExePath"
    exit 1
}

if (Test-Path $DistDir) {
    Remove-Item $DistDir -Recurse -Force
}
New-Item -ItemType Directory -Path $StageDir -Force | Out-Null

Copy-Item $ExePath $StageDir

# GPLv3 要求随二进制一并分发许可证全文
$LicensePath = Join-Path $RepoRoot "LICENSE"
if (Test-Path $LicensePath) {
    Copy-Item $LicensePath $StageDir
}

Compress-Archive -Path $StageDir -DestinationPath $ZipPath

Write-Host "已生成：$ZipPath"
