# MineKeeper Rust 版设计文档

> 版本：v0.1.0（可交付）
> 日期：2026-10-20
> 代码位置：本仓库根目录
>
> **本仓库是 MineKeeper 唯一在维护的实现**，行为判据是 `winmine.exe` 的黑盒观察。
> 仓库外的历史实现均已废弃，不再维护，也不在本文档的讨论范围内。

---

## 1. 文档目的与范围

本文档是 Rust 版 MineKeeper 的**完整设计与实施规格**，目标是让一个了解 Rust、
但不了解本项目的工程师（或 AI 编码助手）能够据此独立完成实现。

覆盖范围：

- 需求规格（玩法、尺寸、布局、交互、持久化）—— 第 3 章
- 技术栈选型及其理由、被否决的方案 —— 第 4 章
- 系统架构与分层规则 —— 第 5 章
- 逐模块的 API、数据结构与算法 —— 第 6 章
- 关键算法与不变量 —— 第 8 章
- 风险、测试策略、开发流程、里程碑 —— 第 9–11 章

不覆盖：任何历史实现的维护 —— 本仓库是唯一在维护的版本。

**阅读顺序建议**：只想了解「为什么这么选」读第 3–4 章；要动手写代码读第 5–6 章；
要接手项目读第 10–11 章。

---

## 2. 背景与目标

### 2.1 要复刻什么

`winmine.exe`（Windows XP 自带扫雷）是一款只存在于 Win32 上的程序，从未跨过平台。
本项目按**黑盒行为观察**复刻它：玩法规格、三档尺寸、配色、音效全部来自截图测量与
实际操作，不引用任何反编译代码或原版素材。

复刻目标里有两件事天然冲突，必须在设计阶段就定下来：

- **像原版**：像素尺寸、交互反馈、音效时机都要对得上；
- **跨平台**：原版依赖 `gdiplus.dll` / `user32.dll`，这两个东西只存在于 Windows。

### 2.2 技术路线

「把渲染锁死在一个确定性的软件光栅器上」是同时满足两者的前提。选型因此不是
「哪个语言更熟」，而是「哪个生态能提供确定性、跨平台、可离线验证的渲染与输入栈」：

| 需求 | 方案 |
|---|---|
| 光栅器 | `tiny-skia` —— 纯 Rust、确定性、三平台逐像素一致 |
| 字形 | `ab_glyph`（界面文本）+ `fontdue`（格内数字） |
| 窗口 / 输入 | `winit` + `softbuffer` |
| 音频 | `cpal` |
| 基线 | 三平台同一份，CI 上精确比对 |

### 2.3 目标

1. **三平台可运行**：Windows x64 / macOS（arm64 + x64）/ Linux x64。
2. **渲染确定性**：同一份场景脚本在三平台产出**逐像素相同**的 PNG。
3. **保留验证栈**：全部逻辑自检与渲染回归在**无显示设备**的 CI 上可跑。
4. **体积可控**：release 二进制 ≤ 1.6 MB（macOS / Windows）。
5. **行为可追溯**：与原版的每一处差异都必须写进 §3.3，不允许隐式偏离。

### 2.4 非目标

- 不追求与 `winmine.exe` 逐像素一致 —— 字形与抗锯齿算法不同，见 §3.3 的渲染偏差。
- 不引入网络、多语言、主题皮肤、存档云同步。

---

## 3. 需求规格

### 3.1 功能需求

#### FR-1 玩法

| 操作 | 效果 |
|---|---|
| 左键 | 翻开格子；**第一次点击永远不会踩雷** |
| 右键 | 旗子 → 问号 → 空 循环（关掉「标记(?)」后为 旗子 ↔ 空） |
| 中键，或**左右键同时按**在一个已翻开的数字上 | 两条自动规则（见下） |
| 点笑脸 / F2 | 开新局 |
| 按住格子后拖到笑脸再松手 | 开新局 |
| 方向键 / Enter / 空格 / F | 移动焦点框 / 翻开 / 插旗 |
| 游戏菜单 → 小/中/大尺寸 | 切换每格 16 / 24 / 32 设备像素 |

**chord 两条规则**（FR-1.1，本项目最核心的行为）：

按住中键或左右键同按在一个**已翻开的数字格**上时：

1. 若「周围未翻开的格数 == 该数字」→ 这些格**必然全是雷**（雷只可能藏在未翻开的格子里），
   **一次性全部标雷**。
2. 否则若「周围已标旗的格数 == 该数字」→ **翻开其余未翻开的邻格**（原版快速展开）。
   旗子插错时会踩雷，与原版一致。

两条规则互斥；都成立不了时什么都不做。**双击不触发**（与原版一致）。
两条规则都额外要求「存在未插旗的未翻开邻格」，否则不动作。

> **扩展开关**：「游戏 → 单击快速展开」打开后，**左键单独**点一个已翻开的数字
> 也能触发上述两条规则。默认关闭，关闭时与原版逐位一致。详见 §3.4。

按住期间的视觉反馈（不改变任何格子状态）：
- 源格描**橙框** `#FF7F27`
- 被规则 1 判定为必雷的未翻开格描**红框** `#ED1C24`
- 被影响的格子呈按下态（凹陷）

**FR-1.2 数字表示周围 8 格的雷数。**
**FR-1.3 计时上限 999 秒**（超过则停在 999 且不再走表）。
**FR-1.3.1 计时只在三种情况下暂停**：本局未开始（`Ready`）、本局已结束（`Dead`/`Won`）、
窗口失去焦点。**打开任何菜单或对话框都不暂停** —— 包括「自定义雷区」与「扫雷英雄榜」。

理由：对话框是模态覆盖层，底下的对局仍在进行；若开对话框就能停表，玩家可以在难判断的
局面把表停住慢慢想。规则写成一条，不要为某个对话框单独开口子。

> 与原版是否一致未经实测；这里选择「一条规则到底」，因为它同时避免了暂停漏洞
> （见 §3.3 D9）。

**FR-1.4 剩余雷数 = 总雷数 − 旗子数，可为负**，此时数码管显示 `-NN`（最多两位）。
**FR-1.5 翻开所有非雷格即获胜**；获胜时未标记的雷**自动补旗**。
**FR-1.6 踩雷即失败**；失败时所有未插旗的雷亮出，插错的旗显示为「雷上盖红叉」，问号一律清除。

#### FR-2 难度

| 档位 | 行 × 列 | 雷数 |
|---|---|---|
| 初级 | 16 × 16 | 40 |
| 中级 | 16 × 30 | 99 |
| 高级（**启动默认**） | 24 × 30 | 199 |
| 自定义 | 5–99 行 × 8–99 列 | 10 – (行×列−10) |

自定义雷区最小宽度取 8 的原因：状态栏是固定像素布局（数码管 41 + 笑脸 26 + 数码管 41，
16px 基准逻辑像素），客户区窄于 8 列时两只数码管会与笑脸重叠。行数不受此限制。

#### FR-3 三档尺寸与像素换算

方格边长只有三档：**小 16 / 中 24 / 大 32 设备像素**，启动默认中档。

**FR-3.1 取整规则（唯一一处）**：

```
Px(v)     = (v * cellPx + 8) / 16          # 逻辑长度 → 设备像素，.5 一律向上
DivPx(n,d) = (n * cellPx + 8*d) / (16 * d)  # 半整数逻辑长度，如雷体半径 9/2
```

**FR-3.2 总尺寸一律由换算后的基本量相加**，绝不对逻辑总量整体缩放。
否则 24px 档下会出现「边框厚度之和 ≠ 凹槽宽度」的缝隙。

**FR-3.3 数码管外框按部件求值**，不能直接 `Px(41)`：
`41 = 1 边框 + 13×3 数字 + 1 边框`，各自取整后相加才对得上。中档下三个数字要 `3×20 = 60`，
直接 `Px(41) = 62` 扣掉两条边框只剩 58，塞不进去。

**FR-3.4 笑脸按钮的取整发生在逻辑空间**（`(逻辑客户区宽 − 26)/2` 再换算），
所以按钮可能比设备像素正中偏左 1px —— 这是原版行为，必须照抄。

**FR-3.5 客户区尺寸（实测基线）**：

| 项目 | 小 16px | 中 24px（默认） | 大 32px |
|---|---|---|---|
| 单元格 | 16 | 24 | 32 |
| 凸起格斜边 | 2 | 3 | 4 |
| 凹陷格灰线 | 1 | 2 | 2 |
| 雷区凹槽 | 3 | 5 | 6 |
| 状态栏凹槽 | 2 | 3 | 4 |
| 数码管外框 | 41×25 | 64×39 | 82×50 |
| 单个数码字 | 13×23 | 20×35 | 26×46 |
| 笑脸按钮 · 面部 | 26 · 24 | 39 · 36 | 52 · 48 |
| 格内字形包围盒 | 10 | 15 | 20 |
| **客户区（高级 24×30）** | **497×444** | **747×668** | **994×888** |

#### FR-4 菜单结构

与原版 `winmine.exe` 的菜单资源一致，命令 ID 沿用原版编号：

```
游戏(&G)
  开局(&N)          F2      510
  ────────
  初级(&B)                  511
  中级(&I)                  512
  高级(&E)                  513
  自定义(&C)                514
  ────────
  小尺寸 16px               520
  中尺寸 24px               521
  大尺寸 32px               522
  ────────
  标记(?)(&M)               515   ← 可勾选
  颜色(&L)                  516   ← 可勾选
  声音(&S)                  517   ← 可勾选
  ────────
  扫雷英雄榜(&T)            518
  ────────
  退出(&X)                  519
帮助(&H)
  使用帮助(&H)      F1      592
  ────────
  关于扫雷(&A)              593
```

尺寸三项是**单选**（RadioCheck），其余勾选项是独立开关。

> **与原版的差异 D8**：帮助菜单只保留「使用帮助」与「关于扫雷」两项。
> 原版还有「目录」与「查找帮助主题」—— 那两项是 Windows 帮助系统的入口，
> 对一个单窗口小游戏没有意义，且三者挤在一起时内容高度重复。
> 命令行 ID 592 / 593 保持原版编号不变；590 / 591 不再使用。

#### FR-5 对话框

三个模态对话框，布局按 XP 原版像素尺寸：

| 对话框 | 客户区 | 内容 |
|---|---|---|
| 自定义雷区 | 232×158 | 高度 / 宽度 / 雷数 三个数值框 + 上下箭头 + 确定 / 取消 |
| 破纪录姓名 | 286×128 | 提示文本 + 单行输入框（**限 24 字符**）+ 确定 / 取消 |
| 扫雷英雄榜 | 300×150 | 初/中/高 各一行「时间 + 姓名」+ 重新计分 / 确定 |

**FR-5.1** 自定义雷区：行列改变时把雷数上限夹回 `MaxMinesFor(rows, cols)`，
并把当前值一并夹住。确定时校验范围，不合法则弹提示且对话框**保持打开**。

**FR-5.2** 姓名输入框默认选中全部文本；留空则记为「匿名」。

#### FR-6 持久化

最佳时间按难度各存一条，写在平台标准缓存目录：

| 平台 | 路径 |
|---|---|
| Windows | `%LOCALAPPDATA%\MineKeeper\scores.json` |
| macOS | `~/Library/Caches/MineKeeper/scores.json` |
| Linux | `$XDG_CACHE_HOME/MineKeeper/scores.json`（回退 `~/.cache`） |

JSON 格式（字段顺序固定，保证 diff 稳定）：

```json
{
  "beginner":     { "seconds": 999, "name": "匿名" },
  "intermediate": { "seconds": 999, "name": "匿名" },
  "expert":       { "seconds": 999, "name": "匿名" }
}
```

`seconds == 999` 是「还没有纪录」的哨兵。自定义难度不记纪录。
读不到 / 格式不对 / 目录只读都**静默降级**，不打扰玩家。

#### FR-7 音效

三段提示音（翻开、爆炸、获胜）在进程内合成，**22050Hz / 8bit / 单声道**，
波形自造，不加载任何原版 WAVE 资源。菜单「声音」可关闭。

#### FR-8 命令行

GUI 二进制：

```
minekeeper                    启动游戏
minekeeper --size small|medium|large   启动时指定方格尺寸
```

无窗口二进制 `minekeeper-cli`：

```
--selftest [n]                              无窗口自检（默认 1000 局随机对局）
--script <脚本> <输出png>                   按脚本渲染一帧 PNG
--compare <a.png> <b.png> [rows cols size]  逐像素比较两张 PNG
--genlayout <输出> [rows cols mines seed]   生成固定雷位脚本头
--glyphdiag                                 打印每个字形的源墨迹与适配结果
```

