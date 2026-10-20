# MineKeeper (Rust) — 跨平台扫雷

Windows XP `winmine.exe` 的行为复刻，用 Rust 从零实现，**三平台共用一份确定性光栅器**。

> 状态：**可交付**。内核、菜单栏、四个对话框、计时、纪录、音效全部接线；
> 15 张渲染基线已录制并逐像素校验。设计文档见
> [`../docs/2026-10-20-rust-port-design.md`](../docs/2026-10-20-rust-port-design.md)。

## 与 Go 版的关系

| | Go 版（`../go`） | Rust 版（本目录） |
|---|---|---|
| 平台 | 仅 Windows x64 | Windows / macOS / Linux |
| 渲染 | GDI+（Windows）/ 自研（其他） | `tiny-skia`，三平台同一份 |
| 字形 | GDI hinting 墨迹 | `fontdue` 轮廓 + 阈值量化 |
| 基线 | 按后端分组 | **一套，精确比对** |
| 依赖 | 零第三方 | 约 150 个 crate（见设计文档 §4） |

场景脚本格式与 Go 版**完全一致**，`../go/testdata/scenes/*.txt` 可以直接复用。

## 快速开始

```sh
cargo test --workspace                 # 74 项单元测试
cargo run -p minekeeper-cli --release -- --selftest 2000   # 22 项无窗口自检
cargo run -p minekeeper-cli --release -- --golden          # 15 张渲染基线逐像素校验
cargo run -p minekeeper-cli --release -- --script testdata/scenes/expert-fresh.txt /tmp/a.png
cargo run -p minekeeper-cli --release -- --ui /tmp/menu.png menu   # 离屏预览界面
cargo build -p minekeeper --release    # GUI（当前平台上运行）

# macOS 打包成 .app
./scripts/make-app.sh --run
```

## 与原版的行为差异

只有一条，且默认关闭：

**「游戏 → 单击快速展开」** —— 打开后左键单独点一个已翻开的数字也能触发快速展开
（原版要求中键或左右键同按）。触控板没有中键，加这个开关是为了让 chord 在 macOS 上可达。
**关闭时与原版逐位一致**（默认关闭）。详见设计文档 §3.4。

## 目录

```
crates/
  mk-core/          平台无关内核：game / theme / raster / pix / view / chrome / text / sound / store / scene
  mk-host/          宿主层：winit + softbuffer 窗口、cpal 音频（全部跨平台）
  minekeeper/       GUI 二进制
  minekeeper-cli/   无窗口二进制：自检 / 出图 / 比对 / 雷位生成 / 图标 / 界面预览
testdata/
  scenes/           场景脚本（20 份，与 Go 版同格式，直接复用）
  golden/           渲染基线（15 张，--golden 逐像素校验）
```

依赖严格单向，规则见 [`../go/CONTRIBUTING.md`](../go/CONTRIBUTING.md) 的同一套：
**逻辑层不许 import 任何绘图或平台相关的东西**。
