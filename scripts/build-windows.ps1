# Windows 打包脚本（exe/msi）—— 需在 Windows 上执行
# 用法：powershell -ExecutionPolicy Bypass -File scripts\build-windows.ps1
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

$Target = "x86_64-pc-windows-msvc"
rustup target add $Target

Write-Host "==> tauri build --target $Target"
npm run tauri build -- --target $Target

$Bundle = "src-tauri\target\$Target\release\bundle"
Write-Host "==> 完成。产物："
Get-ChildItem -Recurse $Bundle -Include *.exe,*.msi | ForEach-Object { Write-Host $_.FullName }
Write-Host ""
Write-Host "注意：Windows Defender 可能拦截未签名 exe（README 风险 #3），首次运行需本地信任。"
Write-Host "需真机验证：spawn exe 启动、路径不存在时错误提示。"