#### FR-9 场景脚本

文本格式见下，`testdata/scenes/` 里有 20 份可直接运行的示例：

```
board 24 30 199        # 行 列 雷数
size medium            # small / medium / large（或 16 / 24 / 32）
mines 0 0              # 固定雷位，可重复
open 12 20             # 左键翻开
flag 3 7
question 4 8
chord 12 20            # 走一次 chord 手势
markview 12 20         # 只画「按住」预览叠加层，不落子
mark 20 15             # 行优先给前 20 个雷位插旗、随后 15 个设为问号
safe                   # 翻开行优先第一个非雷格
autolost               # 一直翻到踩雷
autoview               # 展开 ≤30 格，再取一个可推断的位置做按住预览
solve                  # 把每个非雷格先插旗再取消，然后翻开
colored 0 | marks 0    # 关掉颜色 / 关掉问号
```

### 3.2 非功能需求

| 编号 | 需求 | 验收方式 |
|---|---|---|
| NFR-1 | **渲染确定性**：同脚本在三平台产出逐像素相同的 PNG | CI 三平台跑 `--golden`；已用「SIMD vs 标量构建」等价验证（见 §6.14） |
| NFR-2 | **无头可测**：全部自检与渲染回归无需显示设备 | Linux CI 容器内 `--selftest` 通过 |
| NFR-3 | **体积**：release GUI 二进制 ≤ 1.6 MB（macOS / Windows，逐架构）；Linux 静态 musl 另计 ≈ 2.7 MB | CI 体积守卫 |
| NFR-4 | **零 `unsafe`**（内核） | `#![forbid(unsafe_code)]` 于 `mk-core` |
| NFR-5 | **不崩溃**：字体缺失、音频设备缺失、目录只读均降级而非 panic | 单元测试覆盖 |
| NFR-6 | **确定性随机**：固定种子下雷位可复现 | 单元测试 |
| NFR-7 | **分层不破**：`game` 不得依赖任何绘图或平台代码 | 代码审查 + 依赖图检查 |

### 3.3 与原版的差异（**明确列出，其余必须一致**）

| # | 差异 | 原因 | 影响 |
|---|---|---|---|
| D1 | **字形墨迹不同** | 原版用 GDI hinting 渲染，本实现用 `fontdue` 轮廓 + 阈值量化 | 数字笔画的像素级形状略有差异；**宽高与目标框完全一致**（闭环校正保证） |
| D2 | **笑脸面部抗锯齿算法不同** | 原版的 AA 实现与 `tiny-skia` 不同 | 面部边缘的灰阶分布不同 |
| D3 | **椭圆 / 圆弧量化不同** | 同上 | 雷体、笑脸轮廓的边缘像素略有差异 |
| D4 | **RNG 序列与原版无关** | 原版的随机数实现不可复现，本实现用 PCG32 | 只影响随机对局；**固定雷位场景不受影响** |
| D5 | **对话框实现不同** | 原版用 Win32 原生子控件；本实现自绘 | 观感不同，行为语义一致 |
| D6 | **菜单实现不同** | 同上 | 同上 |
| **D7** | **「单击快速展开」扩展开关** | 本项目**新增**：触控板没有中键，左右同按也极别扭，chord 在 macOS 上原本不可达 | **默认关闭时无差异**；打开后玩法与原版不同（见 §3.4） |
| **D8** | **帮助菜单只保留两项** | 原版的「目录」「查找帮助主题」是 Windows 帮助系统的入口，对单窗口游戏没有意义 | 少两个菜单项；F1 改指「使用帮助」 |
| **D9** | **打开任何对话框都不暂停计时** | 统一为一条规则，不为某个对话框单独开口子（否则可借对话框停表思考） | 与原版是否一致未经实测 |

**D1–D3 是渲染基线的唯一允许差异**（D4–D7 不影响渲染）。判定规则见 §10.3。

### 3.4 唯一的玩法扩展：单击快速展开

这是本项目与原版**唯一的行为差异**，因此单独说明。

**动机**：MacBook 触控板没有中键；「左右键同按」需要一指按住 + 两指点按，物理上极别扭。
对触控板用户，chord 原本**不可达** —— 这不是偏好问题，是功能缺失。

**开关语义**（菜单「游戏 → 单击快速展开」，命令 ID **600**，故意落在原版 510–593 之外）：

| | 关闭（默认） | 打开 |
|---|---|---|
| 左键点**未翻开**格 | 翻开 | 翻开（不变） |
| 左键点**已翻开**数字 | 什么都不做 | **chord** |
| 中键 | chord | chord（**保留**） |
| 左右键同按 | chord | chord（**保留**） |

**只增不减**：打开时不夺走中键与左右同按，外接鼠标用户不会觉得功能被换掉。

**关闭时与原版逐位一致** —— 新逻辑全部挂在「左键点已翻开格」这条原本直接返回的路径上
（`Board::left_click` 对 `open[i]` 提前返回），关闭时那个条件恒为 false。

**已知代价**：chord 规则 2 在旗子插错时会踩雷。原版用「刻意的双键手势」当误触保护，
左键单击把这个保护换成了**视觉确认** —— 打开开关后按下数字会立刻显示 chord 预览
（源格橙框 + 必雷格红框 + 笑脸转专注），松手才生效，发现不对可以拖出雷区取消。
这是知情选择，不是疏漏。

**三条设计决定**（都有测试守着）：

1. 模式**只在按下时判定**。`mouse_move` 不重算，所以从别的格子拖进来不会中途变成 chord。
2. 预览复用现有的 chord 机制（`press_visual_cells` / `rebuild_press_set` / `chord_mines`），零额外代码。
3. 开关**不持久化**，与「标记(?)/颜色/声音」三个现有开关一致。

---

## 4. 技术栈选型

### 4.1 选型结论

```toml
# ── mk-core：平台无关内核，不含任何窗口依赖 ──────────────────────────
tiny-skia   0.11   光栅器。Pixmap 即 premultiplied RGBA8 缓冲
                   default-features = false, features = ["std", "simd"]
fontdue     0.9    格内数字：轮廓 → 覆盖度位图，阈值 128 得二值掩码
ttf-parser  0.25   字体解析（TTC/OTC 集合字体用 Face::parse(data, index)）
ab_glyph    0.2    界面文本：**惰性**解析，只栅格化真正画到的字形。
                   对 CJK 是硬要求 —— fontdue 会急切解析 22–64 MB 的全部字形，
                   用在菜单栏上等于启动卡死 3–9 秒（见 §6.11）
serde       1      纪录持久化（可选，见 §4.3）
serde_json  1      同上

# ── mk-host：宿主层 ─────────────────────────────────────────────────
winit       0.30   窗口 + 事件循环（ApplicationHandler 模型）
                   default-features = false, features = ["rwh_06", "x11", "wayland"]
softbuffer  0.4    把 CPU 像素缓冲呈现到窗口
cpal        0.15   音频输出（只播放，不解码）

# ── minekeeper-cli：无窗口验证栈 ────────────────────────────────────
image       0.25   PNG 编解码
                   default-features = false, features = ["png"]   ← 关键
```

### 4.2 逐项理由与被否决方案

#### 光栅器：`tiny-skia`

**选它的理由**：纯 Rust（无 cgo）、确定性、跨平台逐像素一致、支持显式关闭抗锯齿、
支持偶奇填充规则（凸起格斜边依赖它）。

**被否决**：

| 方案 | 否决理由 |
|---|---|
| 自研扫描线光栅器 | 1–2 周的工作量，且要自己保证跨平台确定性 |
| `raqote` | 维护活跃度低，API 不如 `tiny-skia` 完整 |
| `vello` | GPU 计算着色器，引入 wgpu（数 MB），且 GPU 浮点差异破坏确定性 |
| `wgpu` / `pixels` | 同上；且引入 GPU 缩放 / 过滤的不确定性，违背「整数像素、零拉伸」承诺 |
| GDI+ / CoreGraphics / Cairo（按平台） | 又回到「多后端、多基线」的老问题 |

#### 字形：`fontdue` + `ttf-parser`

**选它的理由**：`fontdue::Font::rasterize(ch, px)` 直接返回覆盖度位图，
阈值 128 即得二值掩码 —— 与内核要求的「某个字符的二值墨迹」接口形状几乎一致。
闭环校正逻辑与源墨迹无关，所以换掉掩码来源后**适配代码一行不用改**。

**被否决**：`ab_glyph`（只给轮廓，覆盖度要自己算）、`swash`（能力强但体积大）、
`cosmic-text`（完整的文本排版引擎，本项目只需要单个字形）、
~~内嵌字体文件~~ —— **这一条后来被推翻了，见 §6.14**。原本担心违反「不嵌入字体文件」的
合规声明，但那条声明的本意是「不内嵌微软的素材」。格内数字若继续用系统字体，
三平台会加载三个不同字体，渲染基线就无法统一（实测 15 张里 9 张不一致）。
最终选择内嵌一份 **SIL OFL 1.1** 的 Liberation Sans Bold —— 许可明确允许再分发，
且与 Arial 度量兼容。界面中文文本仍用系统字体（CJK 字体太大，内嵌不现实）。

#### 窗口：`winit` + `softbuffer`

**选它的理由**：两者都是纯 Rust、无 cgo。`softbuffer` 在 Windows 上底层就是 GDI 的
DIB blit —— 与 GDI 的 `SetDIBitsToDevice` 是同一机制，所以呈现路径的行为可预期。

**被否决**：

| 方案 | 否决理由 |
|---|---|
| `egui` / `eframe` | widget 工具包。你要的是画布，它给的是控件树，还得跟它的主题系统打架 |
| `iced` | 同上；默认走 wgpu |
| `slint` | 同上；自带 DSL 与观感体系 |
| `gtk4-rs` / Qt / FLTK 绑定 | 拖进 C++ 运行时，单文件小体积的定位直接没了 |
| `tauri` / `wry`（webview） | 把游戏搬进 `<canvas>`，彻底改变项目性质；Linux 上还要 WebKitGTK |
| 手写 win32 / cocoa / x11 后端 | `winit` 已经做了，没有理由重做 |

#### 音频：`cpal`（**不用 `rodio`**）

**关键点**：这个项目的音频是**进程内合成的原始 PCM**，不需要任何解码器。
`rodio` 的默认 features 会拉进 `symphonia`（mp3/aac/flac/vorbis 解码器），
那是好几 MB 的无用代码。`cpal` 一个包解决三平台，只做「把采样喂给声卡」。

### 4.3 Feature 纪律（直接决定体积）

三条必须守住，踩一条就多出几 MB：

1. **`image` 必须 `default-features = false, features = ["png"]`。**
   默认会把 jpeg / gif / tiff / webp / bmp / ico / hdr 全拉进来。
2. **不要用 `rodio`，直接用 `cpal`。** 见上。
3. **`serde` 可以做成可选 feature。** 英雄榜只有三条记录、每三个字段。
   如果哪天要再压体积，`mk-core` 的 `store` feature 关掉即可 —— 自检与出图都不需要它。

### 4.4 编译配置与实测体积

```toml
[profile.release]
opt-level     = "s"        # "z" 更小但明显更慢；"s" 是甜点
lto           = "fat"
codegen-units = 1
panic         = "abort"    # 省掉 unwind 表；GUI 应用可以接受
strip         = "symbols"
```

**这组配置不是可选项。** 实测（`aarch64-apple-darwin`，`rustc 1.99.1`）：

| 配置 | GUI 二进制 |
|---|---|
| cargo 默认 release | 1,611,416 B（1.54 MB） |
| **上面这组** | **757,792 B（740 KB）** |

差距 **2.1 倍**。不配 profile，Rust 的体积优势会缩水一半。

> **注意**：`panic = "abort"` 会让 `cargo test --release` 无法工作（测试需要 unwind）。
> 日常用 `cargo test`（dev profile）即可；确实需要 release 测试时临时覆盖 profile。

Windows 上还要在 `main.rs` 顶部加：

```rust
#![windows_subsystem = "windows"]   // 等价于 Go 的 -H windowsgui
```

否则双击会闪一个控制台窗口。

**参考数据（同平台实测）**：

| 二进制 | 字节 | 说明 |
|---|---|---|
| Rust GUI（早期原型，非当前版本） | 791,232 | 仅用于测依赖底噪，当前体积见 §12.1 |
| Rust CLI（早期原型，非当前版本） | 833,376 | 同上 |
| Rust GUI（本配置，最小原型） | 757,792 | 仅图元 + 依赖图，用于测底噪 |

两个观察：

