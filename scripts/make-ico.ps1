# 统一从 SVG 母版生成前端与 Windows 图标（含 16/24/32/48/64/128/256 ICO）。
# 使用已有 Node、Playwright 和系统 Chrome；不改 ICNS / 移动端资源。
# 用法：pwsh -File scripts/make-ico.ps1
$ErrorActionPreference = 'Stop'
$generator = Join-Path $PSScriptRoot 'generate-icons.mjs'
& node $generator
if ($LASTEXITCODE -ne 0) {
  throw "图标生成失败（node 退出码 $LASTEXITCODE）"
}
