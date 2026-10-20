# MineKeeper (Rust) — 跨平台扫雷

Windows XP `winmine.exe` 的行为复刻，用 Rust 从零实现，**三平台共用一份确定性光栅器**。

> 状态：**可交付**。内核、菜单栏、四个对话框、计时、纪录、音效全部接线；
> 15 张渲染基线已录制并逐像素校验。设计文档见
> [`docs/2026-10-20-rust-port-design.md`](docs/2026-10-20-rust-port-design.md)。

## 技术栈

| 层 | 方案 |
|---|---|
| 平台 | Windows / macOS / Linux，单一可执行文件 |
| 渲染 | `tiny-skia`，三平台同一份，**一套基线精确比对** |
| 字形 | `ab_glyph`（界面文本）+ `fontdue`（格内数字，**字体内嵌**） |
| 窗口 / 音频 | `winit` + `softbuffer` / `cpal` |
| 依赖 | 约 150 个 crate（见设计文档 §4） |

场景脚本是纯文本格式（`board` / `size` / `mines` / `open` …），`testdata/scenes/`
里有 20 份可直接运行。

## 快速开始

```sh
cargo test --workspace                 # 96 项单元测试
cargo run -p minekeeper-cli --release -- --selftest 2000   # 22 项无窗口自检
cargo run -p minekeeper-cli --release -- --golden          # 15 张渲染基线逐像素校验
cargo run -p minekeeper-cli --release -- --script testdata/scenes/expert-fresh.txt /tmp/a.png
cargo run -p minekeeper-cli --release -- --ui /tmp/menu.png menu   # 离屏预览界面
cargo build -p minekeeper --release    # GUI（当前平台上运行）
cargo run -p minekeeper --release -- --smoke-test   # 建窗口 + 画一帧即退出（CI 用）

# 一次出 macOS + Windows 的全部产物
./scripts/build-all.sh
./scripts/build-all.sh --macos      # 只出 macOS
./scripts/build-all.sh --windows    # 只出 Windows
./scripts/build-all.sh --linux      # 只出 Linux
./scripts/build-all.sh --run        # 构建并启动
```

## 构建产物

```sh
./scripts/build-all.sh
```

```
dist/
  macos/MineKeeper.app              通用包（arm64 + x86_64），双击运行
  macos/minekeeper-cli              通用命令行
  windows-x64/minekeeper.exe        PE Subsystem = 2 (GUI)，双击不闪控制台
  windows-x64/minekeeper-cli.exe
  linux-x64/minekeeper              Linux GUI（仅本机构建，见下）
  linux-x64/minekeeper-cli
  SHA256SUMS
```

**依赖**：Rust stable + 目标 `aarch64-apple-darwin` / `x86_64-apple-darwin` /
`x86_64-pc-windows-gnu` / `x86_64-unknown-linux-musl`，以及 Homebrew 的 `mingw-w64`
（提供 `dlltool`，Windows 交叉链接必需）。缺任何一个都可以用 `--install-deps` 让脚本自己装。

**Linux 的两条路径**：在 Linux 上构建走本机 `x86_64-unknown-linux-gnu`，出 GUI + CLI，
带音频；在 macOS/Windows 上交叉构建走 `x86_64-unknown-linux-musl`，**只出 CLI**。

交叉不产 GUI 是刻意的：静态 musl 没有动态加载器，`dlopen` 必然失败，而 winit 的
X11/Wayland 后端正是靠 `libloading` 在运行时打开 `libX11` / `libwayland-client`。
那样出来的 GUI 能编译能链接，但一建窗口就失败，不该发布。
CLI 不碰窗口栈，所以静态 musl 下可以正常工作。

Linux 的 GUI 请在 Linux 上构建，或取 CI `build-gui (ubuntu)` 的产物。

脚本会先跑一遍自检，再验证产物（macOS 查双架构、Windows 查 PE 子系统），
不是「构建成功就算完」。

## 为什么内嵌字体

格内数字的字形来自字体。如果走系统字体，三平台会加载三个不同字体
（macOS Arial Bold / Windows arialbd / Linux DejaVuSans-Bold），渲染结果因此不同，
一套渲染基线无法在三平台同时成立（实测：换字体后 15 张里 9 张失败）。

内嵌一份 [Liberation Sans Bold](assets/fonts/)（SIL OFL 1.1，与 Arial 度量兼容）后，
格内数字与平台无关，`testdata/golden/` 的 15 张基线在三平台都成立。
代价是二进制 +405 KB（1.27 MB / arm64）。

界面中文仍用系统字体 —— CJK 字体 16–60 MB，内嵌不现实；界面文本不进渲染基线。

## 与原版的行为差异

只有一条，且默认关闭：

**「游戏 → 单击快速展开」** —— 打开后左键单独点一个已翻开的数字也能触发快速展开
（原版要求中键或左右键同按）。触控板没有中键，加这个开关是为了让 chord 在 macOS 上可达。
**关闭时与原版逐位一致**（默认关闭）。详见设计文档 §3.4。

## 目录

```
assets/fonts/       内嵌的格内数字字体（Liberation Sans Bold，SIL OFL 1.1）
crates/
  mk-core/          平台无关内核：game / theme / raster / pix / view / chrome / text / sound / store / scene
  mk-host/          宿主层：winit + softbuffer 窗口、cpal 音频（全部跨平台）
  minekeeper/       GUI 二进制
  minekeeper-cli/   无窗口二进制：自检 / 出图 / 比对 / 雷位生成 / 图标 / 界面预览
scripts/            build-all.sh（macOS 通用包 + Windows exe）
testdata/
  scenes/           场景脚本（20 份，纯文本，见设计文档 §3.1 FR-9）
  golden/           渲染基线（15 张，--golden 逐像素校验）
```

依赖严格单向：**逻辑层不许 import 任何绘图或平台相关的东西**。
新增功能时先想清楚落在哪一层 —— `game` 里出现 `raster` 就是分层破了。