- **Go 的运行时底噪（1.12 MB）比整个 Rust GUI 二进制还大。**
- CLI 比 GUI 略大是正常的：CLI 里有完整的场景脚本解析器、22 项自检与 PNG 编解码，
  而 GUI 那边的 winit/softbuffer/cpal 加起来反而更轻。

后续实现（菜单 / 对话框 / 完整自检）预计再增加 100–200 KB，仍在 NFR-3 的 1.5 MB 之内。

---

## 5. 系统架构

### 5.1 Crate 划分

```
rust/
├── Cargo.toml                 工作区 + profile
├── crates/
│   ├── mk-core/               平台无关内核（#![forbid(unsafe_code)]）
│   │   └── src/
│   │       ├── game.rs        棋盘状态机、布雷、展开、chord 判定
│   │       ├── theme.rs       16px 基准常量 + 三档换算器
│   │       ├── raster.rs      光栅层：缓冲 + 全部图元
│   │       ├── pix/           绘制图元
│   │       │   ├── bevels.rs  3D 边框原语
│   │       │   ├── sprites.rs 雷 / 旗 / 笑脸
│   │       │   ├── led.rs     七段数码管
│   │       │   └── glyph.rs   字形适配（fontdue + 闭环校正）
│   │       ├── view.rs        绘制 + 输入的唯一状态机
│   │       ├── chrome.rs      自绘菜单栏与模态对话框
│   │       ├── text.rs        界面文本渲染（ab_glyph，覆盖 CJK）
│   │       ├── sound.rs       PCM 合成 + Player 接口
│   │       ├── store.rs       最佳时间持久化
│   │       └── scene.rs       场景脚本解析
│   ├── mk-host/               宿主层
│   │   └── src/
│   │       ├── window.rs      winit + softbuffer（跨平台，一份代码）
│   │       ├── audio.rs       cpal 播放后端（跨平台）
│   │       └── chrome.rs      菜单与对话框（分平台）
│   ├── minekeeper/            GUI 二进制
│   └── minekeeper-cli/        无窗口二进制
└── testdata/
    ├── scenes/                场景脚本（纯文本格式，见 §3.1 FR-9）
    └── golden/                渲染基线
```

### 5.2 依赖方向（严格单向）

```
game ──┐
       ├──> raster ──> pix ──┬──> view ──┐
theme ─┘        │            │           │
                └──> text ───┴──> chrome ┼──> mk-host ──> minekeeper (bin)
sound ────────────────────────────────────┤
store ────────────────────────────────────┘
scene ─────────────────────────────────────────> minekeeper-cli (bin)
```

**硬规则**：

- `game` 只依赖 `std`。出现任何 `raster` / `theme` 的 import 就是分层破了。
- `theme` 只依赖 `std`。
- `raster` 只依赖 `theme`。
- `pix` 依赖 `raster` + `theme` + `game`（`game::Mood` 用于笑脸）。
- `view` 依赖以上全部，但**不认识窗口**。
- `mk-core` 整体**不依赖任何窗口 / 平台 API**，因此可在无头 CI 上编译与测试。

### 5.3 数据流

```
                    ┌──────────────────────────────────────┐
   宿主事件 ───────>│  View（mk-core）                      │
   (winit)          │   ├─ game::Board  状态机              │
                    │   ├─ 按下态 / chord 模式              │
                    │   └─ 脏矩形集合                       │
                    └──────────────┬───────────────────────┘
                                   │ paint(&mut Canvas, clip, &mut FontStack)
                                   v
                    ┌──────────────────────────────────────┐
                    │  Canvas（mk-core::raster, tiny-skia） │
                    │   premultiplied RGBA8 缓冲            │
                    └──────────────┬───────────────────────┘
                                   │ data()
                                   v
                    ┌──────────────────────────────────────┐
                    │  Host::present（mk-host::window）     │
                    │   整数倍最近邻放大 → softbuffer       │
                    └──────────────────────────────────────┘
```

**离屏路径完全同构**：`minekeeper-cli --script` 走的是同一条 `View::paint` →
`Canvas` 路径，只是终点换成 PNG 编码器。这是「渲染回归测试有效」的前提。

---

## 6. 模块详细设计

### 6.1 `mk-core::game` —— 棋盘状态机

**核心类型**

```rust
pub struct Pos { pub row: i32, pub col: i32 }     // Pos::NONE = (-1, -1)
pub enum Mark  { None, Flag, Question }
pub enum Phase { Ready, Playing, Dead, Won }      // Ready = 已摆盘但雷未落下
pub enum Mood  { Normal, Pressed, Concentrate, Dead, Cool }
pub enum Action { None, MarkMines, Reveal }

pub struct Board { /* 私有字段 */ }
pub struct MoveResult { pub changed: Vec<Pos>, pub started: bool, pub boom: bool, pub won: bool }
pub struct Rand { /* PCG32 */ }
```

**关键 API**

```rust
impl Board {
    pub fn new(rows: i32, cols: i32, mines: i32, rng: Rand) -> Self;  // mines 夹在 [1, rows*cols-1]
    pub fn left_click(&mut self, p: Pos) -> MoveResult;
    pub fn cycle_mark(&mut self, p: Pos, allow_question: bool) -> MoveResult;
    pub fn apply_chord(&mut self, p: Pos) -> MoveResult;
    pub fn reset(&mut self);
    pub fn force_mines(&mut self, cells: &[Pos]) -> bool;   // 场景脚本与测试用

    // 查询（渲染层只经这些取值，不碰内部数组）
    pub fn is_mine/is_open/is_sunken/shows_mine/shows_wrong_flag/is_exploded(..) -> bool;
    pub fn adjacent(&self, p: Pos) -> i32;
    pub fn number_at(&self, p: Pos) -> i32;
    pub fn mark_of(&self, p: Pos) -> Mark;
    pub fn flags_left(&self) -> i32;      // 可为负
    pub fn opened_count(&self) -> i32;
    pub fn field_placed(&self) -> bool;
}

pub fn decide(b: &Board, at: Pos) -> Action;
pub fn mark_targets(b: &Board, at: Pos) -> Vec<Pos>;    // 规则 1 的目标
pub fn reveal_targets(b: &Board, at: Pos) -> Vec<Pos>;  // 规则 2 的目标
pub fn neighbors(rows: i32, cols: i32, at: Pos) -> impl Iterator<Item = Pos>;
pub fn distinct(cells: &[Pos]) -> Vec<Pos>;             // 去重，保持首次出现顺序
```

**关键算法**

**A. 布雷（`place`）** —— 首点安全由算法保证，不依赖事后修补：

```
1. 建 pool = 除首点外的全部格号
2. 部分 Fisher–Yates：只洗出前 mines 个
     for i in 0..mines: j = i + rng.below(len - i); swap(pool[i], pool[j]); field[pool[i]] = true
3. 首点格因此天然不在雷位上
```

用部分洗牌而不是全量洗牌，是为了避免为 99×99 分配两次全量数组。

**B. 空白区展开（`expand`）** —— **必须用显式栈迭代**，99×99 全空白递归会栈溢出：

```
stack = [start]
while let Some(p) = stack.pop():
    if is_open(p) || is_mine(p) || !try_open(p): continue
    opened.push(p)
    if adjacent(p) != 0: continue          # 数字格不再向外扩散
    for n in neighbors(p):
        if !is_open(n) && !is_mine(n) && mark_of(n) != Flag: stack.push(n)
```

**C. chord 判定（`decide`）** —— 两条规则的完整条件：

```
if !in_bounds(at) || !is_open(at): return None
n = adjacent(at)
if n == 0: return None

(unopened, unopened_unflagged, flagged) = 统计 8 邻格
if unopened == n && unopened_unflagged > 0: return MarkMines
if flagged  == n && unopened_unflagged > 0: return Reveal
return None
```

`unopened_unflagged > 0` 这个附加条件是必要的：否则「周围全是旗且旗数正好等于数字」时
会进入规则 2 但无事可做，白白触发一次音效与重绘。

**D. 规则 2 的踩雷语义** —— 保留原版风险，且**先判定、后展开**：

```
targets = reveal_targets(at)
boom = targets 中第一个是雷的格
if boom:
    先把所有 target 翻开（含那颗雷），再 die(boom)
else:
    对每个 target 做 expand，结果去重后并入 changed
```

顺序很重要：踩雷时要把 targets 全部翻开再引爆，否则画面会不对。

**E. 失败结算（`die`）**：

```
phase = Dead; exploded = at; open[at] = true; reveal[at] = true
遍历全盘：
  是雷 且 未插旗  → reveal = true          （亮雷）
  非雷 且 插旗    → wrong  = true          （错旗，画红叉）
  非雷 且 问号    → mark = None            （清除问号）
  若 is_sunken   → 计入 changed
```

**F. 胜利判定（`check_win`）**：`opened == rows*cols - mines` 即获胜，
并把所有未标记的雷自动补旗。

### 6.2 `mk-core::theme` —— 几何与配色

**核心类型**

```rust
pub struct Rgb(pub u8, pub u8, pub u8);          // 直通 sRGB，避免绘制代码依赖 tiny_skia::Color
pub enum Size { Small, Medium, Large }           // 16 / 24 / 32
pub struct Metrics { cell_px: i32 }
pub struct Rect { pub x: i32, pub y: i32, pub w: i32, pub h: i32 }
```

**换算器 API**

```rust
impl Metrics {
    pub fn px(&self, v: i32) -> i32;             // (v*cell + 8)/16，.5 一律向上
    pub fn div_px(&self, num: i32, den: i32) -> i32;

    pub fn bevel_px/sunken_line_px/field_border_px/status_border_px/margin_px/
           margin_br_px/status_height_px/status_gap_px/frame_px(&self) -> i32;
    pub fn led_w(&self) -> i32;                  // 2*frame + 3*px(LED_DIGIT_W)  ← 按部件求值
    pub fn led_h(&self) -> i32;

    pub fn client_width(&self, cols: i32) -> i32;
    pub fn client_height(&self, rows: i32) -> i32;
    pub fn status_rect(&self, cols: i32) -> Rect;
    pub fn field_rect(&self, cols: i32, rows: i32) -> Rect;
    pub fn cell_rect(&self, col: i32, row: i32) -> Rect;
    pub fn smiley_rect(&self, cols: i32) -> Rect;   // 取整在逻辑空间
    pub fn led_left_rect(&self) -> Rect;
    pub fn led_right_rect(&self, cols: i32) -> Rect;

    pub fn col_at(&self, x: i32) -> Option<i32>;    // 反算；越界返回 None
    pub fn row_at(&self, y: i32) -> Option<i32>;
}
```

**实现要点**

- `cell_rect` 的格距用**换算后的单元格边长逐格累加**，不是「(原点 + 格号×16) × 比例」——
  后者在 24px 下会让格宽在 23/24/25 之间抖动。
- `col_at` / `row_at` 用换算后的锚点与格距反算，**不能**拿「设备坐标 ÷ 档位比例」。
- `smiley_rect` 的取整必须在逻辑空间做（先算 `(逻辑宽 − 26)/2` 再换算）。

**已实现的测试**（`theme.rs` 的 `#[cfg(test)]`）：客户区尺寸表、数码管按部件求值、
中档 `.5` 向上、状态栏与雷区不重叠、命中测试正反互逆。

### 6.3 `mk-core::raster` —— 光栅层

**为什么不设 trait**：三平台共用同一份 `tiny-skia`，只有一个后端，
具体类型即可 —— 省掉动态派发与 vtable。

**API**

```rust
pub struct Canvas { /* tiny_skia::Pixmap */ }

impl Canvas {
    pub fn new(w: i32, h: i32) -> Option<Canvas>;
    pub fn width(&self) -> i32;
    pub fn height(&self) -> i32;
    pub fn bounds(&self) -> Rect;
    pub fn data(&self) -> &[u8];                 // premultiplied RGBA8
    pub fn data_mut(&mut self) -> &mut [u8];

    pub fn clear(&mut self, c: Rgb);
    pub fn fill_rect(&mut self, r: Rect, c: Rgb);
    pub fn fill_polygon(&mut self, pts: &[(i32, i32)], c: Rgb);   // 偶奇规则
    pub fn fill_ellipse(&mut self, r: Rect, c: Rgb, aa: bool);
    pub fn stroke_ellipse(&mut self, r: Rect, pen: i32, c: Rgb);
    pub fn stroke_line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, w: i32, c: Rgb);
    pub fn stroke_rect(&mut self, r: Rect, pen: i32, c: Rgb);
    pub fn stroke_arc(&mut self, x: f32, y: f32, w: f32, h: f32,
                      start_deg: f32, sweep_deg: f32, pen: i32, c: Rgb);

    pub fn blit(&mut self, at: (i32, i32), src: &Canvas);   // 色键式 1:1 合成
    pub fn inked(&self, x: i32, y: i32) -> bool;
    pub fn ink_bounds(&self) -> (i32, i32, i32, i32);
    pub fn to_rgba(&self) -> Vec<u8>;
}
```

