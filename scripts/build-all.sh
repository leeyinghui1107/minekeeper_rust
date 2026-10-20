#!/usr/bin/env bash
#
# 一次构建出 macOS 与 Windows 的可执行文件。
#
#   ./scripts/build-all.sh                  构建 macOS + Windows
#   ./scripts/build-all.sh --macos          只构建 macOS
#   ./scripts/build-all.sh --windows        只构建 Windows
#   ./scripts/build-all.sh --install-deps   缺依赖时自动安装
#   ./scripts/build-all.sh --run            构建完启动 macOS 版
#   ./scripts/build-all.sh --clean          先清空 dist/
#
# 产物：
#   dist/macos/MineKeeper.app          macOS 通用包（arm64 + x86_64）
#   dist/macos/minekeeper-cli          macOS 通用命令行（自检 / 出图 / 比对）
#   dist/windows-x64/minekeeper.exe    Windows GUI（PE 子系统 = GUI，无控制台）
#   dist/windows-x64/minekeeper-cli.exe
#   dist/SHA256SUMS
#
# 依赖（macOS 上）：
#   - Rust stable，且已装目标：aarch64-apple-darwin、x86_64-apple-darwin、x86_64-pc-windows-gnu
#   - Homebrew 的 mingw-w64（提供 x86_64-w64-mingw32-gcc / dlltool，Windows 交叉链接用）
#   - Xcode 命令行工具（提供 lipo / iconutil / codesign）
#
# 用 --install-deps 可以让脚本自己装上面缺的那些。

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DIST="$ROOT/dist"
WANT_MACOS=0
WANT_WINDOWS=0
INSTALL_DEPS=0
CLEAN=0
RUN_AFTER=0

APP_NAME="MineKeeper"
BUNDLE_ID="com.minekeeper.rust"
VERSION="0.1.0"

# ── 参数 ────────────────────────────────────────────────────────────────────

usage() {
    sed -n '2,26p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

while [ $# -gt 0 ]; do
    case "$1" in
        --macos)        WANT_MACOS=1 ;;
        --windows)      WANT_WINDOWS=1 ;;
        --install-deps) INSTALL_DEPS=1 ;;
        --clean)        CLEAN=1 ;;
        --run)          RUN_AFTER=1; WANT_MACOS=1 ;;
        -h|--help)      usage; exit 0 ;;
        *) echo "未知参数：$1" >&2; usage >&2; exit 2 ;;
    esac
    shift
done
if [ "$WANT_MACOS" -eq 0 ] && [ "$WANT_WINDOWS" -eq 0 ]; then
    WANT_MACOS=1
    WANT_WINDOWS=1
fi

# ── 小工具 ──────────────────────────────────────────────────────────────────

say()  { printf '\033[36m==>\033[0m %s\n' "$*"; }
ok()   { printf '  \033[32m✓\033[0m %s\n' "$*"; }
warn() { printf '  \033[33m!\033[0m %s\n' "$*"; }
die()  { printf '\033[31m✗ %s\033[0m\n' "$*" >&2; exit 1; }

size_of() {
    if [ -f "$1" ]; then
        stat -f%z "$1" 2>/dev/null || stat -c%s "$1"
    else
        echo 0
    fi
}

human() {
    awk -v b="$1" 'BEGIN{ if (b>=1048576) printf "%.2f MB", b/1048576; else printf "%.0f KB", b/1024 }'
}

has_cmd() { command -v "$1" >/dev/null 2>&1; }

has_target() {
    rustup target list --installed 2>/dev/null | grep -qx "$1"
}

ensure_target() {
    if has_target "$1"; then
        ok "rust target $1"
        return
    fi
    if [ "$INSTALL_DEPS" -eq 1 ]; then
        say "安装 rust target $1"
        rustup target add "$1"
    else
        die "缺少 rust target $1。先跑 rustup target add $1，或加 --install-deps"
    fi
}

ensure_cmd() {
    local cmd="$1" hint="$2"
    if has_cmd "$cmd"; then
        ok "$cmd"
        return
    fi
    if [ "$INSTALL_DEPS" -eq 1 ]; then
        say "$hint"
        eval "$hint"
        has_cmd "$cmd" || die "安装后仍找不到 $cmd"
    else
        die "缺少 $cmd。先执行「$hint」，或加 --install-deps"
    fi
}

# ── 环境检查 ────────────────────────────────────────────────────────────────

say "检查构建环境"
has_cmd cargo || die "找不到 cargo，先装 Rust 工具链（https://rustup.rs）"
has_cmd rustup || die "找不到 rustup"
ok "cargo $(cargo --version | awk '{print $2}')"

if [ "$CLEAN" -eq 1 ]; then
    say "清空 dist/"
    rm -rf "$DIST"
fi
mkdir -p "$DIST"

# ── macOS ───────────────────────────────────────────────────────────────────

