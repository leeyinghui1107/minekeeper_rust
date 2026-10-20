//! MineKeeper 图形界面入口。
//!
//! 这一层只做四件事：把宿主事件分派给菜单/对话框或游戏、把画好的缓冲交给宿主呈现、
//! 管理计时器与纪录、把菜单命令落到状态上。**不含任何绘制逻辑**。
//!
//! # 布局
//!
//! 窗口客户区自上而下是「菜单栏 [`Chrome::BAR_HEIGHT`] 像素 + 游戏画面」。
//! 游戏画面下移靠 [`Canvas::set_origin`]，所以 `view` 里的坐标一个都不用改；
//! 反过来，喂给 `view` 的鼠标 y 坐标要先减掉菜单栏高度。
#![windows_subsystem = "windows"]

use std::cell::RefCell;
use std::rc::Rc;

use mk_core::chrome::{Chrome, ChromeEvent, ChromeOutcome, Command, DialogResult, Options};
use mk_core::game::{self, Difficulty, Kind};
use mk_core::pix::FontStack;
use mk_core::sound::Audio;
use mk_core::store::Store;
use mk_core::text::TextRenderer;
use mk_core::view::SoundCue;
use mk_core::{Canvas, Size, View};
use mk_host::window::{run, AppHandler, WindowSpec, WinitHost};
use mk_host::{CpalPlayer, EditKey, Host, InputEvent};

const CLOCK_TIMER: u32 = 0x4D4B;

/// `View` 的回调是 `'static` 闭包，拿不到 `&mut App`，所以它们只往这里投递，
/// 真正的动作在 [`App::after_input`] 里做。
#[derive(Default)]
struct Pending {
    sounds: Vec<SoundCue>,
    capture: Option<bool>,
    /// 点笑脸 / 拖到笑脸松手请求开新局。
    ///
    /// **这个回调必须接** —— `View` 只负责「请求」，真正开新局要改 `App` 的字段，
    /// 而回调拿不到 `&mut App`。漏接的症状就是「输了之后点笑脸没反应」。
    new_game: bool,
}

struct App {
    view: View,
    chrome: Chrome,
    fonts: FontStack,
    text: TextRenderer,
    audio: Audio,
    scores: Store,
    pending: Rc<RefCell<Pending>>,

    /// 游戏画面画布（尺寸 = 游戏客户区）。
    game_canvas: Option<Canvas>,
    /// 窗口画布（尺寸 = 菜单栏 + 游戏客户区）。
    window_canvas: Option<Canvas>,

    difficulty: Difficulty,
    size: Size,
    marks: bool,
    colored: bool,
    sound_on: bool,
    /// 「单击快速展开」扩展开关。默认关 —— 关闭时与原版逐位一致。
    left_click_chord: bool,

    clock_running: bool,
    /// 窗口当前是否处于激活态。失活时秒表必须停住 —— 见 `after_input`。
    focused: bool,
    /// 上一帧的 phase，用来检测「刚获胜」这个边沿。
    last_phase: game::Phase,
    seed: u64,
    /// 破纪录时挂起的待写入纪录，等姓名对话框返回。
    pending_record: Option<(Kind, i32)>,
    /// 自定义雷区校验失败时暂存的值，关掉提示框后重新打开对话框。
    pending_custom: Option<(i32, i32, i32)>,
}

impl App {
    fn new(size: Size) -> App {
        let mut view = View::new();
        view.apply_size(size);

        let mut chrome = Chrome::new(mk_core::chrome::menu_items());
        chrome.sync(Options {
            size_index: size_index(size),
            ..Default::default()
        });

        let mut app = App {
            view,
            chrome,
            fonts: FontStack::load_default(),
            text: TextRenderer::load_default(),
            audio: Audio::new(Box::new(CpalPlayer::new())),
            scores: Store::load_default(),
            pending: Rc::new(RefCell::new(Pending::default())),
            game_canvas: None,
            window_canvas: None,
            difficulty: Difficulty::EXPERT,
            size,
            marks: true,
            colored: true,
            sound_on: true,
            left_click_chord: false,
            clock_running: false,
            focused: true,
            last_phase: game::Phase::Ready,
            seed: 1,
            pending_record: None,
            pending_custom: None,
        };
        app.wire_events();
        app.sync_view_options();
        app.new_game();
        app
    }