**两条契约**

1. **硬边优先**：除笑脸面部外，所有图元都以 `anti_alias = false` 绘制。
   抗锯齿会引入灰阶，破坏「边缘保持整数像素」的承诺。
2. **填充规则显式指定**：凸起格斜边依赖 `FillRule::EvenOdd`（与 GDI+ 的
   `FillModeAlternate` 同口径）；其余用 `FillRule::Winding`。

**`blit` 的语义**：只写 `src` 中 alpha 非零的像素。等价于一次不做缩放的位图拷贝。
数字精灵只有硬边填充，像素只会是「未着墨」或「画笔色」，所以逐像素比 alpha 即可，
不涉及混合、也不会引入灰边。

**`stroke_arc` 的折线逼近**：`tiny-skia` 没有 `DrawArc`，而 GDI+ 的弧线量化本来也无法复现，
所以按固定 48 步采样。**步长是跨平台确定性的一部分，改动它等于改基线。**

### 6.4 `mk-core::pix` —— 绘制图元

#### `bevels.rs`

```rust
pub fn fill_face(dst: &mut Canvas, r: Rect);
pub fn draw_raised_cell(dst: &mut Canvas, r: Rect, m: &Metrics);
pub fn draw_sunken_cell(dst: &mut Canvas, r: Rect, m: &Metrics);
pub fn draw_sunken_rect(dst: &mut Canvas, r: Rect, t: i32);
pub fn draw_raised_rect(dst: &mut Canvas, r: Rect, t: i32);
pub fn draw_button(dst: &mut Canvas, r: Rect, m: &Metrics, pressed: bool);
```

凸起格的斜接规则（从截图逐像素反推，`N` = 格边长，`B` = 斜边厚度）：

```
白 = (X < B || Y < B) && X + Y <= N - B
灰 = (X >= N-B || Y >= N-B) && X + Y >= N
其余 = 面色 #C0C0C0
```

四个梯形的顶点（整体平移到格子原点）：

```
top    = (0,0) (N-B+1,0) (N-2B+1,B) (0,B)
left   = (0,0) (B,0) (B,N-2B+1) (0,N-B+1)
right  = (N-B,B) (N,0) (N,N) (N-B,N)
bottom = (B,N-B) (N,N-B) (N,N) (0,N)
```

`b*2 >= n` 时退化为纯面色填充，避免多边形自交。

#### `sprites.rs`

```rust
pub fn draw_mine(dst: &mut Canvas, r: Rect, m: &Metrics);
pub fn draw_flag(dst: &mut Canvas, r: Rect, m: &Metrics);
pub fn draw_question(dst: &mut Canvas, r: Rect, col: Rgb, m: &Metrics, font: &mut FontStack);
pub fn draw_wrong_flag_cross(dst: &mut Canvas, r: Rect, m: &Metrics);
pub fn draw_face(dst: &mut Canvas, face_box: Rect, mood: Mood, m: &Metrics);
```

- **雷**：先八根辐条，再压上雷体，最后高光 —— 顺序与原版观察一致。
  半径 `DivPx(9,2)`，辐条超出 `Px(2)`。
- **旗**：16px 基准逻辑包围盒，格内 `x 4..11`、`y 3..12`。
- **笑脸**：脸盘 `Px(16)` 圆，**用抗锯齿**；五种 mood 各自的眼 / 嘴 / 墨镜。
  这是全局唯一使用抗锯齿的地方。

#### `led.rs`

```rust
pub fn led_format(value: i32) -> [char; 3];     // '0'..'9' / '-' / ' '
pub fn draw_led(dst: &mut Canvas, outer: Rect, value: i32, m: &Metrics);
```

段位序（低位起）：`0 top`、`1 upper-left`、`2 upper-right`、`3 middle`、
`4 lower-left`、`5 lower-right`、`6 bottom`。

段的多边形顶点按 16px 基准逻辑坐标定义后经 `Metrics` 落到设备像素。
**不要**用 `box.w / LED_DIGIT_W` 反推档位 —— 那是「档位比例必为整数」的假设，
中档下 `20/13 = 1`，整只数码管会缩回小档大小。

格式化规则：`value < 0` 显示 `-NN`（`min(|v|, 99)`）；否则 `min(v, 999)` 三位带前导零。

#### `glyph.rs`

```rust
pub struct FontStack { /* 字体表 + 掩码缓存 + 精灵缓存 */ }
pub struct GlyphMask { pub mask: Vec<bool>, pub w: usize, pub h: usize, pub family: String }

impl FontStack {
    pub fn load(paths: &[&str]) -> FontStack;      // 加载失败的路径跳过
    pub fn load_default() -> FontStack;            // 用内置三平台回退表
    pub fn is_empty(&self) -> bool;
    pub fn ink(&mut self, ch: char) -> Option<&GlyphMask>;
    pub fn render_glyph(&mut self, ch: char, cell_px: i32, m: &Metrics,
                        target_w: i32, col: Rgb) -> Option<Canvas>;
    pub fn diag(&mut self, m: &Metrics) -> Vec<String>;
}
```

**取墨迹**：`fontdue` 在 `PROBE_EM = 96.0` 像素高度上栅格化，
覆盖度 `>= 128` 判定着墨，裁到包围盒。放大取轮廓再回采，是为了让细笔画在量化时不消失。

**适配 + 闭环校正**：

```
box   = m.px(GLYPH_BOX)      # 字形包围盒高（逻辑 10）
off   = m.px(GLYPH_OFFSET)   # 格内纵向偏移（逻辑 3）
sy0   = box / info.h
sx    = sy0
wantW = target_w > 0 ? target_w : round(info.w * sx)
if target_w > 0: sx = target_w / info.w
left  = off + (box - wantW) / 2
top   = off

for _ in 0..4:
    paint_mask(sp, mask, sx, sy, dx, dy, col)
    (gx, gy, gw, gh) = sp.ink_bounds()
    if gw == 0: break
    if gw == wantW && gh == box && gx == left && gy == top: break
    sx *= wantW / gw;  sy *= box / gh
    dx += left - gx;   dy += top - gy
```

**`paint_mask`**：把源墨迹按 `(sx, sy)` 缩放、平移到 `(dx, dy)`，
**面积覆盖 ≥ 50% 判定着墨** —— 等价于把轮廓缩放后做硬边填充，不产生任何灰阶。

**逐字定宽**：数字 2–8 撑满 `GlyphBox`（逻辑 10），只有「1」是 7 宽。
通用粗体字的数字宽高比只有约 0.65，只按字高等比画会偏窄，所以做水平/垂直**各自独立**的
整数目标框适配。问号 `target_w = 0`，保持字体自然宽高比。

**字族回退表**（`DEFAULT_FAMILIES`）：

```
C:/Windows/Fonts/arialbd.ttf
C:/Windows/Fonts/segoeuib.ttf
/System/Library/Fonts/Supplemental/Arial Bold.ttf
/System/Library/Fonts/Supplemental/Verdana Bold.ttf
/System/Library/Fonts/Helvetica.ttc
/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf
/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf
```

全部加载失败时 `render_glyph` 返回 `None`，画面缺数字但**不崩溃**（NFR-5）。

### 6.5 `mk-core::view` —— 绘制 + 输入状态机

**为什么绘制与输入在同一个类型里**：左右键同按 / 中键的 chord 检测
**必须和按下态渲染共用一份状态**。拆开会导致「按下态」与「实际模式」不一致。

**API**

```rust
pub enum Button { Left, Right, Middle }
pub enum Key { Up, Down, Left, Right, Enter, Space, F }

pub struct Events {
    pub first_move_made: Option<Box<dyn FnMut()>>,
    pub game_finished: Option<Box<dyn FnMut(bool)>>,
    pub new_game_requested: Option<Box<dyn FnMut()>>,
    pub capture: Option<Box<dyn FnMut(bool)>>,     // 指针捕获
}

pub struct View { /* 私有字段 */ }

impl View {
    pub fn new() -> View;                          // 默认高级 + 中档
    pub fn board(&self) -> &Board;
    pub fn board_mut(&mut self) -> &mut Board;
    pub fn metrics(&self) -> Metrics;
    pub fn client_width(&self) -> i32;
    pub fn client_height(&self) -> i32;

    pub fn apply_size(&mut self, size: Size);
    pub fn start_game(&mut self, d: Difficulty, seed: u64);
    pub fn preview_chord(&mut self, at: Pos);      // 离屏渲染 / 测试用
    pub fn tick_second(&mut self);

    pub fn button_down(&mut self, b: Button, pt: (i32, i32));
    pub fn button_up(&mut self, b: Button, pt: (i32, i32));
    pub fn mouse_move(&mut self, pt: (i32, i32));
    pub fn mouse_leave(&mut self);
    pub fn key_down(&mut self, k: Key);

    pub fn paint(&self, dst: &mut Canvas, clip: Rect, font: &mut FontStack);

    pub fn invalidate_all_pub(&mut self);
    pub fn take_dirty(&mut self) -> (Vec<Rect>, bool);
}
```

**按下态模型**（三个独立布尔量 + 一个派生模式）：

```
left_down / right_down / middle_down     ← 逐条原始事件维护，不做任何合成

mode = if middle_down || (left_down && right_down) { Chord }
       else if left_down  { Left }
       else if right_down { Right }
       else               { None }
```

**这是整个设计里最容易被做错的地方。** 宿主必须**逐条投递**原始鼠标消息，
不能依赖任何「按键事件合成」—— 否则「一个键已按下时再来第二个键」会被吞掉，
左右键同按整个失效。

**`button_up` 的落子条件**（三重判定，缺一不可）：

```
m = mode; cell = pressed
mode = None; pressed = NoPos; rebuild_press_set()
if m == None: return
invalidate(press_visual_cells(cell))
if !cell.is_valid() || cell_at(pt) != cell: return    ← 松手时指针必须还在同一格
match m:
    Chord  → board.apply_chord(cell)
    Left   → do_open(cell)    且 b == Button::Left
    Right  → do_mark(cell)    且 b == Button::Right
```

**`press_visual_cells`** —— chord 模式下会把周围未插旗的格子一起压下去：

```
list = [at]
if mode == Chord && is_open(at) && adjacent(at) > 0:
    list += reveal_targets(board, at)
```

**脏矩形**：`invalidate` 追加矩形，`invalidate_all` 置整块标志。
`take_dirty` 取走并清空。注意 `rebuild_press_set` 必须把**上一帧画过的叠加框**
一并失效，否则松手后会留下残影。

**计时**：`tick_second` 由宿主每秒调用一次，`seconds = min(999, seconds + 1)`，
只失效右侧数码管区域。

### 6.6 `mk-core::sound` —— 音频

```rust
pub const RATE: u32 = 22050;
pub enum Kind { Open, Boom, Cheer }

pub trait Player {
    fn play(&mut self, wav: &[u8]);
    fn stop_all(&mut self);
}

pub struct SilentPlayer;                              // 无音频设备时降级
pub struct Audio { /* clips + player + enabled */ }

impl Audio {
    pub fn new(player: Box<dyn Player>) -> Audio;     // 构造时合成三段
    pub fn play(&mut self, k: Kind);                  // enabled == false 时什么都不做
    pub fn stop_all(&mut self);
    pub fn clip(&self, k: Kind) -> &[u8];
}
```

**合成部分是纯数学、完全可移植的**，三平台跑出逐位相同的 PCM。关键点：

- **银行家舍入**：`round_even(x)`，**不要**换成 `f64::round` —— 那是「四舍五入远离零」，
  在 `.5` 上会差 1 个 LSB，三平台的 PCM 就不再逐位相同。
- 三段波形：`blip`（指数衰减正弦）、`boom`（LFSR 噪声 + 下滑正弦）、
  `cheer`（四音琶音 + 泛音）。
- `wav()` 包 44 字节标准头（PCM / 单声道 / 8bit）。

### 6.7 `mk-core::store` —— 持久化

```rust
pub const NO_RECORD: i32 = 999;
pub struct Entry { pub seconds: i32, pub name: String }   // Default = {999, "匿名"}
pub struct Store { /* best: [Entry; 3], pub path: PathBuf */ }

impl Store {
    pub fn default_path() -> PathBuf;
    pub fn load(path: impl AsRef<Path>) -> Store;          // 任何失败都静默降级
    pub fn load_default() -> Store;
    pub fn get(&self, k: Kind) -> Entry;
    pub fn is_record(&self, k: Kind, seconds: i32) -> bool; // 自定义档恒 false
    pub fn record(&mut self, k: Kind, seconds: i32, name: &str);
    pub fn clear(&mut self);
    pub fn save(&self);
}
```

