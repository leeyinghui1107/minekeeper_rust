//! # mk-core —— MineKeeper 的平台无关内核
//!
//! 这个 crate **不依赖任何窗口、消息循环或平台 API**。它可以在无显示的 CI 环境里
//! 编译、测试、渲染出图，这也是整个跨平台方案的地基。
//!
//! ## 分层
//!
//! ```text
//!   game     纯逻辑：棋盘状态机、布雷、展开、chord 判定      ← 只依赖 std
//!   theme    16px 基准常量 + 三档换算器                      ← 只依赖 std
//!   raster   光栅层：一块缓冲 + 全部图元（tiny-skia）        ← theme
//!   pix      绘制图元：斜边、雷旗笑脸、数码管、字形          ← raster, theme, game
//!   view     绘制 + 输入的唯一状态机                         ← 以上全部
//!   sound    PCM 合成（可移植）+ Player 接口（后端实现）
//!   store    最佳时间持久化
//!   scene    场景脚本解析（验证栈用）
//! ```
//!
//! 依赖严格单向：**逻辑层不许 import 任何绘图或平台相关的东西**。
//! `game` 里出现 `raster` 就是分层破了。
//!
//! ## 两条贯穿全局的不变量
//!
//! 1. **取整只有一处**：`theme::Metrics::px`，`.5` 一律向上。所有总尺寸由换算后的
//!    基本量相加得到，绝不对逻辑总量整体缩放。
//! 2. **除笑脸面部外一律硬边**：抗锯齿会引入灰阶，破坏「边缘保持整数像素」的承诺。

#![forbid(unsafe_code)]

pub mod chrome;
pub mod game;
pub mod pix;
pub mod raster;
pub mod scene;
pub mod sound;
pub mod text;
pub mod theme;
pub mod view;

#[cfg(feature = "store")]
pub mod store;

pub use chrome::{Chrome, ChromeEvent, ChromeOutcome, Command, DialogResult, MenuItem};
pub use game::{Board, Difficulty, Kind, Mark, Mood, Phase, Pos, Rand};
pub use raster::Canvas;
pub use text::{TextMetrics, TextRenderer};
pub use theme::{Metrics, Rect, Rgb, Size};
pub use view::{Button, Key, View};

/// 版本号，供 `--version` 与「关于」对话框使用。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