    /// 把开关落到 `View` 上。`View` 是游戏状态的唯一持有者，这里只做单向同步。
    fn sync_view_options(&mut self) {
        self.view.allow_question = self.marks;
        self.view.colored = self.colored;
        self.view.left_click_chord = self.left_click_chord;
    }

    fn menu_options(&self) -> Options {
        Options {
            marks: self.marks,
            colored: self.colored,
            sound: self.sound_on,
            left_click_chord: self.left_click_chord,
            size_index: size_index(self.size),
        }
    }

    fn wire_events(&mut self) {
        let p = self.pending.clone();
        self.view.events.play_sound = Some(Box::new(move |cue| p.borrow_mut().sounds.push(cue)));

        let p = self.pending.clone();
        self.view.events.capture = Some(Box::new(move |on| p.borrow_mut().capture = Some(on)));

        let p = self.pending.clone();
        self.view.events.new_game_requested =
            Some(Box::new(move || p.borrow_mut().new_game = true));
    }

    fn new_game(&mut self) {
        self.seed = self
            .seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.view.start_game(self.difficulty, self.seed);
        self.last_phase = self.view.board().phase;
        self.view.invalidate_all_pub();
    }

    // ── 窗口几何 ────────────────────────────────────────────────────────────

    fn game_w(&self) -> i32 {
        self.view.client_width()
    }
    fn game_h(&self) -> i32 {
        self.view.client_height()
    }
    fn window_w(&self) -> i32 {
        self.game_w()
    }
    fn window_h(&self) -> i32 {
        self.game_h() + Chrome::BAR_HEIGHT
    }
    fn menu_h(&self) -> i32 {
        Chrome::BAR_HEIGHT
    }

    // ── 计时器 ──────────────────────────────────────────────────────────────

    fn start_clock(&mut self, host: &mut WinitHost) {
        if self.view.board().seconds >= 999 {
            return;
        }
        host.set_timer(CLOCK_TIMER, 1000);
        self.clock_running = true;
    }

    fn stop_clock(&mut self, host: &mut WinitHost) {
        if self.clock_running {
            host.kill_timer(CLOCK_TIMER);
            self.clock_running = false;
        }
    }

    /// 每次输入之后同步派生状态：音效、指针捕获、计时器、纪录。
    ///
    /// **不能用「计时器在不在跑」判断要不要续表** —— `stop_clock` 会清掉那个标志，
    /// 那样永远续不上。判据必须是 `board.phase`。
    fn after_input(&mut self, host: &mut WinitHost) {
        let (sounds, capture, new_game) = {
            let mut p = self.pending.borrow_mut();
            (
                std::mem::take(&mut p.sounds),
                p.capture.take(),
                std::mem::replace(&mut p.new_game, false),
            )
        };
        // 开新局要排在计时器同步**之前**：新局回到 Ready，计时器应当随即停下。
        if new_game {
            self.new_game();
        }
        for cue in sounds {
            self.audio.play(match cue {
                SoundCue::Open => mk_core::sound::Kind::Open,
                SoundCue::Boom => mk_core::sound::Kind::Boom,
                SoundCue::Cheer => mk_core::sound::Kind::Cheer,
            });
        }
        if let Some(on) = capture {
            host.set_capture(on);
        }

        let phase = self.view.board().phase;
        // **必须同时看 `focused`**：失活时 `stop_clock` 刚把表停掉，这里是同一轮
        // 输入里紧接着执行的，若只看 phase 会把表立刻重新装上 —— 失活停表等于没做。
        if phase == game::Phase::Playing && self.focused && !self.clock_running {
            self.start_clock(host);
        }
        if phase != game::Phase::Playing && self.clock_running {
            self.stop_clock(host);
        }
        if phase == game::Phase::Won && self.last_phase != game::Phase::Won {
            self.on_won();
        }
        self.last_phase = phase;
    }