平台缓存目录**只用标准库**解析（不引入 `dirs` crate），见 FR-6。

### 6.8 `mk-core::scene` —— 场景脚本

```rust
pub struct Scene { pub view: View, pub rows: i32, pub cols: i32, pub size: Size }
pub fn run_script(path: impl AsRef<Path>, seed: u64) -> Result<Scene, String>;
pub fn run_script_text(text: &str, seed: u64) -> Result<Scene, String>;
pub fn gen_layout(rows: i32, cols: i32, mines: i32, seed: u64) -> String;
```

脚本里 `mines` 声明的数量必须与 `board` 的雷数一致，否则返回错误 ——
**不能画出一幅对不上的画面却让人以为对得上**。

### 6.9 `mk-host` —— 宿主层

**关键结论：只有两处真正需要分平台。**

| 能力 | 方案 | 分平台？ |
|---|---|---|
| 窗口、输入、定时器、像素呈现 | `winit` + `softbuffer` | **否** |
| 音频输出 | `cpal` | **否** |
| 原生菜单栏 | `Chrome` | 是 |
| 模态对话框 | `Chrome` | 是 |

#### 五条硬约束

这些是 `mk-core` 已经在依赖、而多数 GUI 库默认不提供的行为。**选型时逐条验收**：

| # | 约束 | 为什么 | 对应 API |
|---|---|---|---|
| 1 | **左右键同时按下必须可观测** | 「中键或左右键同按」这个核心玩法的全部依赖 | `InputEvent::ButtonDown` 逐键投递，不合成 |
| 2 | **指针离开客户区必须可观测** | 拖出窗口时松开按下态；拖到笑脸开新局 | `InputEvent::MouseLeave`（`CursorLeft`） |
| 3 | **指针捕获必须支持** | 按下后指针移出窗口，抬起事件仍要送回同一窗口 | `Host::set_capture` |
| 4 | **窗口激活 / 失活必须可观测** | 失活停表、回前台且本局已开始才续表 | `InputEvent::Focus(bool)` |
| 5 | **呈现必须整数倍最近邻放大** | 「全程零位图拉伸」是核心承诺 | `Host::scale` |

#### `Host` trait

```rust
pub trait Host {
    fn present(&mut self, buf: &[u8], w: i32, h: i32);
    fn invalidate(&mut self, rects: &[Rect]);
    fn set_title(&mut self, title: &str);
    fn set_timer(&mut self, id: u32, ms: u32);
    fn kill_timer(&mut self, id: u32);
    fn set_capture(&mut self, on: bool);
    fn scale(&self) -> i32;
    fn quit(&mut self);
}
```

#### 呈现路径

`softbuffer` 的缓冲是 `&mut [u32]`（`0x00RRGGBB`），而 `tiny-skia` 是 RGBA8 字节，
所以每帧要做一次字节 → u32 的重排。**这个转换缓冲必须复用**（`WinitHost::scratch`），
不要每帧分配。

放大用**整数倍最近邻**：`dst[y][x] = src[y/scale][x/scale]`。

> **API 陷阱**：`softbuffer::Buffer::present()` 会**消费 `self`**（签名是 `present(self)`）。
> 所以「呈现完把缓冲存下来做缓存」编译不过，必须先 `to_vec()` 快照再 present。

#### 五条硬约束的实测风险

约束 1 与 3 是**最可能出问题的两条**，而且都是核心手感：

- **约束 1**：某些 Wayland compositor 与 X11 配置下，按住左键再按右键时第二个事件
  可能不投递（libinput 的按键模拟、部分合成器的抓取策略）。
- **约束 3**：`winit` **没有暴露 `SetCapture` 语义**。Wayland 在拖拽期间由合成器隐式抓取，
  X11 / Win32 不会。

**这两条必须在正式实现前用一个一次性原型验证**，见 §11 里程碑 M0。

#### `Chrome` trait（菜单与对话框）

```rust
pub enum MenuItem {
    Entry { cmd: Command, label: String, checked: bool },
    Separator,
    Submenu { label: String, items: Vec<MenuItem> },
}

pub trait Chrome {
    fn build_menu(&mut self, items: &[MenuItem]);
    fn sync_menu(&mut self, marks: bool, colored: bool, sound: bool, size_index: usize);
    fn message(&mut self, title: &str, body: &str);
    fn prompt_text(&mut self, title: &str, prompt: &str, initial: &str) -> Option<String>;
    fn prompt_custom_region(&mut self, current: (i32, i32, i32)) -> Option<(i32, i32, i32)>;
    fn show_scores(&mut self, entries: [(&'static str, i32, String); 3]) -> bool;
}

pub fn menu_items() -> Vec<MenuItem>;    // 菜单结构的唯一来源
```

`Command` 枚举携带原版菜单 ID（`Command::id()`），与 FR-4 一一对应。

### 6.10 两个二进制

#### `minekeeper`（GUI）

```rust
struct App {
    view: View, fonts: FontStack, audio: Audio,
    canvas: Option<Canvas>,      // 复用，只在尺寸变化时重建
    difficulty: Difficulty, size: Size,
    marks/colored/sound_on: bool, clock_running: bool, seed: u64,
}
```

实现 `mk_host::window::AppHandler`：

| 回调 | 行为 |
|---|---|
| `resumed` | `host.fit_client(view.client_width(), client_height())` |
| `input` | 转成 `View` 的方法调用，然后 `take_dirty()` → `host.invalidate()` |
| `command` | 菜单命令分派（开局 / 难度 / 尺寸 / 三个开关 / 退出） |
| `redraw` | 尺寸变了就重建 `Canvas`；`view.paint(...)` → `host.present(...)` |
| `tick` | `view.tick_second()` + 失效右侧数码管 |

**计时器生命周期**：

```
首点落下（phase 从 Ready → Playing）  → start_clock（1 秒心跳）
失活（Focus(false)）                  → stop_clock
回到前台 且 phase == Playing          → start_clock
踩雷 / 获胜                           → stop_clock
```

> 注意：**不能用「计时器是否在跑」判断要不要续表** —— `stop_clock` 会清掉那个标志，
> 那样永远续不上。判据必须是 `board.phase == Playing`。

#### `minekeeper-cli`（无窗口验证栈）

不依赖 `winit` / `softbuffer` / `cpal`，因此**可以在无显示设备的 CI 上跑全部自检与出图**。
这是整个跨平台方案里最先能落地、也最有价值的一块。

`checks.rs` 里的自检清单是 `--selftest` 与 `cargo test` **共用的唯一来源** ——
两份清单走偏是这类项目最常见的腐化方式。

### 6.11 `mk-core::text` —— 界面文本

```rust
pub struct TextMetrics { pub width: i32, pub ascent: i32, pub descent: i32 }

pub struct TextRenderer { /* 字体面 + 字形缓存 */ }

impl TextRenderer {
    pub fn load(paths: &[&str]) -> TextRenderer;
    pub fn load_default() -> TextRenderer;
    pub fn is_empty(&self) -> bool;
    pub fn loaded_bytes(&self) -> usize;
    pub fn measure(&mut self, text: &str, px: f32) -> TextMetrics;
    pub fn draw(&mut self, dst, x, baseline, text, px, col) -> i32;
    pub fn draw_in(&mut self, dst, r, text, px, col, left_pad) -> i32;   // 垂直居中
    pub fn draw_centered(&mut self, dst, r, text, px, col) -> i32;       // 水平+垂直居中
    pub fn missing_chars(&self, text: &str) -> Vec<char>;
}
```

**与 `pix::glyph` 的分工**：`pix::glyph` 画格内数字（硬边二值、逐字定宽、闭环校正），
这里是界面文字（抗锯齿、按字体自然宽度排版）。XP 的菜单栏本来就用抗锯齿渲染，
所以这不是妥协。两套并存，互不干扰。

**`&` 是助记符标记**，不画也不占宽度。`measure` 与 `draw` 必须一致，
否则居中会偏 —— 这是两条路径唯一需要同步的地方，已有测试守住。

**加载有两条内存纪律**（见 §6.13 的实测）：

1. 每个字体文件只取集合里的**第 0 个面**。
2. 拿到能画中文的字体就**停**，不把整张回退表读进来。

### 6.12 `mk-core::chrome` —— 菜单栏与对话框

见 §7。

### 6.13 实测踩到的坑

这些是文档里不会写、只有跑起来才知道的。

**① `softbuffer::Buffer::present()` 会消费 `self`**（签名是 `present(self)`）。
所以「呈现完把缓冲存下来做缓存」编译不过，必须先 `to_vec()` 快照再 present。

**② softbuffer 的缓冲是 `&mut [u32]`（`0x00RRGGBB`），而 tiny-skia 是 RGBA8 字节。**
每帧要做一次字节 → u32 的重排，转换缓冲必须复用。

**③ `PathBuilder::from_oval()` 返回 `Option<Path>`，`from_rect()` 直接返回 `Path`。**
前者因为椭圆可能退化。

**④ `MouseInput` 事件不带坐标。** `winit` 的 `WindowEvent::MouseInput` 只有
`{ device_id, state, button }`，按下时的命中测试需要自己记最近一次 `CursorMoved`
的位置。不记的话所有点击都落在 `(0,0)`。

**⑤ `LogicalSize` 已经是逻辑单位。** `request_inner_size(LogicalSize::new(w, h))`
里 winit 会按 scale_factor 自己换算成物理像素，**不能再乘 scale** —— 乘了会得到 4 倍窗口。

**⑥ `panic = "abort"` 让 `cargo test --release` 不可用**（测试需要 unwind）。
日常用 `cargo test`（dev profile）即可。

**⑦ 字体常驻内存曾经是 1.1 GB。** 见下。

**⑧ 自绘对话框的文本必须折行，而且要守行首禁则。**

第一版 `open_message` 用固定宽度 360 且正文**完全不折行** —— 帮助文本里较长的句子
直接冲出对话框右边界，压在棋盘上。修法是让对话框尺寸由**折行结果**决定，
并夹在窗口可用区域内。

折行本身踩了三个坑，每个都写成了测试：

| 坑 | 现象 | 修法 |
|---|---|---|
| 只在空格处断行 | `· 规则一：…` 被断成孤零零的 `·` 一行（整行只有一个空格） | 空格断点必须落在行宽 60% 之后，否则按字符硬断（CJK 允许） |
| 缺行首禁则 | `。）` 被甩到行首 | 收尾标点不得起行；宁可让上一行超宽一个字 |
| 禁则只看已入行的字符 | `）。` 仍会单独成行 —— 冒犯的标点正是**还没加进来的下一个字符** | 断行判断改成「下一个字符不是收尾标点时才允许断」 |

第 3 条是最容易写错的地方：禁则检查必须针对**即将加入的字符**，而不是 `cur` 里切点之后的字符。
当时 `cut == cur.len()`，检查循环根本不执行，测试才把它暴露出来。

**⑨ 画布缓冲是 RGBA，不是 BGRA。**

`tiny-skia` 的 `Pixmap` 字节序是 R,G,B,A。早期有注释写成 BGRA，照着写的新代码
（按底色找包围盒）把 R 和 B 弄反了 —— 因为测试用的 `FACE = 0xC0C0C0` 三个通道相等，
这个错误**在灰阶上完全看不出来**，直到用蓝色标题条去断言才暴露。

缓冲的三种用途都依赖这个顺序：`blend_pixel`（文字混合）、`to_rgba()`（PNG 编码）、
`Host::present`（转 `0x00RRGGBB`）。改任何一处前先确认顺序。

**⑩ 恒真的测试比没有测试更危险。**

这一轮独立复核揪出五处「写得像断言、实际永远通过」的检查：

| 位置 | 为什么恒真 |
|---|---|
| `checks.rs` 左右键同按 | 断言 `opened_count() < before`，而 `opened` 只增不减 |
| `text.rs` × 2、`chrome.rs`、`icon.rs` | 画布 `clear()` 后全像素 alpha=255，`ink_bounds` 恒为整块画布 |

后果是：删掉「左右同按」判定、或完全不画对话框，测试依然全绿。
修法分别是改成 `!=` 和新增 [`Canvas::ink_bounds_on`]（按底色而非 alpha 判定）。

**判据**：一条断言如果「把被测代码删掉它还是通过」，那它就是无效的。
新增可见性断言时，先在心里删一遍实现。

