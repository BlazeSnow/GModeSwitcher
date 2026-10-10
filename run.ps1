# 构建并启动 GModeSwitcher
# 用法:
#   .\run.ps1            # debug 构建并启动（开发用）
#   .\run.ps1 -Release   # release 构建并启动
#   .\run.ps1 -SkipBuild # 跳过构建直接启动
# 说明:
#   - 程序需要管理员权限，启动时经 UAC 提权，请在弹出的对话框中确认
#   - 运行中的实例会锁住 exe 导致链接器拒绝写入，启动前先结束已有进程
param(
    [switch]$Release,
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

$RepoRoot = $PSScriptRoot
$TargetDir = if ($Release) { "release" } else { "debug" }
$ExePath = Join-Path $RepoRoot "target\$TargetDir\GModeSwitcher.exe"

Get-Process GModeSwitcher -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 500

if (-not $SkipBuild) {
    Push-Location $RepoRoot
    if ($Release) {
        cargo build --release
    }
    else {
        cargo build
    }
    $buildExit = $LASTEXITCODE
    Pop-Location

    if ($buildExit -ne 0) {
        Write-Error "构建失败（exit $buildExit）"
        exit 1
    }
}

if (-not (Test-Path $ExePath)) {
    Write-Error "找不到 $ExePath，请先构建"
    exit 1
}

Write-Host "启动 $ExePath（需要管理员权限）" -ForegroundColor Green
Start-Process -FilePath $ExePath -Verb RunAs