build_macos() {
    say "macOS：构建两个架构并合并为通用二进制"
    ensure_target aarch64-apple-darwin
    ensure_target x86_64-apple-darwin
    ensure_cmd lipo "xcode-select --install"
    ensure_cmd iconutil "xcode-select --install"

    cargo build --release --target aarch64-apple-darwin -p minekeeper -p minekeeper-cli
    cargo build --release --target x86_64-apple-darwin  -p minekeeper -p minekeeper-cli

    local out="$DIST/macos"
    mkdir -p "$out"

    lipo -create \
        target/aarch64-apple-darwin/release/minekeeper \
        target/x86_64-apple-darwin/release/minekeeper \
        -output "$out/.minekeeper.universal"
    lipo -create \
        target/aarch64-apple-darwin/release/minekeeper-cli \
        target/x86_64-apple-darwin/release/minekeeper-cli \
        -output "$out/minekeeper-cli"

    # 通用二进制要在本机能跑，先用它自检一次再打包。
    say "自检（通用 CLI）"
    "$out/minekeeper-cli" --selftest 500 >/dev/null
    ok "22 项自检通过"

    say "生成图标并打包 .app"
    local app="$out/$APP_NAME.app"
    local iset="$out/MineKeeper.iconset"
    rm -rf "$app" "$iset"
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$iset"

    local gen=("$out/minekeeper-cli" --icon)
    "${gen[@]}" "$iset/icon_16x16.png" 16      >/dev/null
    "${gen[@]}" "$iset/icon_16x16@2x.png" 32   >/dev/null
    "${gen[@]}" "$iset/icon_32x32.png" 32      >/dev/null
    "${gen[@]}" "$iset/icon_32x32@2x.png" 64   >/dev/null
    "${gen[@]}" "$iset/icon_128x128.png" 128   >/dev/null
    "${gen[@]}" "$iset/icon_128x128@2x.png" 256 >/dev/null
    "${gen[@]}" "$iset/icon_256x256.png" 256   >/dev/null
    "${gen[@]}" "$iset/icon_256x256@2x.png" 512 >/dev/null
    "${gen[@]}" "$iset/icon_512x512.png" 512   >/dev/null
    "${gen[@]}" "$iset/icon_512x512@2x.png" 1024 >/dev/null
    iconutil -c icns "$iset" -o "$app/Contents/Resources/$APP_NAME.icns"
    rm -rf "$iset"

    mv "$out/.minekeeper.universal" "$app/Contents/MacOS/minekeeper"
    chmod +x "$app/Contents/MacOS/minekeeper"

    cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>          <string>minekeeper</string>
    <key>CFBundleIdentifier</key>          <string>$BUNDLE_ID</string>
    <key>CFBundleName</key>                <string>$APP_NAME</string>
    <key>CFBundleDisplayName</key>         <string>扫雷</string>
    <key>CFBundlePackageType</key>         <string>APPL</string>
    <key>CFBundleShortVersionString</key>  <string>$VERSION</string>
    <key>CFBundleVersion</key>              <string>$VERSION</string>
    <key>CFBundleIconFile</key>            <string>$APP_NAME</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>LSMinimumSystemVersion</key>      <string>10.15</string>
    <key>NSHighResolutionCapable</key>     <true/>
    <key>LSApplicationCategoryType</key>   <string>public.app-category.puzzle-games</string>
</dict>
</plist>
PLIST

    # ad-hoc 签名：本机可双击运行；分发到其他机器仍会被 Gatekeeper 拦。
    codesign --force --deep --sign - "$app" >/dev/null 2>&1 || warn "ad-hoc 签名失败（不影响本机运行）"
    ok "$app"
}

# ── Windows ─────────────────────────────────────────────────────────────────

build_windows() {
    say "Windows：x86_64-pc-windows-gnu"
    ensure_target x86_64-pc-windows-gnu
    # windows-sys 需要 dlltool 生成导入库，它来自 mingw-w64 的 binutils。
    ensure_cmd x86_64-w64-mingw32-gcc   "brew install mingw-w64"
    ensure_cmd x86_64-w64-mingw32-dlltool "brew install mingw-w64"

    cargo build --release --target x86_64-pc-windows-gnu -p minekeeper -p minekeeper-cli

    local src="target/x86_64-pc-windows-gnu/release"
    local out="$DIST/windows-x64"
    mkdir -p "$out"
    cp "$src/minekeeper.exe" "$out/"
    cp "$src/minekeeper-cli.exe" "$out/"

    # 守住「双击不闪控制台」：PE 可选头的 Subsystem 必须是 2 (GUI)。
    local sub
    sub=$(python3 - "$out/minekeeper.exe" <<'PY'
import struct, sys
d = open(sys.argv[1], 'rb').read()
pe = struct.unpack('<I', d[0x3c:0x40])[0]
print(struct.unpack('<H', d[pe + 0x5c:pe + 0x5e])[0])
PY
)
    if [ "$sub" != "2" ]; then
        die "minekeeper.exe 的 PE Subsystem 是 $sub，应为 2（GUI）。检查 #![windows_subsystem] 是否被删掉"
    fi
    ok "PE Subsystem = 2 (GUI)"
    ok "$out/minekeeper.exe"
}

# ── 执行 ────────────────────────────────────────────────────────────────────

if [ "$WANT_MACOS" -eq 1 ]; then build_macos; fi
if [ "$WANT_WINDOWS" -eq 1 ]; then build_windows; fi

# ── 校验和与摘要 ────────────────────────────────────────────────────────────

say "生成校验和"
(
    cd "$DIST"
    find . -type f \( -name '*.exe' -o -name 'minekeeper-cli' -o -name 'minekeeper' \) \
        | sort | xargs shasum -a 256 > SHA256SUMS
)
ok "dist/SHA256SUMS"

say "产物"
printf '  %-46s %10s\n' "文件" "大小"
printf '  %-46s %10s\n' "----------------------------------------------" "----------"
while IFS= read -r f; do
    printf '  %-46s %10s\n' "${f#"$DIST"/}" "$(human "$(size_of "$f")")"
done < <(find "$DIST" -type f ! -name 'SHA256SUMS' | sort)

if [ "$RUN_AFTER" -eq 1 ] && [ "$WANT_MACOS" -eq 1 ]; then
    say "启动"
    open -a "$DIST/macos/$APP_NAME.app"
fi
