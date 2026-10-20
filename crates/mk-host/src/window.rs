//! `winit` + `softbuffer` 的窗口宿主。**一份代码跑三平台。**
//!
//! 事件循环由 `winit` 拥有，这里把它翻译成 [`InputEvent`] 与 [`Command`]，
//! 通过 [`AppHandler`] 回调给上层应用。

use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Ime, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WKey, KeyCode, ModifiersState, NamedKey, PhysicalKey};
use winit::window::{Window, WindowId};

use mk_core::{Button, Key, Rect};

use crate::{EditKey, Host, InputEvent};

/// 上层应用要实现的东西。
pub trait AppHandler {
    /// 窗口就绪，可以开始画了。
    fn resumed(&mut self, host: &mut WinitHost);
    fn input(&mut self, host: &mut WinitHost, ev: InputEvent);
    /// 系统要求重绘。
    fn redraw(&mut self, host: &mut WinitHost);
    /// 定时器到期。
    fn tick(&mut self, host: &mut WinitHost, id: u32);
}

/// 窗口尺寸（客户区逻辑像素）。
#[derive(Clone, Copy, Debug)]
pub struct WindowSpec {
    pub width: i32,
    pub height: i32,
    pub title: &'static str,
}

/// `winit` 宿主。
pub struct WinitHost {
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    spec: WindowSpec,
    /// 整数倍显示缩放。**约束 5**：呈现只允许按这个倍数最近邻放大。
    scale: i32,
    timer: Option<(u32, Duration, Instant)>,
    /// 复用同一块转换缓冲，避免每帧分配。
    scratch: Vec<u32>,
    /// 最后一次光标位置（**逻辑**像素）。`MouseInput` 事件本身不带坐标，
    /// 而按下时的命中测试需要它 —— 不记这个，所有点击都会落在 (0,0)。
    cursor: (i32, i32),
    /// `quit()` 只是置位，真正退出事件循环由 `Runner` 做（它才拿得到 `ActiveEventLoop`）。
    quit_requested: bool,
}

impl WinitHost {
    pub fn new(spec: WindowSpec) -> WinitHost {
        WinitHost {
            window: None,
            surface: None,
            spec,
            scale: 1,
            timer: None,
            scratch: Vec::new(),
            cursor: (0, 0),
            quit_requested: false,
        }
    }

    /// 窗口是否真的建出来了。宿主的 `present` 在建窗失败时是静默返回的，
    /// 所以「呈现过一次」不等于「窗口存在」—— 冒烟测试要用这个判定。
    pub fn has_window(&self) -> bool {
        self.window.is_some()
    }

    /// 请求一次重绘。
    pub fn request_redraw(&self) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    /// 把窗口尺寸调到目标客户区大小。
    ///
    /// `winit` 直接接受**客户区**尺寸，不需要像 Win32 那样「先猜一个外框、
    /// 再迭代修正非客户区差值」。
    pub fn fit_client(&mut self, w: i32, h: i32) {
        if let Some(win) = &self.window {
            // LogicalSize 是**逻辑**单位，winit 会按 scale_factor 自己换算成物理像素，
            // 所以这里不能再乘 scale —— 乘了会得到 4 倍大小的窗口。
            let _ = win.request_inner_size(LogicalSize::new(w as f64, h as f64));
        }
    }

    fn ensure_surface(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(self.spec.title)
            .with_inner_size(LogicalSize::new(
                self.spec.width as f64,
                self.spec.height as f64,
            ));
        let Ok(w) = el.create_window(attrs) else {
            return;
        };
        let w = Rc::new(w);
        // 整数倍缩放：取整数部分，且至少 1。
        self.scale = (w.scale_factor().floor() as i32).max(1);
        if let Ok(ctx) = softbuffer::Context::new(w.clone()) {
            if let Ok(s) = softbuffer::Surface::new(&ctx, w.clone()) {
                self.surface = Some(s);
            }
        }
        // 允许 IME：破纪录时的中文姓名输入依赖它。
        w.set_ime_allowed(true);
        self.window = Some(w);
        // winit 不保证窗口创建后自动来一发 RedrawRequested，必须自己请。
        self.request_redraw();
    }
}

