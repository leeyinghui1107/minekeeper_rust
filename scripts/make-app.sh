#!/usr/bin/env bash
# 构建 macOS .app 包。
#
#   ./scripts/make-app.sh            构建 release 并打包
#   ./scripts/make-app.sh --run      打包后直接启动
#
# 产物：dist/MineKeeper.app（ad-hoc 签名，可双击运行）
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

APP="dist/MineKeeper.app"
IS="dist/MineKeeper.iconset"

echo "==> cargo build --release"
cargo build --release -p minekeeper -p minekeeper-cli

echo "==> 生成图标（用游戏内同一套绘制代码现画）"
rm -rf "$APP" "$IS"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$IS"
CLI=target/release/minekeeper-cli
gen() { "$CLI" --icon "$IS/$1" "$2" >/dev/null; }
gen icon_16x16.png 16;      gen icon_16x16@2x.png 32
gen icon_32x32.png 32;      gen icon_32x32@2x.png 64
gen icon_128x128.png 128;   gen icon_128x128@2x.png 256
gen icon_256x256.png 256;   gen icon_256x256@2x.png 512
gen icon_512x512.png 512;   gen icon_512x512@2x.png 1024
iconutil -c icns "$IS" -o "$APP/Contents/Resources/MineKeeper.icns"
rm -rf "$IS"

echo "==> 组装 bundle"
cp target/release/minekeeper "$APP/Contents/MacOS/minekeeper"
cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>          <string>minekeeper</string>
    <key>CFBundleIdentifier</key>          <string>com.minekeeper.rust</string>
    <key>CFBundleName</key>                <string>MineKeeper</string>
    <key>CFBundleDisplayName</key>         <string>扫雷</string>
    <key>CFBundlePackageType</key>         <string>APPL</string>
    <key>CFBundleShortVersionString</key>  <string>0.1.0</string>
    <key>CFBundleVersion</key>             <string>0.1.0</string>
    <key>CFBundleIconFile</key>            <string>MineKeeper</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>LSMinimumSystemVersion</key>      <string>10.15</string>
    <key>NSHighResolutionCapable</key>     <true/>
    <key>LSApplicationCategoryType</key>   <string>public.app-category.puzzle-games</string>
</dict>
</plist>
PLIST

echo "==> ad-hoc 签名"
codesign --force --deep --sign - "$APP"
codesign --verify --verbose=1 "$APP"

echo
echo "产物：$ROOT/$APP"
du -sh "$APP"

if [ "${1:-}" = "--run" ]; then
    echo "==> 启动"
    open -a "$ROOT/$APP"
fi