    /// 获胜结算。破纪录时弹姓名框，等 [`DialogResult::Name`] 回来再落盘。
    fn on_won(&mut self) {
        let seconds = self.view.board().seconds;
        let kind = self.difficulty.kind;
        if !self.scores.is_record(kind, seconds) {
            return;
        }
        self.pending_record = Some((kind, seconds));
        let prompt = format!("已破{}记录。\n请留尊姓大名。", self.difficulty.label());
        let initial = self.scores.get(kind).name;
        self.chrome.open_name(&prompt, &initial);
    }

    // ── 菜单命令 ────────────────────────────────────────────────────────────

    fn command(&mut self, host: &mut WinitHost, cmd: Command) {
        match cmd {
            Command::New => self.new_game(),
            Command::Beginner => self.set_difficulty(host, Difficulty::BEGINNER),
            Command::Intermediate => self.set_difficulty(host, Difficulty::INTERMEDIATE),
            Command::Expert => self.set_difficulty(host, Difficulty::EXPERT),
            Command::Custom => {
                let d = self.difficulty;
                self.chrome.open_custom(d.rows, d.cols, d.mines);
            }
            Command::SizeSmall => self.set_size(host, Size::Small),
            Command::SizeMedium => self.set_size(host, Size::Medium),
            Command::SizeLarge => self.set_size(host, Size::Large),
            Command::Marks => {
                self.marks = !self.marks;
                self.sync_view_options();
            }
            Command::Colored => {
                self.colored = !self.colored;
                self.sync_view_options();
                self.view.invalidate_all_pub();
            }
            Command::LeftClickChord => {
                self.left_click_chord = !self.left_click_chord;
                self.sync_view_options();
            }
            Command::Sound => {
                self.sound_on = !self.sound_on;
                self.audio.enabled = self.sound_on;
                if !self.sound_on {
                    self.audio.stop_all();
                }
            }
            Command::Scores => self.show_scores(),
            Command::Exit => host.quit(),
            Command::HelpUsing => self
                .chrome
                .open_message("扫雷帮助", mk_core::chrome::HELP_USING),
            Command::HelpAbout => self
                .chrome
                .open_message("关于扫雷", mk_core::chrome::ABOUT_TEXT),
        }
        let opts = self.menu_options();
        self.chrome.sync(opts);
        self.flush(host);
    }

    fn set_difficulty(&mut self, host: &mut WinitHost, d: Difficulty) {
        self.difficulty = d;
        self.new_game();
        self.fit(host);
    }

    fn set_size(&mut self, host: &mut WinitHost, s: Size) {
        if self.size == s {
            return;
        }
        self.size = s;
        self.view.apply_size(s);
        self.game_canvas = None;
        self.window_canvas = None;
        self.fit(host);
        self.view.invalidate_all_pub();
    }

    fn show_scores(&mut self) {
        let e = |k: Kind| {
            let entry = self.scores.get(k);
            if entry.has_record() {
                (format!("{} 秒", entry.seconds), entry.name)
            } else {
                ("999 秒".to_string(), "匿名".to_string())
            }
        };
        self.chrome
            .open_scores([e(Kind::Beginner), e(Kind::Intermediate), e(Kind::Expert)]);
    }

    // ── 对话框结果 ──────────────────────────────────────────────────────────