impl Host for WinitHost {
    fn present(&mut self, buf: &[u8], w: i32, h: i32) {
        let (Some(win), Some(surf)) = (self.window.clone(), self.surface.as_mut()) else {
            return;
        };
        let size = win.inner_size();
        let (pw, ph) = (size.width, size.height);
        if pw == 0 || ph == 0 || w <= 0 || h <= 0 {
            return;
        }
        let s = self.scale.max(1);
        let (sw, sh) = (w * s, h * s);

        self.scratch.resize((pw * ph) as usize, 0);
        for y in 0..ph as i32 {
            let sy = (y / s).min(h - 1);
            for x in 0..pw as i32 {
                let sx = (x / s).min(w - 1);
                // 超出源图的部分留白（窗口比画布大时才会发生）。
                if x >= sw || y >= sh {
                    self.scratch[(y as u32 * pw + x as u32) as usize] = 0;
                    continue;
                }
                let si = ((sy as u32 * w as u32 + sx as u32) * 4) as usize;
                let r = buf[si] as u32;
                let g = buf[si + 1] as u32;
                let b = buf[si + 2] as u32;
                self.scratch[(y as u32 * pw + x as u32) as usize] = (r << 16) | (g << 8) | b;
            }
        }

        let _ = surf.resize(
            NonZeroU32::new(pw).unwrap_or(NonZeroU32::new(1).unwrap()),
            NonZeroU32::new(ph).unwrap_or(NonZeroU32::new(1).unwrap()),
        );
        if let Ok(mut dst) = surf.buffer_mut() {
            let n = dst.len().min(self.scratch.len());
            dst[..n].copy_from_slice(&self.scratch[..n]);
            // present 会消费 self，所以上面必须先写完。
            let _ = dst.present();
        }
    }

    fn invalidate(&mut self, _rects: &[Rect]) {
        // softbuffer 是整帧呈现，脏矩形只用来减少**重绘**的工作量，
        // 不改变上传粒度。真正的收益在 `View::paint` 的 clip 参数上。
        self.request_redraw();
    }

    fn set_title(&mut self, title: &str) {
        if let Some(w) = &self.window {
            w.set_title(title);
        }
    }

    fn set_timer(&mut self, id: u32, ms: u32) {
        self.timer = Some((id, Duration::from_millis(ms as u64), Instant::now()));
    }

    fn kill_timer(&mut self, id: u32) {
        if self.timer.map(|(t, _, _)| t) == Some(id) {
            self.timer = None;
        }
    }

    fn set_capture(&mut self, _on: bool) {
        // 这里**故意是空实现**，两点原因：
        //
        // 1. `winit` 没有暴露跨平台的 `SetCapture`。Windows 的 `SetCapture` / X11 的
        //    `XGrabPointer` 都得自己调平台 API（见设计文档 §9 的风险 R2）。
        // 2. macOS 与 Wayland 本来就由系统隐式抓取：按下鼠标后，窗口会持续收到
        //    mouseDragged / mouseUp，直到抬起为止，哪怕指针已经移出窗口。
        //    所以「按住格子拖到笑脸再松手」在本平台不需要额外动作。
        //
        // 注意：**不要**用 `set_cursor_hittest(!on)` 凑数 —— 那个 API 是让窗口
        // 「忽略鼠标事件」，语义正好相反，会把拖拽彻底弄坏。
    }

    fn scale(&self) -> i32 {
        self.scale
    }

    fn quit(&mut self) {
        self.quit_requested = true;
    }
}

/// 跑事件循环。这个函数会阻塞到窗口关闭。
pub fn run<A: AppHandler + 'static>(app: A, host: WinitHost) -> Result<(), String> {
    let el = EventLoop::new().map_err(|e| e.to_string())?;
    let mut runner = Runner {
        app,
        host,
        modifiers: ModifiersState::empty(),
    };
    el.run_app(&mut runner).map_err(|e| e.to_string())
}

struct Runner<A: AppHandler> {
    app: A,
    host: WinitHost,
    modifiers: ModifiersState,
}

