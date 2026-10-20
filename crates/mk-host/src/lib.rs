//! # mk-host —— 宿主层
//!
//! 把「窗口系统要什么」和「游戏要什么」隔开。`mk-core` 不知道窗口的存在，
//! 这一层负责把平台事件翻译成 [`mk_core::View`] 的方法调用，再把画好的缓冲呈现出去。
//!
//! ## 为什么不是「三个平台后端」
//!
//! 早期方案设想的是 win32 / cocoa / x11 三套后端。实际选型后只有**两处**真正需要分平台：
//!
//! | 能力 | 方案 | 是否分平台 |
//! |---|---|---|
//! | 窗口、输入、定时器、像素呈现 | `winit` + `softbuffer` | **否**，一份代码三平台 |
//! | 音频输出 | `cpal` | **否**，一份代码三平台 |
//! | 原生菜单栏 | `mk_core::chrome`（自绘） | 否 |
//! | 模态对话框 | `mk_core::chrome`（自绘） | 否 |
//!
//! 实际做完之后发现**一处都不需要分平台**：菜单与对话框用 `mk_core` 的绘制原语自绘，
//! 文本输入交给系统 IME。这一层因此完全跨平台。
//!
//! ## 宿主必须满足的五条硬约束
//!
//! 这些是 `mk-core` 已经在依赖、而多数 GUI 库默认不提供的行为。选型时逐条验收：
//!
//! 1. **左右键同时按下**必须可观测（[`InputEvent::ButtonDown`] 逐键投递，不合成）。
//! 2. **指针离开客户区**必须可观测（[`InputEvent::MouseLeave`]）。
//! 3. **指针捕获**必须支持（[`Host::set_capture`]）—— 拖到窗口外再松手依赖它。
//! 4. **窗口激活 / 失活**必须可观测（[`InputEvent::Focus`]）。
//! 5. **呈现必须是整数倍最近邻放大**，不允许线性缩放（[`Host::scale`]）。

pub mod audio;
pub mod window;

pub use audio::open_default as open_audio;
pub use window::WinitHost;

use mk_core::{Button, Key};

pub use mk_core::chrome::{Chrome, ChromeEvent, ChromeOutcome, Command, DialogResult, MenuItem};

/// 平台无关的输入事件。宿主把原始消息翻译成它，再逐条投给 `View` 或 `Chrome`。
///
/// `KeyDown` 与 `Text` / `Edit` **会同时投递**（同一个按键可能两者都产生），
/// 由应用按当前模式决定路由：模态对话框开着时 `KeyDown` 一律丢弃，只吃 `Text` / `Edit`。
/// 这样宿主不需要知道应用处于什么模式。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum InputEvent {
    ButtonDown(Button, i32, i32),
    ButtonUp(Button, i32, i32),
    MouseMove(i32, i32),
    /// 指针离开客户区。**不能**用「移出边界」推断 —— 拖拽时指针被捕获，
    /// 移出边界不产生这个事件。
    MouseLeave,
    /// 游戏按键（方向 / Enter / 空格 / F / F1 / F2）。
    KeyDown(Key),
    /// 已提交的文本：IME 组字结果，或直接键入的可打印字符。
    Text(String),
    /// 编辑键。
    Edit(EditKey),
    /// 窗口获得 / 失去激活。失活要停表，回到前台且本局已开始才续表。
    Focus(bool),
    /// Cmd+Q（macOS）/ Ctrl+Q（其他平台）。
    ///
    /// 自绘菜单栏没有原生「退出」项，所以这个快捷键必须在宿主层兜住 ——
    /// 否则 macOS 上用户只能靠关窗口退出。
    QuitRequested,
}

/// 编辑键。与游戏按键分开，因为对话框里的左右方向键属于文本光标，
/// 而不是游戏的焦点框。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditKey {
    Backspace,
    Delete,
    Home,
    End,
    Escape,
    Tab,
}

/// 窗口呈现能力。
pub trait Host {
    /// 把一整块 RGBA8 缓冲呈现到窗口。**必须**按 [`Host::scale`] 整数倍最近邻放大。
    fn present(&mut self, buf: &[u8], w: i32, h: i32);

    /// 请求重绘（把 `mk-core` 收集的脏矩形变成真正的失效请求）。
    fn invalidate(&mut self, rects: &[mk_core::Rect]);

    fn set_title(&mut self, title: &str);

    /// 起一个毫秒级心跳（计时器每秒跳一格）。
    fn set_timer(&mut self, id: u32, ms: u32);
    fn kill_timer(&mut self, id: u32);

    /// 指针捕获开关。**约束 3**：没有它，「按住格子拖到笑脸再松手开新局」无法实现。
    fn set_capture(&mut self, on: bool);

    /// 当前显示缩放（1 = 100%，2 = Retina）。呈现时按这个整数倍最近邻放大。
    fn scale(&self) -> i32;

    fn quit(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 菜单命令 ID 必须与原版取证结果一致 —— 这是「行为复刻」的一部分。
    #[test]
    fn command_ids_match_the_original() {
        assert_eq!(Command::New.id(), 510);
        assert_eq!(Command::Exit.id(), 519);
        assert_eq!(Command::SizeLarge.id(), 522);
        assert_eq!(Command::HelpAbout.id(), 593);
    }
}
