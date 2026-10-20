# 内嵌字体

## LiberationSans-Bold.ttf

格内数字（1–8 与问号）的字形来源。

**为什么内嵌**：字形来自系统字体，而三平台加载的是三个不同字体 ——

| 平台 | 原本会加载 |
|---|---|
| macOS | `/System/Library/Fonts/Supplemental/Arial Bold.ttf` |
| Windows | `C:/Windows/Fonts/arialbd.ttf` |
| Linux | `/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf` |

三者的字形不同，渲染出的像素也就不同，于是**一套渲染基线在三平台无法同时成立**。
实测：把回退表里的 Arial Bold 换成 Verdana Bold，15 张基线里 9 张失败，差异全在雷区。

内嵌之后，格内数字的渲染与平台无关，`testdata/golden/` 的 15 张基线在三平台都成立。

**为什么选它**：SIL OFL 1.1 明确允许原样再分发；且与 Arial **度量兼容**，
字形观感与原先的系统字体最接近，视觉改动最小。

**许可**：SIL Open Font License 1.1，全文见
[`LiberationSans-Bold.LICENSE.txt`](LiberationSans-Bold.LICENSE.txt)。
版权与保留字体名信息见 [`LiberationSans-Bold.AUTHORS.txt`](LiberationSans-Bold.AUTHORS.txt)。

文件为**原样复制**，未作任何修改 —— OFL 允许原样再分发，因此保留原文件名与保留字体名。

来源：<https://github.com/liberationfonts/liberation-fonts>，版本 2.1.5。

## 界面文本不内嵌

菜单栏与对话框的中文文本仍用系统字体（`mk-core/src/text.rs`）——
CJK 字体动辄 16–60 MB，内嵌不现实。界面文本**不进渲染基线**（基线只渲染游戏画面），
所以平台间观感略有差异不影响回归保护。