impl<A: AppHandler> ApplicationHandler for Runner<A> {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        self.host.ensure_surface(el);
        self.app.resumed(&mut self.host);
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if self.host.quit_requested {
            el.exit();
            return;
        }
        // 没有计时器时就真等事件，不要每 100ms 空转一次（省电，也让 idle CPU 归零）。
        match self.host.timer {
            Some((id, period, last)) => {
                let now = Instant::now();
                if now.duration_since(last) >= period {
                    self.host.timer = Some((id, period, now));
                    self.app.tick(&mut self.host, id);
                }
                let next = self.host.timer.map(|(_, p, l)| l + p).unwrap_or(now);
                el.set_control_flow(ControlFlow::WaitUntil(next));
            }
            None => el.set_control_flow(ControlFlow::Wait),
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::RedrawRequested => self.app.redraw(&mut self.host),
            WindowEvent::Resized(_) => self.app.redraw(&mut self.host),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                // 窗口在不同 DPI 的显示器之间移动（Retina ↔ 外接 1x）时，
                // `scale` 必须跟着更新，否则画面会一直按旧倍数呈现，直到重启。
                self.host.scale = (scale_factor.floor() as i32).max(1);
                self.app.redraw(&mut self.host);
            }
            WindowEvent::Focused(on) => {
                self.app.input(&mut self.host, InputEvent::Focus(on));
            }
            WindowEvent::CursorLeft { .. } => {
                self.app.input(&mut self.host, InputEvent::MouseLeave);
            }
            WindowEvent::CursorMoved { position, .. } => {
                let s = self.host.scale.max(1);
                let (x, y) = (position.x as i32 / s, position.y as i32 / s);
                self.host.cursor = (x, y);
                self.app.input(&mut self.host, InputEvent::MouseMove(x, y));
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let Some(b) = map_button(button) else { return };
                // MouseInput 不带坐标，用最近一次 CursorMoved 记下的位置。
                let (x, y) = self.host.cursor;
                let ev = match state {
                    // 逐键投递，**不做任何合成** —— 左右键同按的全部依赖。
                    ElementState::Pressed => InputEvent::ButtonDown(b, x, y),
                    ElementState::Released => InputEvent::ButtonUp(b, x, y),
                };
                self.app.input(&mut self.host, ev);
            }
            WindowEvent::ModifiersChanged(m) => {
                self.modifiers = m.state();
            }
            // 只吃 IME 的 Commit —— 那才是最终文本。组字过程中的 Preedit 不处理：
            // 自绘候选窗超出当前范围，系统候选窗已经够用。
            WindowEvent::Ime(Ime::Commit(text)) if !text.is_empty() => {
                self.app.input(&mut self.host, InputEvent::Text(text));
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed || event.repeat {
                    return;
                }

                // Cmd+Q / Ctrl+Q：自绘菜单没有原生「退出」项，这里兜住。
                let cmd_or_ctrl = self.modifiers.super_key() || self.modifiers.control_key();
                if cmd_or_ctrl {
                    if let PhysicalKey::Code(KeyCode::KeyQ) = event.physical_key {
                        self.app.input(&mut self.host, InputEvent::QuitRequested);
                        return;
                    }
                }

                // 编辑键与可打印字符来自**逻辑键**（受键盘布局与 IME 影响）。
                match &event.logical_key {
                    WKey::Named(NamedKey::Backspace) => {
                        self.app
                            .input(&mut self.host, InputEvent::Edit(EditKey::Backspace));
                    }
                    WKey::Named(NamedKey::Delete) => {
                        self.app
                            .input(&mut self.host, InputEvent::Edit(EditKey::Delete));
                    }
                    WKey::Named(NamedKey::Home) => {
                        self.app
                            .input(&mut self.host, InputEvent::Edit(EditKey::Home));
                    }
                    WKey::Named(NamedKey::End) => {
                        self.app
                            .input(&mut self.host, InputEvent::Edit(EditKey::End));
                    }
                    WKey::Named(NamedKey::Escape) => {
                        self.app
                            .input(&mut self.host, InputEvent::Edit(EditKey::Escape));
                    }
                    WKey::Named(NamedKey::Tab) => {
                        self.app
                            .input(&mut self.host, InputEvent::Edit(EditKey::Tab));
                    }
                    WKey::Character(txt) if !cmd_or_ctrl => {
                        self.app
                            .input(&mut self.host, InputEvent::Text(txt.to_string()));
                    }
                    _ => {}
                }

                // 游戏按键来自**物理键**，这样中文输入法开着也能用方向键。
                if let PhysicalKey::Code(code) = event.physical_key {
                    if let Some(k) = map_key(code) {
                        self.app.input(&mut self.host, InputEvent::KeyDown(k));
                    }
                }
            }
            _ => {}
        }
    }
}

fn map_button(b: MouseButton) -> Option<Button> {
    match b {
        MouseButton::Left => Some(Button::Left),
        MouseButton::Right => Some(Button::Right),
        MouseButton::Middle => Some(Button::Middle),
        _ => None,
    }
}

fn map_key(code: KeyCode) -> Option<Key> {
    match code {
        KeyCode::ArrowUp => Some(Key::Up),
        KeyCode::ArrowDown => Some(Key::Down),
        KeyCode::ArrowLeft => Some(Key::Left),
        KeyCode::ArrowRight => Some(Key::Right),
        KeyCode::Enter | KeyCode::NumpadEnter => Some(Key::Enter),
        KeyCode::Space => Some(Key::Space),
        KeyCode::KeyF => Some(Key::F),
        KeyCode::F1 => Some(Key::F1),
        KeyCode::F2 => Some(Key::F2),
        _ => None,
    }
}

/// 便捷构造：默认中档高级的窗口尺寸。
pub fn default_spec() -> WindowSpec {
    WindowSpec {
        width: 747,
        height: 668,
        title: "扫雷",
    }
}