    fn poll_dialogs(&mut self, host: &mut WinitHost) {
        while let Some(r) = self.chrome.take_dialog_result() {
            match r {
                DialogResult::Name(name) => {
                    if let Some((kind, seconds)) = self.pending_record.take() {
                        self.scores.record(kind, seconds, &name);
                    }
                }
                DialogResult::Custom { rows, cols, mines } => {
                    // **先校验、不先夹**。早先是「先 clamp 再 is_valid_custom」，而
                    // is_valid_custom 判定的正是那几个区间，于是恒真 —— 提示框永远弹不出来，
                    // 玩家输入 999 颗雷会被静默改写成上限值。
                    if Difficulty::is_valid_custom(rows, cols, mines) {
                        self.set_difficulty(host, Difficulty::custom(rows, cols, mines));
                    } else {
                        // 原版行为：拒绝，并在关掉提示后把对话框放回原处等玩家改。
                        self.pending_custom = Some((rows, cols, mines));
                        self.chrome.open_message(
                            "自定义雷区",
                            &format!(
                                "请输入有效范围：\n高度 {}–{}\n宽度 {}–{}\n雷数 {}–{}。",
                                Difficulty::MIN_ROWS,
                                Difficulty::MAX_ROWS,
                                Difficulty::MIN_COLS,
                                Difficulty::MAX_COLS,
                                Difficulty::MIN_MINES,
                                Difficulty::max_mines_for(
                                    rows.clamp(Difficulty::MIN_ROWS, Difficulty::MAX_ROWS),
                                    cols.clamp(Difficulty::MIN_COLS, Difficulty::MAX_COLS),
                                )
                            ),
                        );
                    }
                }
                DialogResult::ResetScores => {
                    self.scores.clear();
                    self.show_scores();
                }
                DialogResult::Dismissed => {
                    // 自定义雷区校验失败时，关掉提示后把对话框放回去继续改。
                    if let Some((r, c, m)) = self.pending_custom.take() {
                        self.chrome.open_custom(r, c, m);
                    }
                }
            }
        }
    }

    // ── 输入路由 ────────────────────────────────────────────────────────────

    /// 把宿主事件翻成界面事件。返回 `None` 表示这个事件与界面无关。
    fn to_chrome(&self, ev: &InputEvent) -> Option<ChromeEvent> {
        let modal = self.chrome.is_modal();
        let menu_open = self.chrome.menu_open();
        match ev {
            InputEvent::ButtonDown(mk_core::Button::Left, x, y) => {
                // 菜单栏始终可点；菜单展开或模态时，任意位置的点击都归界面。
                (self.chrome.in_bar(*y) || modal || menu_open).then_some(ChromeEvent::Click(*x, *y))
            }
            InputEvent::ButtonUp(mk_core::Button::Left, x, y) => {
                (modal || menu_open || self.chrome.in_bar(*y))
                    .then_some(ChromeEvent::Release(*x, *y))
            }
            InputEvent::MouseMove(x, y) => {
                (modal || menu_open || self.chrome.in_bar(*y)).then_some(ChromeEvent::Move(*x, *y))
            }
            InputEvent::Text(s) => Some(ChromeEvent::Text(s.clone())),
            InputEvent::Edit(e) => Some(match e {
                EditKey::Backspace => ChromeEvent::Backspace,
                EditKey::Delete => ChromeEvent::Delete,
                EditKey::Home => ChromeEvent::Home,
                EditKey::End => ChromeEvent::End,
                EditKey::Escape => ChromeEvent::Escape,
                EditKey::Tab => ChromeEvent::Tab,
            }),
            // 模态时方向键与回车属于文本编辑，不属于游戏。
            InputEvent::KeyDown(k) if modal => match k {
                mk_core::Key::Left => Some(ChromeEvent::Left),
                mk_core::Key::Right => Some(ChromeEvent::Right),
                mk_core::Key::Enter | mk_core::Key::Space => Some(ChromeEvent::Enter),
                _ => None,
            },
            _ => None,
        }
    }

    fn flush(&mut self, host: &mut WinitHost) {
        let (dirty, all) = self.view.take_dirty();
        if all || !dirty.is_empty() {
            host.invalidate(&dirty);
        } else {
            host.invalidate(&[]);
        }
    }

    fn fit(&mut self, host: &mut WinitHost) {
        host.fit_client(self.window_w(), self.window_h());
    }
}

impl AppHandler for App {
    fn resumed(&mut self, host: &mut WinitHost) {
        host.set_title("扫雷");
        self.fit(host);
        self.view.invalidate_all_pub();
    }