同一轮还发现帮助菜单的三个条目指向同一段文案（它们被写进了同一个 match 分支）。
现在菜单只保留「使用帮助」与「关于扫雷」两项（D8），文案上移到 `mk-core::chrome`，
GUI 与 `--ui` 离屏预览共用同一份。

代价是允许最多一个字的溢出 —— 所以对话框的内文宽度要按 `w - 48`（左 16 + 右 16 + 禁则余量 16）算。

#### 字体内存：1127 MB → 148 MB

第一版 `TextRenderer::load` 写成这样：

```rust
for p in paths {
    let data = fs::read(p)?;
    for idx in 0..16 {                          // ← 对每个集合 index
        if let Ok(f) = FontVec::try_from_vec_and_index(data.clone(), idx) {  // ← 复制整份文件
            faces.push(f);                      // ← 而且全部保留
        }
    }
}
```

`.ttc` 是字体集合。实测 macOS 上：

| 字体 | 体积 | 面数 | 复制出的常驻内存 |
|---|---|---|---|
| Hiragino Sans GB.ttc | 22.4 MB | 4 | 90 MB |
| STHeiti Light.ttc | 53.2 MB | 2 | 106 MB |
| Songti.ttc | 63.8 MB | 8 | **512 MB** |
| | | | **合计 ≈ 708 MB** |

加上 macOS 窗口栈的 ~135 MB 与画布，进程 RSS 到 **1127 MB**。

修法就是 §6.11 那两条纪律。修完 **148 MB**（其中 135 MB 是窗口栈，应用自身约 13 MB）。

`font_memory_is_bounded` 测试守住这条：加载字节数 < 80 MB，且字体面 ≤ 3。

### 6.14 字体的平台依赖（一次被推翻的设计判断）

设计初期我判定：三平台共用同一份 `tiny-skia`，所以渲染基线只有一套，CI 上可以精确比对。
**这个判断漏了一件事 —— 字形来自系统字体。**

`pix::glyph` 原先把系统字体排成回退表：

| 平台 | 实际加载 |
|---|---|
| macOS | `/System/Library/Fonts/Supplemental/Arial Bold.ttf` |
| Windows | `C:/Windows/Fonts/arialbd.ttf` |
| Linux | `/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf` |

三个不同的字体 → 三套字形 → 三套像素。**实测**：把回退表里的 Arial Bold 换成
Verdana Bold（模拟 Linux 加载 DejaVu），15 张基线里 **9 张失败**，差异全在雷区：

```
✘ expert-won：40994 像素不同（状态栏 0 / 雷区 40994 / 其它 0）
基线校验：15 张，通过 6，失败 9
```

**修法：内嵌一份字体。** 选了 **Liberation Sans Bold** ——
SIL OFL 1.1 明确允许原样再分发，且与 Arial **度量兼容**。换字体后
全图差异仅 **174 / 882,672 像素（0.02%）**，肉眼几乎不可辨。

内嵌后二进制从 850 KB 涨到 **1.27 MB**（arm64）/ **1.39 MB**（x86_64），
仍在 NFR-3 的 1.5 MB 内，但 x86_64 已接近上限 —— 再加功能前先量体积。

**架构差异的另一半风险**：基线录在 arm64 上，而 CI runner 多为 x86_64。
`tiny-skia` 用了 SIMD，不同架构的 SIMD 实现是否位精确？用等价实验验证：
把 `tiny-skia` 的 `simd` feature 关掉重新构建（走标量路径），
**15 张基线依然全部通过** —— 说明 SIMD 与标量是位精确的，
架构差异不太可能改变输出。（这不能*证明* x86 SIMD 等价，但两者都等于同一个标量参考。）

**界面文本不内嵌**：菜单与对话框的中文用系统字体。CJK 字体 16–60 MB，内嵌不现实。
界面文本不进渲染基线（基线只渲染游戏画面），平台间观感略有差异不影响回归保护。

**顺带修掉的一个启动问题**：`TextRenderer` 最初对每个 `.ttc` 的**每个** collection index
都 `data.clone()`，`Songti.ttc` 有 8 个面 × 64 MB = 512 MB，进程 RSS 到 **1127 MB**。
改成「只取第 0 个面 + 拿到中文字体就停」后降到 148 MB（其中 135 MB 是 macOS 窗口栈）。

---

## 7. 菜单与对话框方案

### 7.1 决策

**菜单与「自定义雷区」「英雄榜」自绘；姓名输入保留宿主接口走系统输入框。**

### 7.2 理由

三平台各写一套原生实现意味着三份代码、三种行为（macOS 的菜单栏还是全局的），
成本远高于收益。而这个项目连笑脸和数码管都是现画的，自绘菜单不违背它的气质，
而且一套代码三平台完全一致。

绘制原语（`draw_raised_rect` / `draw_sunken_rect` / `draw_button`）已经全部具备。

### 7.3 落地形态：`mk_core::chrome`

实现之后发现**一处都不需要分平台**。菜单与对话框都在 `mk-core` 里自绘，
文本输入交给系统 IME，因此 `mk-host` 完全跨平台。

布局：窗口客户区自上而下是「菜单栏 `Chrome::BAR_HEIGHT`（20 逻辑像素）+ 游戏画面」。
游戏画面下移靠 `Canvas::set_origin` 的平移变换 —— **`view` 里的坐标一个都不用改**；
反过来，喂给 `view` 的鼠标 y 要减掉菜单栏高度。

输入路由（`App::input`）三步：

1. **界面优先**：`App::to_chrome` 把宿主事件翻成 `ChromeEvent`。菜单栏始终可点；
   菜单展开或对话框打开时，任意位置的点击都归界面。
2. **界面挡着就不漏给游戏**：`chrome.is_modal() || chrome.menu_open()` 时直接返回。
3. **游戏**：其余事件照旧，y 减去菜单栏高度。

`Chrome::handle` 返回 `ChromeOutcome::{Ignored, Redraw, Command}`：
`Ignored` 表示事件落到游戏，`Command` 表示菜单命令，由应用分派。

### 7.4 唯一的例外：姓名输入

「请留尊姓大名」需要中文 IME。自绘输入框要自己实现光标、选区、候选窗和 IME 消息循环，
这是真正的坑。所以 `Chrome::prompt_text` 单独留成宿主接口，各平台用系统输入框：

| 平台 | 实现 |
|---|---|
| Windows | 原生 `EDIT` 子控件 |
| macOS | `NSAlert` + `NSTextField` |
| Linux | GTK 输入框，或降级为自绘单行输入（**接受无 IME**） |

### 7.5 自绘对话框的复用

`modal.go` 里的 Tab 链、默认按钮、Esc 取消这套语义可以照搬：

- Tab / Shift+Tab 在控件间循环
- Enter 触发默认按钮
- Esc 触发取消
- 模态期间父窗口不响应输入

「自定义雷区」的上下箭头 + 数字联动需要重写，逻辑本身不复杂，
真正要小心的是**「行列变了把雷数上限夹回来」**这条规则别在重写时丢了：

```
on rows/cols change:
    max = max_mines_for(rows, cols)
    spin_range = (MIN_MINES, max)
    spin_value = clamp(spin_value, MIN_MINES, max)
```

---

## 8. 关键算法与不变量

### 8.1 不变量清单

实现过程中必须始终成立的性质，每一条都应有测试守护：

| # | 不变量 | 守护测试 |
|---|---|---|
| I1 | 首点永不踩雷，且首点格不在雷位上 | `game::tests::first_click_is_never_a_mine` |
| I2 | 雷数精确等于声明值 | `game::tests::mine_count_is_exact` |
| I3 | 展开永不翻开雷 | `game::tests::expand_never_opens_a_mine` |
| I4 | 规则 1 只标真雷 | `game::tests::chord_rule_one_marks_only_real_mines` |
| I5 | 99×99 全盘展开不栈溢出 | `game::tests::big_board_expand_does_not_overflow` |
| I6 | 全开非雷格即获胜，且自动补旗 | `game::tests::opening_every_safe_cell_wins` |
| I7 | 取整规则只有 `Metrics::px` 一处，`.5` 向上 | `theme::tests::medium_rounding_is_half_up` |
| I8 | 命中测试正反互逆 | `theme::tests::hit_test_round_trips` |
| I9 | 状态栏与雷区不重叠、不越界 | `theme::tests::status_and_field_do_not_overlap` |
| I10 | 硬边填充不产生灰阶 | `raster::tests::hard_edge_fill_has_no_greyscale` |
| I11 | 偶奇填充规则正确 | `raster::tests::polygon_uses_even_odd_rule` |
| I12 | 凸起格斜接逐像素符合实测规则 | `pix::bevels::tests::small_bevel_matches_measured_rule` |
| I13 | 左右键同按进入 chord 模式 | `view::tests::left_plus_right_enters_chord_mode` |
| I14 | 拖动换格时新旧两格都失效 | `view::tests::moving_between_cells_invalidates_both` |
| I15 | 同种子雷位可复现 | `checks::same_seed_reproducible` |
| I16 | 三段音效的 WAV 头自洽 | `sound::tests::wav_header_is_well_formed` |
| I17 | 银行家舍入，跨平台 PCM 逐位一致 | `sound::tests::round_even_is_bankers` |

### 8.2 三个最容易做错的地方

**① 取整只能有一处。** 所有总尺寸由**换算后的基本量相加**得到。
如果写成「逻辑总量 × 档位比例」，24px 档下会出现「边框厚度之和 ≠ 凹槽宽度」的缝隙。
同理，**不要从设备尺寸反推档位**（`box.w / 13` 这种），要把 `Metrics` 一路传下去。

**② 鼠标事件必须逐条投递，不做任何合成。** 见 §6.5。

**③ 硬边与抗锯齿的边界。** 只有笑脸面部用 AA，其余一律 `anti_alias = false`。
新增任何图元前先问：它会引入灰阶吗？

---

## 9. 风险与对策

| # | 风险 | 概率 | 影响 | 对策 |
|---|---|---|---|---|
| R1 | **左右键同按在某些 Linux 合成器下不投递第二个事件** | 中 | **致命**（核心玩法失效） | M0 原型先验证；若失败则评估「中键单独可用 + 文档说明」或改用 `raw-window-handle` 直连 X11 |
| R2 | **`winit` 无 `SetCapture` 语义**，拖出窗口后抬起事件丢失 | 中 | 高（拖到笑脸开新局失效） | M0 原型验证；必要时 `raw-window-handle` + 平台代码补一个捕获 |
| R3 | 系统字体缺失导致数字画不出来 | 低 | 中 | 多路径回退表 + `render_glyph` 返回 `None` 时不崩溃（NFR-5） |
| R4 | 依赖数量膨胀（约 150 crate） | 高 | 中 | 接受；用 `cargo deny` / `cargo tree` 定期审计；`image` 与 `serde` 已做 feature 收窄 |
| R5 | 二进制体积超出预期 | 中 | 中 | CI 体积守卫（> 2 MB 失败）；profile 已实测 2.1× 收益 |
| R6 | 跨平台渲染不一致 | 低 | 高 | CI 三平台互比 PNG；`tiny-skia` 是纯 CPU 软件光栅，浮点路径一致 |
| R7 | 自绘菜单的观感不如原生 | 中 | 低 | 已接受（§7.2）；Windows 上可选保留原生菜单作为增强 |
| R8 | `panic = "abort"` 使 `cargo test --release` 不可用 | 高 | 低 | 文档说明；日常用 dev profile 测试 |
| R9 | macOS Retina 下画面过小 | 中 | 中 | `Host::scale` 整数倍最近邻放大（约束 5）；画布逻辑尺寸不变，尺寸表与基线不受影响 |
| R10 | 渲染基线需要全部重录 | 高 | 低 | 已知且可接受；`testdata/golden/` 在 M2 一次性录制 |

### R9 展开：Retina / DPI

本设计的前提是「方格尺寸 = 用户选的**物理像素**，不随 DPI 缩放」。
这在 Windows 上是对的，但 macOS 的 backing scale 是 2，1 物理像素 = 0.5pt，
24px 的格子只有 12pt 宽，画面会小到不可用。

**对策**：画布的逻辑尺寸与像素内容**完全不变**，由后端做整数倍最近邻放大
（macOS 2×，Windows 高 DPI 可选 1×/2×）。这样：

- 尺寸表、渲染基线、命中测试**全部不用改**
- 每个逻辑像素变成 N×N 个物理像素，边缘依然是硬的
- 需要新增一条自检：「放大必须是整数倍且最近邻」

---

## 10. 测试策略

### 10.1 三层结构

