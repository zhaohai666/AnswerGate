#!/usr/bin/env bash
# Android APK 打包 —— 需 Android SDK/NDK 环境
# 用法：scripts/build-android.sh
set -euo pipefail
cd "$(dirname "$0")/.."

# 1) 未初始化 Android 工程则先 init
if [ ! -d src-tauri/gen/android ]; then
  echo "==> npm run tauri android init"
  npm run tauri android init
fi

# 2) 从 config.toml 生成 AndroidManifest <queries> 白名单（Android 11+ 包可见性，U2）
python3 scripts/sync_android_queries.py

# 3) 出 release APK
echo "==> npm run tauri android build -- --release"
npm run tauri android build -- --release

echo "==> 完成。产物：src-tauri/gen/android/app/build/outputs/apk/universal/release/*.apk"
echo "注意：需真机验证 Intent 跳转、未安装包报错、返回键答题页拦截（PRD P0-11）。"
