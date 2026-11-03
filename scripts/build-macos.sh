#!/usr/bin/env bash
# macOS 打包：aarch64 + x86_64 双 target 分别出 .app/.dmg；--universal 额外合并 lipo 通用包。
# 用法：scripts/build-macos.sh [--universal]
set -euo pipefail
cd "$(dirname "$0")/.."

TARGET_A=aarch64-apple-darwin
TARGET_B=x86_64-apple-darwin
BIN=answergate

rustup target add "$TARGET_A" "$TARGET_B"

echo "==> 打包 $TARGET_A"
npm run tauri build -- --target "$TARGET_A"
echo "==> 打包 $TARGET_B"
npm run tauri build -- --target "$TARGET_B"

APP_A="src-tauri/target/$TARGET_A/release/bundle/macos/AnswerGate.app"
APP_B="src-tauri/target/$TARGET_B/release/bundle/macos/AnswerGate.app"

if [[ "${1:-}" == "--universal" ]]; then
  echo "==> 合并通用包（lipo）"
  OUT="src-tauri/target/universal-release/AnswerGate.app"
  rm -rf "$OUT"
  mkdir -p "$(dirname "$OUT")"
  cp -R "$APP_A" "$OUT"
  lipo -create \
    "$APP_A/Contents/MacOS/$BIN" \
    "$APP_B/Contents/MacOS/$BIN" \
    -output "$OUT/Contents/MacOS/$BIN"
  echo "==> 通用包：$OUT"
  echo "    （可用 hdiutil create -volname AnswerGate -srcfolder '$(dirname "$OUT")' -ov -format UDZO AnswerGate.dmg 制作 dmg）"
fi

echo "==> 完成。产物："
ls -d src-tauri/target/$TARGET_A/release/bundle/macos/*.app src-tauri/target/$TARGET_A/release/bundle/dmg/*.dmg 2>/dev/null || true
ls -d src-tauri/target/$TARGET_B/release/bundle/macos/*.app src-tauri/target/$TARGET_B/release/bundle/dmg/*.dmg 2>/dev/null || true