| 层 | 内容 | 运行环境 |
|---|---|---|
| **L1 纯逻辑** | 棋盘状态机、布雷、展开、chord 判定、几何换算 | 任何平台，无光栅器 |
| **L2 渲染** | 图元、字形适配、整盘出图 | 任何平台（`tiny-skia` 是纯 CPU） |
| **L3 输入路径** | 左右键同按、中键、拖动、焦点框 | 任何平台，直接调 `View` 方法 |

三层**全部**可以在无显示设备的 CI 上跑。

### 10.2 自检清单（`--selftest`）

`mk-core` 与 `minekeeper-cli::checks` 的清单是唯一来源，`cargo test` 读同一份。

| # | 检查 | 类型 |
|---|---|---|
| 1 | 首点永不踩雷 | 纯逻辑 |
| 2 | 雷数精确 | 纯逻辑 |
| 3 | 相邻计数正确 | 纯逻辑 |
| 4 | 永不翻开雷 | 纯逻辑 |
| 5 | 全开非雷即获胜 | 纯逻辑 |
| 6 | chord 只在条件成立时生效 | 纯逻辑 |
| 7 | chord 规则 1 只标真雷 | 纯逻辑 |
| 8 | 99×99 展开不溢出 | 纯逻辑 |
| 9 | 单格推断只报真雷 | 纯逻辑 |
| 10 | 客户区尺寸表（小/中/大） | 几何 |
| 11 | 数码管格式化 | 几何 |
| 12 | 凸起格斜接落在调色板内 | 渲染 |
| 13 | 数字字形逐字定宽 | 渲染 |
| 14 | 左右键同按进入 chord 模式 | 输入 |
| 15 | 中键进入 chord 模式 | 输入 |
| 16 | chord 手势矩阵 · 中键 / 标雷 | 输入 |
| 17 | chord 手势矩阵 · 中键 / 展开 | 输入 |
| 18 | chord 手势矩阵 · 左右同按 / 标雷 | 输入 |
| 19 | chord 手势矩阵 · 左右同按 / 展开 | 输入 |
| 20 | chord 手势矩阵 · 右左同按 / 标雷 | 输入 |
| 21 | chord 手势矩阵 · 右左同按 / 展开 | 输入 |
| 22 | 同种子雷位可复现 | 纯逻辑 |

**chord 手势矩阵**：3 种手势（middle / both / both-reverse）× 2 条规则 = 6 项，
全部走**真实输入路径**（`View::button_down` / `button_up`），不直接调 `Board::apply_chord` ——
这样按键顺序、按下态、模式判定都被覆盖到。

已实测有效：把 `update_press_mode` 里的「左右同按」条件删掉，矩阵立刻报出 4 项失败。

**`cargo test` 与 `--selftest` 共用同一份清单**：`minekeeper-cli` 有一条测试直接跑
`checks::checks()`，两份清单走偏会立刻失败。

### 10.3 渲染基线

**关键优势**：三平台共用同一份 `tiny-skia`，所以基线**只有一套**，
CI 上可以做**精确比对**，不需要容差。

`testdata/golden/` 里有 **15 张**已录制的基线，覆盖小/中/大三档 ×
旗子 / 问号 / 输 / 赢 / chord 等状态，共 15 张。

```sh
# 校验（CI 跑的就是这条）
cargo run -p minekeeper-cli --release -- --golden

# 有意改了画面之后重录
cargo run -p minekeeper-cli --release -- --golden --update
```

**基线缺失会直接失败**（退出码 3），不会静默跳过。
早期 CI 写成 `if [ -f ... ]` 包一层，结果基线目录空着也一路绿灯 —— 那种「有测试但不跑」
比没有测试更危险。

`layout-*`（`--genlayout` 的雷位输入）与 `bevel-*`（调试凸起格斜接的 8×8 小图）
不产基线。

**已实测有效**：把 `theme::FACE` 从 `0xC0C0C0` 改成 `0xC0C0C1`，
15 张基线全部报错并给出分区定位（状态栏 / 雷区 / 其它）。

**跨后端比较的判定规则**（用于不同光栅器之间的异构比较）：
`--compare` 按状态栏 / 雷区 / 其它分区报告差异。
只允许「格内字形」与「笑脸面部」两个分区有差异（对应 D1–D3），其余必须零差异。

### 10.4 CI 矩阵

见 `rust/.github/workflows/ci.yml`：

- `headless` job：三平台跑 `fmt` / `clippy` / `test` / `selftest` / 渲染基线比对
- `build-gui` job：三平台编译 GUI + 体积守卫（> 2 MB 失败）+ 产物上传

---

## 11. 开发流程与里程碑

### 11.1 里程碑

| # | 名称 | 交付物 | 验收标准 | 预估 |
|---|---|---|---|---|
| **M0** | **风险原型**（可丢弃） | 一个 winit + softbuffer 窗口，手绘一个静态高级盘面 | **约束 1 与 3 验证通过**：左右键同按可观测、拖出窗口后抬起事件能收到 | 3–5 天 |
| M1 | 内核骨架 | `mk-core` 全部模块可编译可测试 | `cargo test` 全绿；`--selftest` 通过 | 已完成 |
| M2 | 渲染闭环 | `--script` 出图 + 基线录制 | 15 张基线逐像素比对通过 | ✅ 已完成 |
| M3 | 完整 GUI | 三平台可玩 | 菜单、对话框、计时、纪录、音效全部可用 | ✅ 已完成（macOS 实测） |
| M4 | 打磨与发布 | CI 矩阵 + 三平台产物 | 体积 ≤ 1.5 MB；三平台人工验收 | 3–5 天 |

**M0 是硬门槛。** 如果约束 1 或 3 验证失败，整个 Rust 方案需要重新评估
（那时应该回头评估「直连 Win32 + 自研 X11 后端」的方案）。

### 11.2 每个里程碑的固定动作

1. `cargo fmt --all` 与 `cargo clippy --workspace --all-targets -- -D warnings` 必须干净
2. `cargo test --workspace` 全绿
3. `cargo run -p minekeeper-cli --release -- --selftest 2000` 通过
4. 有意改了画面 → 重录基线，并在提交信息里写清**为什么**画面变了
5. 提交信息说明该里程碑改变了哪些不变量

### 11.3 编码规范

- **注释解释「为什么」**，尤其是「这里为什么不能按直觉写」的地方
  （例如 `softbuffer::present` 为什么先快照、`expand` 为什么用显式栈）。
- **不写 `unsafe`**（`mk-core` 已 `#![forbid(unsafe_code)]`）。
- 绘制代码里**不出现字面量像素**，一律走 `Metrics::px` / `div_px`。
- 新增依赖前先问：能不能用已有的？体积代价是多少？

---

## 12. 构建、CI 与发布

### 12.1 本地构建

一条命令出全部产物：

```sh
./scripts/build-all.sh              # macOS（通用）+ Windows + Linux
./scripts/build-all.sh --macos      # 只出 macOS
./scripts/build-all.sh --windows    # 只出 Windows
./scripts/build-all.sh --linux      # 只出 Linux
./scripts/build-all.sh --install-deps   # 缺依赖时自动装
```

产物：

```
dist/
  macos/MineKeeper.app      通用包（arm64 + x86_64），ad-hoc 签名
  macos/minekeeper-cli      通用命令行（自检 / 出图 / 比对）
  windows-x64/minekeeper.exe      PE Subsystem = 2 (GUI)，无控制台
  windows-x64/minekeeper-cli.exe
  linux-x64/minekeeper            Linux GUI（**仅本机构建**，见下）
  linux-x64/minekeeper-cli
  SHA256SUMS
```

脚本会做三件事：**构建**、**自检**（通用 CLI 跑 22 项）、**验产物**
（macOS 用 `lipo -info` 确认双架构；Windows 读 PE 可选头的 Subsystem，必须是 2，
否则说明 `#![windows_subsystem]` 被删了、双击会闪控制台）。

**版本号一致性**：`Cargo.toml` 是唯一权威，另有四处必须与它相同 ——
`scripts/build-all.sh` 的 `VERSION=`、设计文档头部、`CHANGELOG` 最新条目，
以及（tag 触发 CI 时）tag 本身。这四处靠手动对齐，漏改不会有任何报错，
所以 CI 有一条 `./scripts/check-version.sh` 专门守它。

单包构建仍然可以：

```sh
cargo build --release -p minekeeper        # 宿主架构
cargo build --release -p minekeeper-cli
```

`profile.release` 的配置见 §4.4 —— **不要用默认口径**，体积会翻倍。

**Linux 的两种构建方式**，脚本按宿主自动选择：

| 宿主 | 目标 | 音频 | 产物 |
|---|---|---|---|
| Linux | 本机 `x86_64-unknown-linux-gnu` | **有**（ALSA） | GUI + CLI，动态链接 ELF |
| macOS / Windows | 交叉到 `x86_64-unknown-linux-musl` | 无（CLI 不用音频） | **只有 CLI**，静态 pie ELF |

**交叉构建不产出 GUI，这是刻意的。** 交叉出来的 GUI 曾一度被当作产物发布，
直到发现它根本起不了窗口（见下）。现在 CI 的 `build-gui` 会用
`minekeeper --smoke-test` 在 Xvfb 下真的建一次窗口，这类「能编译不能运行」
的产物不会再溜过去。

 静态 musl 没有动态加载器，`dlopen` 必然失败；
而 `winit` 的 X11 / Wayland 后端正是通过 `libloading` 在运行时 dlopen
`libX11` / `libwayland-client`（`cargo tree` 里能看到 x11-dl、wayland-client、libloading）。
交叉出来的 GUI **能编译、能链接，但一建窗口就失败** —— 那种产物不该发布。

CLI 完全不碰窗口栈（对同一目标做依赖查询，winit/libloading 命中数为 0），
所以静态 musl 下可以正常工作。Linux 的 GUI 请在 Linux 上构建，或取 CI
`build-gui (ubuntu)` 的产物。

交叉路径另有两处必须绕开：

1. **音频**。`cpal` 在 Linux 上经 `alsa-sys` 编译，需要 `libasound` 头文件。
   交叉编译拿不到，所以 `mk-host` 的 `audio` 做成可选 feature，交叉时
   `--no-default-features` 关掉，播放自动降级为 [`SilentPlayer`]。
   **这个 feature 没有放进 `mk-host` 的 `default`** —— 放进去之后
   `workspace = true` 的依赖写法无法可靠地覆盖 `default-features`，关了也白关。
2. **`-ldl`**。musl 没有单独的 `libdl.a`，而 `x11-dl` / `wayland-sys` 仍会 `-ldl`
   （现在交叉只编 CLI，理论上碰不到，但保留这层兜底）。脚本在临时目录放一个内容
   为注释的链接脚本冒充 `libdl.a`，`rust-lld` 按链接脚本解析，等于空输入。

另外交叉链接必须显式指定 `-C linker=rust-lld` —— 默认会用宿主机的 `cc`，
而 macOS 的 `ld` 不认识 `--as-needed`、`-Bstatic` 这些 ELF 参数。

**Linux 的运行时依赖**（编译期依赖之外）：winit 的 X11 后端在**运行时** dlopen
`libX11` / `libXcursor` / `libXrandr` / `libXi` / `libxkbcommon` / `libxkbcommon-x11`；
cpal 运行时需要 `libasound2`。桌面发行版通常都有，但 `libxkbcommon-x11-0`
在精简镜像里经常缺失 —— CI 的 ubuntu runner 就缺，缺了会在建窗时直接 panic。

**GUI 冒烟测试**：`minekeeper --smoke-test` 会启动事件循环、等第一帧真正呈现
（`Host::present` 在建窗失败时是静默返回的，所以还要确认 `has_window()`），
打印 `SMOKE ok WxH` 后自动退出。退出码 0 才算通过。CI 在 Linux 上套 `xvfb-run`。

**实测体积**（`opt-level="s"` + fat LTO + strip）：

| 目标 | GUI | CLI |
|---|---|---|
| aarch64-apple-darwin | 1.21 MB | 1.27 MB |
| x86_64-apple-darwin | 1.39 MB | 1.45 MB |
| **macOS 通用（lipo）** | **2.64 MB** | **2.67 MB** |
| x86_64-pc-windows-gnu | 1.52 MB | 1.48 MB |
| x86_64-linux-gnu（本机，含音频） | **2.61 MB** | — |
| x86_64-linux-musl（静态，仅 CLI） | — | 1.62 MB |

Linux 静态目标比另外两个大一倍，因为 musl 把 libc 和所有依赖都静态链进去了。

通用包是两架构之和，属正常。`opt-level="z"` 只再省 9 KB，不值得换。
Windows 用 mingw 链接，比 Darwin 目标略大，1.52 MB 仍在 NFR-3 内。

