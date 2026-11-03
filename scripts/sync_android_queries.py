#!/usr/bin/env python3
"""从 resources/config.toml 读取 android 包名，写入 gen/android 主应用
AndroidManifest.xml 的 <queries> 节点（Android 11+ 包可见性，架构决策 U2）。

用法：python3 scripts/sync_android_queries.py
先决条件：已运行 `npm run tauri android init` 生成 src-tauri/gen/android。
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CONFIG = ROOT / "resources" / "config.toml"
MANIFEST = ROOT / "src-tauri" / "gen" / "android" / "app" / "src" / "main" / "AndroidManifest.xml"
MARK_START = "<!-- answergate:queries:start -->"
MARK_END = "<!-- answergate:queries:end -->"


def main() -> int:
    packages = re.findall(r'^\s*android\s*=\s*"([^"]+)"', CONFIG.read_text(encoding="utf-8"), re.M)
    packages = list(dict.fromkeys(packages))  # 去重保序

    if not packages:
        print("[sync_android_queries] config.toml 未配置 android 包名，跳过")
        return 0
    if not MANIFEST.exists():
        print(
            f"[sync_android_queries] 未找到 {MANIFEST}，"
            "请先运行 `npm run tauri android init` 后重试"
        )
        return 1

    xml = MANIFEST.read_text(encoding="utf-8")
    block = (
        f"{MARK_START}\n"
        "    <queries>\n"
        + "".join(f'        <package android:name="{p}" />\n' for p in packages)
        + "    </queries>\n"
        f"    {MARK_END}"
    )

    if MARK_START in xml:
        xml = re.sub(
            f"{re.escape(MARK_START)}.*?{re.escape(MARK_END)}", block, xml, flags=re.S
        )
    elif "<application" in xml:
        xml = xml.replace("<application", f"{block}\n    <application", 1)
    else:
        xml = xml.replace("</manifest>", f"{block}\n</manifest>")

    MANIFEST.write_text(xml, encoding="utf-8")
    print(f"[sync_android_queries] 已写入 {len(packages)} 个包名的 <queries> → {MANIFEST}")
    print("[sync_android_queries] 注意：config 新增包名后需重新打包才能生效（U2 决策）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