    fn input(&mut self, host: &mut WinitHost, ev: InputEvent) {
        if ev == InputEvent::QuitRequested {
            host.quit();
            return;
        }

        // 1) 界面优先
        if let Some(ce) = self.to_chrome(&ev) {
            let (w, h) = (self.window_w(), self.window_h());
            match self.chrome.handle(ce, w, h, &mut self.text) {
                ChromeOutcome::Command(c) => {
                    self.command(host, c);
                    return;
                }
                ChromeOutcome::Redraw => {
                    host.invalidate(&[]);
                    return;
                }
                ChromeOutcome::Ignored => {} // 落到游戏
            }
        }

        // 2) 界面挡着时不把事件漏给游戏
        if self.chrome.is_modal() || self.chrome.menu_open() {
            return;
        }

        // 3) 游戏。注意 y 要减掉菜单栏高度。
        let dy = -self.menu_h();
        match ev {
            InputEvent::ButtonDown(b, x, y) => self.view.button_down(b, (x, y + dy)),
            InputEvent::ButtonUp(b, x, y) => self.view.button_up(b, (x, y + dy)),
            InputEvent::MouseMove(x, y) => self.view.mouse_move((x, y + dy)),
            InputEvent::MouseLeave => self.view.mouse_leave(),
            InputEvent::KeyDown(mk_core::Key::F2) => self.new_game(),
            InputEvent::KeyDown(mk_core::Key::F1) => {
                self.chrome
                    .open_message("扫雷帮助", mk_core::chrome::HELP_USING);
            }
            InputEvent::KeyDown(k) => self.view.key_down(k),
            InputEvent::Focus(focused) => {
                self.focused = focused;
                if !focused {
                    self.stop_clock(host);
                }
            }
            _ => {}
        }

        self.after_input(host);
        self.poll_dialogs(host);
        self.flush(host);
    }

    fn redraw(&mut self, host: &mut WinitHost) {
        let (gw, gh) = (self.game_w(), self.game_h());
        let (ww, wh) = (self.window_w(), self.window_h());

        if self.game_canvas.as_ref().map(|c| (c.width(), c.height())) != Some((gw, gh)) {
            self.game_canvas = Canvas::new(gw, gh);
        }
        if self.window_canvas.as_ref().map(|c| (c.width(), c.height())) != Some((ww, wh)) {
            self.window_canvas = Canvas::new(ww, wh);
        }
        let menu_h = self.menu_h();
        let (Some(game), Some(win)) = (self.game_canvas.as_mut(), self.window_canvas.as_mut())
        else {
            return;
        };

        // 游戏画面：画进自己的画布（坐标不变），再整体贴到菜单栏之下。
        let bounds = game.bounds();
        self.view.paint(game, bounds, &mut self.fonts);

        win.clear(mk_core::theme::FACE);
        win.set_origin(0, menu_h);
        win.blit((0, 0), game);
        win.set_origin(0, 0);

        // 菜单栏与对话框画在最上层。
        self.chrome.paint(win, ww, &mut self.text);

        let data = win.to_rgba();
        host.present(&data, ww, wh);
    }

    fn tick(&mut self, host: &mut WinitHost, id: u32) {
        if id == CLOCK_TIMER {
            self.view.tick_second();
            self.flush(host);
        }
    }
}

fn size_index(s: Size) -> usize {
    match s {
        Size::Small => 0,
        Size::Medium => 1,
        Size::Large => 2,
    }
}

fn main() {
    let mut size = Size::Medium;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--size" => match args.get(i + 1).map(|s| Size::parse(s)) {
                Some(Ok(s)) => {
                    size = s;
                    i += 2;
                }
                _ => {
                    eprintln!("用法：--size small|medium|large（或 16 / 24 / 32）");
                    std::process::exit(2);
                }
            },
            other => {
                eprintln!("未知参数：{other}");
                std::process::exit(2);
            }
        }
    }

    let app = App::new(size);
    let spec = WindowSpec {
        width: app.window_w(),
        height: app.window_h(),
        title: "扫雷",
    };
    if let Err(e) = run(app, WinitHost::new(spec)) {
        eprintln!("事件循环失败：{e}");
        std::process::exit(1);
    }
}
