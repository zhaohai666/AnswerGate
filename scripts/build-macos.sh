#!/usr/bin/env bash
# macOS 打包：aarch64 + x86_64 双 target 分别出 .app；--universal 额外合并 lipo 通用包。
# dmg 用 hdiutil 制作（不依赖 create-dmg/bundle_dmg.sh，规避其环境兼容问题）。
# 用法：scripts/build-macos.sh [--universal] [--dmg]
set -euo pipefail
cd "$(dirname "$0")/.."

TARGET_A=aarch64-apple-darwin
TARGET_B=x86_64-apple-darwin
BIN=answergate
MAKE_DMG=0
[[ "${1:-}" == "--universal" ]] && UNIVERSAL=1 || UNIVERSAL=0
for arg in "$@"; do [[ "$arg" == "--dmg" ]] && MAKE_DMG=1; done

rustup target add "$TARGET_A" "$TARGET_B"

echo "==> 打包 $TARGET_A（仅 .app，dmg 由 hdiutil 单独制作）"
npm run tauri build -- --target "$TARGET_A" --bundles app
echo "==> 打包 $TARGET_B"
npm run tauri build -- --target "$TARGET_B" --bundles app

APP_A="src-tauri/target/$TARGET_A/release/bundle/macos/AnswerGate.app"
APP_B="src-tauri/target/$TARGET_B/release/bundle/macos/AnswerGate.app"

if [[ $UNIVERSAL == 1 ]]; then
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
fi

if [[ $MAKE_DMG == 1 ]]; then
  echo "==> hdiutil 制作 dmg（架构：$TARGET_A 产物）"
  hdiutil create -volname AnswerGate \
    -srcfolder "src-tauri/target/$TARGET_A/release/bundle/macos" \
    -ov -format UDZO "AnswerGate_${TARGET_A}.dmg"
fi

echo "==> 完成。产物："
ls -d src-tauri/target/$TARGET_A/release/bundle/macos/*.app 2>/dev/null || true
ls -d src-tauri/target/$TARGET_B/release/bundle/macos/*.app 2>/dev/null || true
[[ $UNIVERSAL == 1 ]] && ls -d src-tauri/target/universal-release/*.app || true
[[ $MAKE_DMG == 1 ]] && ls -1 *.dmg || true
