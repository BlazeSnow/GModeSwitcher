# 把根目录 VERSION 里的版本号同步进 Cargo.toml 的 [package] version
#
# 用法:
#   .\version.ps1
#
# 说明:
#   - VERSION 允许带前导 v（如 v1.0.0-beta.1），写进 Cargo.toml 时会去掉
#   - 只改 [package] 段的 version，不会碰 [dependencies] 里的版本号
#   - Cargo.toml 以 UTF-8 无 BOM、LF 写回，保持 cargo 自己的写法
#   - 改完版本号后 Cargo.lock 需要重新构建才会同步
Set-Location $PSScriptRoot

$ErrorActionPreference = 'Stop'

$versionFile = Join-Path $PSScriptRoot 'VERSION'
if (-not (Test-Path $versionFile)) {
    Write-Error 'VERSION file not found.'
    exit 1
}

try {
    $version = (Get-Content $versionFile -Raw).Trim().TrimStart('v')
}
catch {
    Write-Error 'Unable to read the VERSION file.'
    exit 1
}

if ([string]::IsNullOrWhiteSpace($version)) {
    Write-Error 'Package version is empty.'
    exit 1
}

# Cargo 只接受合法的 semver，先挡掉明显不合法的写法
if ($version -notmatch '^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$') {
    Write-Error "Not a valid semver version: $version"
    exit 1
}

$cargoFile = Join-Path $PSScriptRoot 'Cargo.toml'
if (-not (Test-Path $cargoFile)) {
    Write-Error 'Cargo.toml not found.'
    exit 1
}

$text = [System.IO.File]::ReadAllText($cargoFile)
# 匹配 [package] 段内的 version 行；[^\[] 保证不会越段匹配到 [dependencies]
$regex = [regex]::new('(?m)(\[package\][^\[]*?^version\s*=\s*")([^"]*)(")')
$match = $regex.Match($text)
if (-not $match.Success) {
    Write-Error 'Unable to locate the version field in the [package] section.'
    exit 1
}

$oldVersion = $match.Groups[2].Value
if ($oldVersion -eq $version) {
    Write-Host "Cargo.toml is already at $version, nothing to do."
    exit 0
}

$updated = $regex.Replace($text, ('${1}' + $version + '${3}'), 1)
[System.IO.File]::WriteAllText($cargoFile, $updated, (New-Object System.Text.UTF8Encoding($false)))

Write-Host "Cargo.toml: $oldVersion -> $version"
exit 0