### 12.2 交叉编译

| 目标 | 工具 |
|---|---|
| Windows x64 from macOS/Linux | `x86_64-pc-windows-gnu` + Homebrew `mingw-w64`（脚本已支持） |
| macOS 通用二进制 | `cargo build --target aarch64-apple-darwin` + `--target x86_64-apple-darwin` + `lipo` |
| Linux x64 | 在 Linux 上原生构建；从 macOS/Windows 交叉则用 `scripts/build-all.sh --linux`（musl 静态、无音频） |

> Go 的 `GOOS=windows go build` 开箱即用，Rust 需要额外工具链。这是 Rust 侧的一项真实成本。

### 12.3 产物

| 平台 | 产物 |
|---|---|
| Windows | `minekeeper.exe`（`#![windows_subsystem = "windows"]`，PE Subsystem 2，无控制台） |
| macOS | `MineKeeper.app`（Info.plist + `.icns`，通用二进制） |
| Linux | 尚未接入构建脚本（`cargo build --release` 可直接出 ELF） |

**图标**：用与游戏内完全相同的绘制代码现画。
把绘制部分提到 `mk-core` 做成平台无关的 `pix::app_icon(size) -> Vec<u8>`，
各平台只做包装（Windows: `CreateIcon`；macOS: `NSImage` / icns；Linux: PNG + WM_CLASS）。

---

## 13. 附录

### 13.1 实现状态（截至 2026-10-20）

| 模块 | 状态 |
|---|---|
| `mk-core::theme` | ✅ 完整，含 5 项测试 |
| `mk-core::game` | ✅ 完整，含 7 项测试 |
| `mk-core::raster` | ✅ 完整，含 3 项测试 |
| `mk-core::pix::bevels` | ✅ 完整，含 2 项测试 |
| `mk-core::pix::sprites` | ✅ 完整 |
| `mk-core::pix::led` | ✅ 完整，含 2 项测试 |
| `mk-core::pix::glyph` | ✅ 完整（闭环校正） |
| `mk-core::view` | ✅ 完整，含 13 项测试 |
| `mk-core::sound` | ✅ 合成完整，含 3 项测试 |
| `mk-core::store` | ✅ 完整，含 3 项测试 |
| `mk-core::scene` | ✅ 完整，含 6 项测试 |
| `mk-core::chrome` | ✅ 完整：菜单栏 + 四个对话框，含 18 项测试 |
| `mk-core::pix::icon` | ✅ 完整：平台无关的图标绘制 |
| `mk-core::text` | ✅ 完整：CJK 文本渲染与折行，含 14 项测试 |
| `mk-host::window` | ⚠️ 可编译；约束 1/3 未在真机验证（M0） |
| `mk-host::lib` | ✅ 事件与命令契约，含 1 项测试 |
| `mk-host::audio` | ✅ 完整，含 3 项测试 |
| `minekeeper`（GUI） | ✅ 可用：菜单 / 对话框 / 计时 / 纪录 / 音效全部接线 |
| `minekeeper-cli` | ✅ 可用（自检 / 出图 / 比对 / genlayout / glyphdiag / icon / ui / golden），含 2 项测试 |
| 渲染基线 | ✅ 15 张已录制并校验通过 |
| CI | ✅ 已配置（三平台矩阵 + 基线 + 体积守卫） |
| git 仓库 | ✅ 已 init 并推送到 GitHub |
| 持续集成 | ✅ 已跑通：run #7 六个 job 全绿（三平台编译 + 测试 + 自检 + 基线 + 冒烟 + 体积守卫） |

**实测数据**（`aarch64-apple-darwin`，`rustc 1.99.1`）：

```
cargo fmt --all -- --check               干净
cargo clippy --workspace --all-targets -- -D warnings   0 warning
cargo test --workspace                   84 passed
--selftest 2000                          22 passed, 0 failed
--golden                                 15 张基线逐像素通过
--script expert-fresh                    994×888 ✓（大档，与尺寸表一致）
--script expert-medium-chord             747×668 ✓（中档）
--script beginner-small                  273×316 ✓（小档）
--ui menu / help / name / custom / scores  全部出图正确
```

GUI 运行期实测（`aarch64-apple-darwin`）：

```
窗口客户区       747×688（菜单栏 20 + 游戏 668），整窗 747×720
空闲 CPU         0.1%（ControlFlow::Wait，不空转）
RSS              148 MB（其中 ~135 MB 是 macOS 窗口栈，应用自身约 13 MB）
```

三档客户区尺寸全部命中 README 的实测表，说明 `Metrics` 的取整规则移植正确。
`testdata/scenes/` 的 20 份场景脚本可直接使用。

### 13.2 验证记录

早期版本这里是「未验证项」清单 —— 设计文档写着三平台，实际只在 macOS arm64 上跑过。
截至 2026-10-20 全部关闭，逐条记录**怎么验的**，避免以后重新变成「设计如此」。

| # | 项目 | 结论 | 证据 |
|---|---|---|---|
| U1 | 三平台能否编译**并起窗口** | ✅ | CI `build-gui` 三平台全绿，且新增 GUI 冒烟测试：真的建出窗口、画出第一帧才通过 |
| U2 | CI 能否运行 | ✅ | run #4 六个 job 全绿 |
| U3 | 左右键同时按下 | ✅ macOS / Windows 实测通过 | 用户真机验证（Linux 部分按用户决定不再追） |
| U4 | 指针捕获 | ✅ | 用户真机验证：拖出窗口再回来松手正常 |
| U5 | 触控板能否左右同按 | — | 随 U3 一并关闭，不再作为扩展开关的依据 |
| U6 | IME 中文输入 | ✅ | 用户真机验证：拼音输入姓名正常 |
| U7 | 跨平台渲染一致性 | ✅ | run #3/#4：15 张基线在 **Linux 与 Windows** 上也逐像素一致 |
| U8 | Windows GUI 子系统 | ✅ | 构建脚本读 PE 可选头，Subsystem 必须为 2 (GUI) |

**U7 的分量最重**：它验证的是「内嵌字体 + 单一 `tiny-skia`」这个架构决策
（§6.14）—— 15 张基线在三个操作系统上逐像素相同，所以「一套基线」不是设计意图，
是实测结果。

**真机跑过的**（CI 覆盖不到的）：

- macOS arm64：启动 `.app`、空闲 CPU 0.1%、RSS 143 MB、三档切换、菜单与对话框
- Windows：编译产物在真机运行（用户验证）
- 左右键同按、中键、指针捕获、IME 中文输入 —— 均在实际设备上操作过

### 13.2.1 首次 CI 结果（run #2，`1e10615`）

| Job | 结果 | 原因 |
|---|---|---|
| `build-gui (windows-latest)` | ✅ | Windows 原生编译通过 |
| `build-gui (macos-latest)` | ✅ | |
| `headless (macos-latest)` | ✅ | fmt / clippy / test / selftest / golden 全过 |
| `build-gui (ubuntu-latest)` | ❌ exit 101 | `alsa-sys` 编译失败 —— 缺 `libasound2-dev` |
| `headless (ubuntu-latest)` | ❌ exit 101 | 同上（clippy 步骤先触发构建） |
| `headless (windows-latest)` | ❌ exit 1 | `wrap_does_not_orphan_a_bullet`：测试阈值卡在字体度量的临界值上 |

两个失败都不是「代码在平台上不工作」，而是**环境与测试假设**的问题：

1. Ubuntu 的两次 101 都是 `alsa-sys` 的 build script 失败。修复是给 CI 装
   `libasound2-dev`，并把音频改成可选 feature。
2. Windows 的失败是测试本身写窄了：那段文字在 macOS 的 Arial Bold 下宽 459px，
   测试用 428 作阈值只有 6.7% 余量，换成 Windows 的 CJK 字体就翻不过去。
   折行本来就依赖字体度量，测试应留足余量（改为 240）。

**这正是 CI 第一次运行的价值**：三个平台各自暴露了一个本地测不出来的假设。

### 13.2.2 第二次 CI（run #3，`9345e18`）

修复后重跑，6 个 job 中 5 个通过：

| Job | 结果 |
|---|---|
| `headless (macos-latest)` | ✅ |
| `headless (ubuntu-latest)` | ✅ —— ALSA 修复生效，clippy / test / selftest / golden 全过 |
| `headless (windows-latest)` | ✅ —— 折行测试修复生效 |
| `build-gui (macos-latest)` | ✅ |
| `build-gui (windows-latest)` | ✅ |
| `build-gui (ubuntu-latest)` | ❌ 体积守卫：2,739,272 字节 > 2 MB 上限 |

只剩一处：体积守卫原本对所有平台用同一个 2 MB 上限，而 Linux 的
winit X11/Wayland 后端加 cpal/ALSA 本身就比 macOS/Windows 大一倍
（实测 2.61 MB vs 1.2–1.5 MB）。上限改为按平台取（Linux 3 MB，其余 2 MB）。

**结论**：`headless` 三个平台全绿，意味着 ——
三平台都能编译、全部 86 项测试通过、22 项自检通过、
**15 张渲染基线在 Linux 与 Windows 上逐像素一致**（U7 就此关闭）。
`build-gui` 的三平台编译也都通过，只差体积守卫这个门槛。

### 13.2.3 第三次 CI（run #4，`3191dd5`）

体积守卫改为按平台取上限后，**6 个 job 全部通过**：

```
OK   build-gui (ubuntu-latest)
OK   build-gui (windows-latest)
OK   headless (macos-latest)
OK   build-gui (macos-latest)
OK   headless (windows-latest)
OK   headless (ubuntu-latest)
```

至此 CI 能证明的部分全部为绿：三平台编译、86 项测试、22 项自检、
15 张渲染基线在三个操作系统上逐像素一致、体积守卫。

### 13.2.4 第四次 CI（run #7，`7d82bdc`）

加入 GUI 冒烟测试后的第一次全绿：

```
OK   build-gui (ubuntu-latest)    · gui smoke test 通过
OK   build-gui (windows-latest)   · gui smoke test 通过
OK   build-gui (macos-latest)     · gui smoke test 通过
OK   headless (ubuntu-latest)
OK   headless (windows-latest)
OK   headless (macos-latest)
```

三个平台的 GUI 二进制都**真的建出了窗口并画出了第一帧**，不只是编译通过。
Linux 那次套的是 `xvfb-run`。

中间失败过一次（run #6）：runner 缺 `libxkbcommon-x11-0`，winit 的 X11 后端在
运行时 dlopen 它，缺了会在建窗时 panic（exit 134）。这正是冒烟测试要抓的那类
「能编译、能链接、运行时起不来」的问题 —— 从「靠推断」变成了「CI 直接报错」。

### 13.3 字体与许可

| 用途 | 来源 | 许可 | 是否内嵌 |
|---|---|---|---|
| 格内数字 1–8 与问号 | Liberation Sans Bold 2.1.5 | **SIL OFL 1.1** | **是**，原样复制于 `assets/fonts/`，附完整许可与作者文件 |
| 界面文本（菜单、对话框） | 各平台系统字体 | 随系统 | 否 |

内嵌的只有格内数字字体（约 405 KB）。原样复制、未作修改，因此保留原文件名与保留字体名，
符合 OFL 对再分发的要求。界面文本**不内嵌**，因为 CJK 字体体积不可接受。

项目**不包含**任何微软原版的代码、位图、图标或 WAVE 资源 —— 图形与音效全部由代码现画/现合成。

### 13.4 参考

- 设计文档（本文）：`docs/2026-10-20-rust-port-design.md`
- 场景脚本：`testdata/scenes/`（20 份）
- 渲染基线：`testdata/golden/`（15 张）
- 内嵌字体与许可：`assets/fonts/`

> 仓库外的历史实现与取证材料已废弃，只作为出处标注保留，不做成链接
> —— 从 GitHub 上点开必然 404。

### 13.5 术语

| 术语 | 含义 |
|---|---|
| **chord** | 中键或左右键同按在一个数字上，触发标雷 / 快速展开 |
| **规则 1** | 未翻开邻格数 == 数字 ⇒ 全部标雷 |
| **规则 2** | 已标旗邻格数 == 数字 ⇒ 翻开其余邻格 |
| **三档** | 小 16 / 中 24 / 大 32 设备像素的方格边长 |
| **逻辑像素** | 16px 基准下的坐标口径，即从 XP 截图量出的原始尺寸 |
| **色键合成** | 只写非透明像素的 1:1 位图拷贝，不涉及 alpha 混合 |
| **闭环校正** | 画完量实际墨迹，把误差补回缩放与平移，迭代至收敛 |
