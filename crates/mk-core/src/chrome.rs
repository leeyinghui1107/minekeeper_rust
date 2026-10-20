//! 菜单栏与模态对话框（**自绘**）。
//!
//! # 为什么不走原生
//!
//! 三平台各写一套原生实现意味着三份代码、三种行为（macOS 的菜单栏还是全局的），
//! 成本远高于收益。这个项目连笑脸和数码管都是现画的，自绘菜单不违背它的气质，
//! 而且一套代码三平台完全一致。
//!
//! 唯一的例外是**中文输入法**：自绘输入框要自己处理 IME 候选窗，那是真正的坑。
//! 所以文本输入走 [`ChromeEvent::Text`] —— 由宿主把 `winit` 的 IME 提交事件喂进来，
//! 系统负责组字，这里只接收最终文本。
//!
//! # 布局
//!
//! 菜单栏占据窗口客户区**顶部** [`Chrome::BAR_HEIGHT`] 个逻辑像素，游戏画面整体下移。
//! 下移靠 [`crate::raster::Canvas::set_origin`] 的平移变换实现，
//! 所以 `view` 里的坐标一个都不用改。

use crate::raster::Canvas;
use crate::text::TextRenderer;
use crate::theme::{self, Metrics, Rect, Rgb, Size};

/// 菜单命令。ID 沿用原版 `winmine.exe` 的菜单资源编号（阶段 1 取证结果）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    New,
    Beginner,
    Intermediate,
    Expert,
    Custom,
    Marks,
    Colored,
    Sound,
    Scores,
    Exit,
    SizeSmall,
    SizeMedium,
    SizeLarge,
    /// **扩展**：左键单独点一个已翻开的数字也触发快速展开。默认关闭。
    LeftClickChord,
    HelpUsing,
    HelpAbout,
}

impl Command {
    pub fn id(self) -> u32 {
        match self {
            Command::New => 510,
            Command::Beginner => 511,
            Command::Intermediate => 512,
            Command::Expert => 513,
            Command::Custom => 514,
            Command::Marks => 515,
            Command::Colored => 516,
            Command::Sound => 517,
            Command::Scores => 518,
            Command::Exit => 519,
            Command::SizeSmall => 520,
            Command::SizeMedium => 521,
            Command::SizeLarge => 522,
            // 600 起是**扩展**，故意落在原版菜单资源（510–593）之外，
            // 这样代码与文档里一眼能看出「这不是原版的功能」。
            Command::LeftClickChord => 600,
            Command::HelpUsing => 592,
            Command::HelpAbout => 593,
        }
    }
}

#[derive(Clone, Debug)]
pub enum MenuItem {
    Entry {
        cmd: Command,
        label: String,
        checked: bool,
        /// 单选组里的条目画圆点而不是勾。
        radio: bool,
    },
    Separator,
    Submenu {
        label: String,
        items: Vec<MenuItem>,
    },
}

impl MenuItem {
    fn entry(cmd: Command, label: &str) -> MenuItem {
        MenuItem::Entry {
            cmd,
            label: label.to_string(),
            checked: false,
            radio: false,
        }
    }
    fn radio(cmd: Command, label: &str) -> MenuItem {
        MenuItem::Entry {
            cmd,
            label: label.to_string(),
            checked: false,
            radio: true,
        }
    }
    /// 去掉 `\t` 之后的快捷键提示部分。
    fn label_main(&self) -> &str {
        match self {
            MenuItem::Entry { label, .. } | MenuItem::Submenu { label, .. } => {
                label.split('\t').next().unwrap_or(label)
            }
            MenuItem::Separator => "",
        }
    }
    fn label_accel(&self) -> Option<&str> {
        match self {
            MenuItem::Entry { label, .. } | MenuItem::Submenu { label, .. } => {
                label.split('\t').nth(1)
            }
            MenuItem::Separator => None,
        }
    }
}

/// 菜单结构的唯一来源。GUI 与测试都读它，避免两份清单走偏。
pub fn menu_items() -> Vec<MenuItem> {
    use Command::*;
    vec![
        MenuItem::Submenu {
            label: "游戏(&G)".into(),
            items: vec![
                MenuItem::entry(New, "开局(&N)\tF2"),
                MenuItem::Separator,
                MenuItem::entry(Beginner, "初级(&B)"),
                MenuItem::entry(Intermediate, "中级(&I)"),
                MenuItem::entry(Expert, "高级(&E)"),
                MenuItem::entry(Custom, "自定义(&C)"),
                MenuItem::Separator,
                MenuItem::radio(SizeSmall, "小尺寸 16px"),
                MenuItem::radio(SizeMedium, "中尺寸 24px"),
                MenuItem::radio(SizeLarge, "大尺寸 32px"),
                MenuItem::Separator,
                MenuItem::entry(Marks, "标记(?)(&M)"),
                MenuItem::entry(Colored, "颜色(&L)"),
                MenuItem::entry(Sound, "声音(&S)"),
                MenuItem::Separator,
                MenuItem::entry(LeftClickChord, "单击快速展开(&K)"),
                MenuItem::Separator,
                MenuItem::entry(Scores, "扫雷英雄榜(&T)"),
                MenuItem::Separator,
                MenuItem::entry(Exit, "退出(&X)"),
            ],
        },
        MenuItem::Submenu {
            label: "帮助(&H)".into(),
            items: vec![
                MenuItem::entry(HelpUsing, "使用帮助(&H)\tF1"),
                MenuItem::Separator,
                MenuItem::entry(HelpAbout, "关于扫雷(&A)"),
            ],
        },
    ]
}

/// 菜单勾选状态的一份快照。参数收成一个结构体，免得 `sync` 长到五六个 bool。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Options {
    pub marks: bool,
    pub colored: bool,
    pub sound: bool,
    /// 左键单独点已翻开数字也触发快速展开（**扩展**，默认关）。
    pub left_click_chord: bool,
    /// 尺寸档位：0 = 小 16px，1 = 中 24px，2 = 大 32px。
    pub size_index: usize,
}

impl Default for Options {
    fn default() -> Self {
        // 与游戏启动默认一致：三个开关都开、中档、扩展关闭。
        Options {
            marks: true,
            colored: true,
            sound: true,
            left_click_chord: false,
            size_index: 1,
        }
    }
}

/// 宿主喂进来的界面事件。游戏输入不走这里（那是 `view` 的事）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ChromeEvent {
    /// 已提交的文本（IME 组字完成，或直接键入的字符）。
    Text(String),
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
    Enter,
    Escape,
    Tab,
    Click(i32, i32),
    Release(i32, i32),
    Move(i32, i32),
}

/// 一次交互的结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ChromeOutcome {
    /// 事件没被菜单/对话框消费，交给游戏。
    Ignored,
    /// 消费了，需要重绘。
    Redraw,
    Command(Command),
}

/// 对话框关闭时回传的结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum DialogResult {
    Name(String),
    Custom {
        rows: i32,
        cols: i32,
        mines: i32,
    },
    /// 英雄榜里点了「重新计分」。
    ResetScores,
    /// 取消 / Esc / 点了确定但不需要回传数据。
    Dismissed,
}

// ── 帮助与关于文案 ──────────────────────────────────────────────────────────
//
// 放在 mk-core 而不是二进制里：GUI 与 `--ui` 离屏预览必须用同一份文本，
// 否则两边会悄悄走偏（早期 `--ui help` 用的就是一段缩短的副本）。

/// 「帮助 → 使用帮助」/ F1：唯一的帮助条目，承载完整的玩法说明。
pub const HELP_USING: &str = "\
目标
在不触雷的情况下翻开所有非雷格子。翻开所有非雷格即获胜，点中雷即失败。

操作
· 左键翻开格子；第一次点击永远不会踩雷。
· 右键在「旗子 → 问号 → 空」之间循环。
· 数字表示周围 8 格里有几颗雷。

快速展开
· 把鼠标停在一个已翻开的数字上，同时按下左右键，或按下中键。
· 规则一：周围未翻开的格数等于该数字时，把这些格全部标雷（永远安全）。
· 规则二：周围已标旗的格数等于该数字时，翻开其余相邻格。
  旗子插错时规则二会踩雷 —— 这是原版的设定。

快捷键
· F1 打开帮助，F2 开新局。
· 方向键移动焦点框，Enter 或空格翻开，F 插旗。

扩展
· 「游戏 → 单击快速展开」打开后，左键单独点一个数字也能快速展开。
  这是本项目新增的，原版没有；关闭时与原版逐位一致。

帮助对话框
· 打开期间游戏不接收鼠标与键盘。
· 按 Enter 或点「确定」关闭；按 Esc 也可以。";

/// 「帮助 → 关于扫雷」。
pub const ABOUT_TEXT: &str = "\
扫雷（Rust 版）
行为规格复刻自 Windows XP 的 winmine.exe，
方格可在小/中/大三档间切换（16/24/32 像素）。
本程序不含任何微软原版代码或素材，
图形与音效均为代码现画/现合成。

原版作者：Robert Donner 与 Curt Johnson（本处仅为署名致敬）";

// ── 配色（XP 菜单观感）──────────────────────────────────────────────────────

const MENU_BG: Rgb = Rgb(0xF5, 0xF4, 0xEC);
const MENU_SEL_BG: Rgb = Rgb(0x31, 0x6A, 0xC5);
const MENU_SEL_FG: Rgb = Rgb(0xFF, 0xFF, 0xFF);
const MENU_BORDER: Rgb = Rgb(0x9A, 0x9A, 0x9A);
const DIALOG_BG: Rgb = Rgb(0xF0, 0xF0, 0xF0);
const FIELD_BG: Rgb = Rgb(0xFF, 0xFF, 0xFF);

// ── 单行文本编辑 ────────────────────────────────────────────────────────────

#[derive(Default, Clone)]
struct TextField {
    chars: Vec<char>,
    /// 光标位置，单位是**字符**而不是字节。
    caret: usize,
    limit: usize,
    numeric: bool,
}

impl TextField {
    fn new(initial: &str, limit: usize, numeric: bool) -> TextField {
        let chars: Vec<char> = initial.chars().collect();
        let caret = chars.len();
        TextField {
            chars,
            caret,
            limit,
            numeric,
        }
    }
    fn text(&self) -> String {
        self.chars.iter().collect()
    }
    fn value(&self) -> Option<i32> {
        self.text().trim().parse().ok()
    }
    fn insert(&mut self, s: &str) {
        for c in s.chars() {
            if c.is_control() {
                continue;
            }
            if self.numeric && !c.is_ascii_digit() {
                continue;
            }
            if self.chars.len() >= self.limit {
                break;
            }
            self.chars.insert(self.caret, c);
            self.caret += 1;
        }
    }
    fn backspace(&mut self) {
        if self.caret > 0 {
            self.caret -= 1;
            self.chars.remove(self.caret);
        }
    }
    fn delete(&mut self) {
        if self.caret < self.chars.len() {
            self.chars.remove(self.caret);
        }
    }
    fn left(&mut self) {
        self.caret = self.caret.saturating_sub(1);
    }
    fn right(&mut self) {
        self.caret = (self.caret + 1).min(self.chars.len());
    }
    fn home(&mut self) {
        self.caret = 0;
    }
    fn end(&mut self) {
        self.caret = self.chars.len();
    }
}

// ── 对话框 ──────────────────────────────────────────────────────────────────

#[derive(Clone)]
enum DialogKind {
    Message {
        body: String,
    },
    Name {
        prompt: String,
        field: TextField,
    },
    Custom {
        rows: TextField,
        cols: TextField,
        mines: TextField,
        /// 当前焦点字段：0=行 1=列 2=雷数
        focus: usize,
    },
    Scores {
        rows: Vec<(String, String)>,
    },
}

struct Dialog {
    title: String,
    kind: DialogKind,
    w: i32,
    h: i32,
    pressed: Option<usize>,
}

impl Dialog {
    /// 按钮数。
    fn button_count(&self) -> usize {
        match self.kind {
            DialogKind::Message { .. } => 1,
            _ => 2,
        }
    }

    /// 按钮布局：右下角起，依次向左。索引 0 是最右的「确定」。
    fn buttons(&self, frame: Rect) -> Vec<(Rect, &'static str)> {
        let (bw, bh, gap, margin) = (78, 24, 8, 12);
        let n = self.button_count();
        let mut out = Vec::new();
        for i in 0..n {
            let x = frame.right() - margin - (i as i32 + 1) * bw - i as i32 * gap;
            let y = frame.bottom() - margin - bh;
            let label = match (n, i) {
                (_, 0) => "确定",
                (_, 1) => {
                    if matches!(self.kind, DialogKind::Scores { .. }) {
                        "重新计分(&R)"
                    } else {
                        "取消"
                    }
                }
                _ => "",
            };
            out.push((Rect::new(x, y, bw, bh), label));
        }
        out
    }
}

// ── 下拉布局（绘制与命中测试共用同一份几何）────────────────────────────────

struct Dropdown {
    rect: Rect,
    /// 每个**可点击**条目的矩形与它在 items 里的索引。
    entries: Vec<(Rect, usize)>,
}

// ── Chrome ──────────────────────────────────────────────────────────────────

/// 对话框的实际几何与折行结果。绘制与命中测试共用同一份，保证两者永远一致。
struct DialogLayout {
    frame: Rect,
    /// 折行后的正文（只有 Message / Name 使用）。
    lines: Vec<String>,
}

/// 菜单栏 + 对话框的唯一状态机。
pub struct Chrome {
    items: Vec<MenuItem>,
    /// 一级菜单的矩形（命中测试用），由 `layout_bar` 填充。
    top_rects: Vec<Rect>,
    open: Option<usize>,
    hover: Option<usize>,
    dialog: Option<Dialog>,
    dialog_result: Option<DialogResult>,
    font_px: f32,
}

impl Chrome {
    /// 菜单栏高度（逻辑像素）。游戏画面从这一行之下开始。
    pub const BAR_HEIGHT: i32 = 20;
    const ITEM_HEIGHT: i32 = 19;
    const FONT_PX: f32 = 13.0;

    pub fn new(items: Vec<MenuItem>) -> Chrome {
        Chrome {
            items,
            top_rects: Vec::new(),
            open: None,
            hover: None,
            dialog: None,
            dialog_result: None,
            font_px: Self::FONT_PX,
        }
    }

    /// 有没有模态对话框开着（开着时游戏完全不接收输入）。
    pub fn is_modal(&self) -> bool {
        self.dialog.is_some()
    }

    /// 下拉菜单有没有展开（展开时游戏不接收输入）。
    pub fn menu_open(&self) -> bool {
        self.open.is_some()
    }

    /// 这个 y 是否落在菜单栏上。
    pub fn in_bar(&self, y: i32) -> bool {
        y < Self::BAR_HEIGHT
    }

    // ── 打开对话框 ──────────────────────────────────────────────────────────

    pub fn open_message(&mut self, title: &str, body: &str) {
        let lines = body.lines().count().max(1) as i32;
        let h = (24 + lines * 18 + 48).max(112);
        self.dialog = Some(Dialog {
            title: title.to_string(),
            kind: DialogKind::Message {
                body: body.to_string(),
            },
            w: 460,
            h,
            pressed: None,
        });
    }

    pub fn open_name(&mut self, prompt: &str, initial: &str) {
        self.dialog = Some(Dialog {
            title: "扫雷".into(),
            kind: DialogKind::Name {
                prompt: prompt.to_string(),
                field: TextField::new(initial, 24, false),
            },
            w: 286,
            h: 128,
            pressed: None,
        });
    }

    pub fn open_custom(&mut self, rows: i32, cols: i32, mines: i32) {
        self.dialog = Some(Dialog {
            title: "自定义雷区".into(),
            kind: DialogKind::Custom {
                rows: TextField::new(&rows.to_string(), 2, true),
                cols: TextField::new(&cols.to_string(), 2, true),
                mines: TextField::new(&mines.to_string(), 3, true),
                focus: 0,
            },
            w: 232,
            h: 158,
            pressed: None,
        });
    }

    /// `entries` 是 (时间, 姓名)，顺序为初/中/高。
    pub fn open_scores(&mut self, entries: [(String, String); 3]) {
        let labels = ["初级:", "中级:", "高级:"];
        let rows = labels
            .iter()
            .zip(entries)
            .map(|(l, (time, name))| (l.to_string(), format!("{time}  {name}")))
            .collect();
        self.dialog = Some(Dialog {
            title: "扫雷英雄榜".into(),
            kind: DialogKind::Scores { rows },
            w: 300,
            h: 150,
            pressed: None,
        });
    }

    /// 取走对话框结果（若有）。取走后对话框已关闭。
    pub fn take_dialog_result(&mut self) -> Option<DialogResult> {
        self.dialog_result.take()
    }

    fn close_dialog(&mut self, r: DialogResult) {
        self.dialog_result = Some(r);
        self.dialog = None;
    }

    // ── 菜单勾选状态同步 ────────────────────────────────────────────────────

    /// 按当前选项刷新勾选与单选状态。
    pub fn sync(&mut self, o: Options) {
        let Some(MenuItem::Submenu { items, .. }) = self.items.first_mut() else {
            return;
        };
        for it in items.iter_mut() {
            let MenuItem::Entry { cmd, checked, .. } = it else {
                continue;
            };
            match cmd {
                Command::Marks => *checked = o.marks,
                Command::Colored => *checked = o.colored,
                Command::Sound => *checked = o.sound,
                Command::LeftClickChord => *checked = o.left_click_chord,
                Command::SizeSmall => *checked = o.size_index == 0,
                Command::SizeMedium => *checked = o.size_index == 1,
                Command::SizeLarge => *checked = o.size_index == 2,
                _ => {}
            }
        }
    }

    // ── 几何 ────────────────────────────────────────────────────────────────

    /// 菜单栏上一级项的水平布局。同时写回 `top_rects` 供命中测试用。
    fn layout_bar(&mut self, text: &mut TextRenderer) -> Vec<Rect> {
        let mut out = Vec::with_capacity(self.items.len());
        let mut x = 4;
        for it in &self.items {
            let label = it.label_main().to_string();
            let tw = text.measure(&label, self.font_px).width;
            let r = Rect::new(x, 0, tw + 16, Self::BAR_HEIGHT);
            out.push(r);
            x += r.w;
        }
        self.top_rects = out.clone();
        out
    }

    fn layout_dropdown(&self, open: usize, text: &mut TextRenderer) -> Option<Dropdown> {
        let MenuItem::Submenu { items, .. } = self.items.get(open)? else {
            return None;
        };
        let anchor = *self.top_rects.get(open)?;

        let mut content_w = 0;
        for it in items {
            let main = text.measure(it.label_main(), self.font_px).width;
            let accel = it
                .label_accel()
                .map(|a| text.measure(a, self.font_px).width + 28)
                .unwrap_or(0);
            content_w = content_w.max(main + accel);
        }
        let w = content_w + 24 + 16;
        let h: i32 = items
            .iter()
            .map(|i| match i {
                MenuItem::Separator => 7,
                _ => Self::ITEM_HEIGHT,
            })
            .sum();
        let rect = Rect::new(anchor.x, Self::BAR_HEIGHT, w, h);

        let mut entries = Vec::new();
        let mut y = rect.y + 3;
        for (i, it) in items.iter().enumerate() {
            let ih = match it {
                MenuItem::Separator => 7,
                _ => Self::ITEM_HEIGHT,
            };
            if !matches!(it, MenuItem::Separator) {
                entries.push((Rect::new(rect.x + 1, y, rect.w - 2, ih), i));
            }
            y += ih;
        }
        Some(Dropdown { rect, entries })
    }

    /// 对话框的实际几何。
    ///
    /// 尺寸**由折行后的内容决定**，并夹在窗口可用区域之内 —— 早期版本用固定的
    /// `w: 360` 且正文不折行，长句子会直接冲出对话框右边界。
    fn dialog_layout(
        &self,
        win_w: i32,
        win_h: i32,
        text: &mut TextRenderer,
    ) -> Option<DialogLayout> {
        let d = self.dialog.as_ref()?;

        // 可用区域：左右各留 12，别压住菜单栏。
        let avail_w = (win_w - 24).max(180);
        let avail_h = (win_h - Self::BAR_HEIGHT - 16).max(100);

        let w = d.w.min(avail_w);
        // 左右各 16 的内边距，再留 16 给折行的「行首禁则」溢出（收尾标点不甩到下一行）。
        let inner_w = (w - 48).max(40);

        // 只有 Message / Name 有可变长正文，其余是固定布局。
        let lines = match &d.kind {
            DialogKind::Message { body } => text.wrap(body, inner_w, self.font_px),
            DialogKind::Name { prompt, .. } => text.wrap(prompt, inner_w, self.font_px),
            _ => Vec::new(),
        };

        const TITLE_H: i32 = 20;
        const LINE_H: i32 = 18;
        const BTN_H: i32 = 24;
        const FIELD_H: i32 = 22;
        const PAD: i32 = 12;

        // 窗口太小放不下时截断，并在末尾标一个省略号 —— 宁可少显示几行，
        // 也不能让文字画出对话框之外。
        const CHROME_FIXED: i32 = 4 + TITLE_H + 6 + 8 + BTN_H + PAD;
        let max_lines = ((avail_h - CHROME_FIXED) / LINE_H).max(1) as usize;
        let mut lines = lines;
        if lines.len() > max_lines {
            lines.truncate(max_lines);
            if let Some(last) = lines.last_mut() {
                last.push('…');
            }
        }

        let content = lines.len().max(1) as i32 * LINE_H;
        let h = match &d.kind {
            // 上边距 4 + 标题 20 + 间隔 6 + 正文 + 间隔 8 + 按钮 24 + 下边距 12
            DialogKind::Message { .. } => 4 + TITLE_H + 6 + content + 8 + BTN_H + PAD,
            // 正文 + 间隔 8 + 输入框 22 + 间隔 8 + 按钮 24 + 下边距 12
            DialogKind::Name { .. } => 4 + TITLE_H + 6 + content + 8 + FIELD_H + 8 + BTN_H + PAD,
            // 自定义雷区 / 英雄榜：固定布局
            _ => d.h,
        };
        let h = h.min(avail_h).max(80);

        let x = (win_w - w) / 2;
        let y = (Self::BAR_HEIGHT + (win_h - Self::BAR_HEIGHT - h) / 2).max(Self::BAR_HEIGHT);
        Some(DialogLayout {
            frame: Rect::new(x, y, w, h),
            lines,
        })
    }

    // ── 输入 ────────────────────────────────────────────────────────────────

    /// 处理一个界面事件。`width` / `height` 是窗口客户区尺寸（逻辑像素）。
    pub fn handle(
        &mut self,
        ev: ChromeEvent,
        width: i32,
        height: i32,
        text: &mut TextRenderer,
    ) -> ChromeOutcome {
        if self.dialog.is_some() {
            return self.handle_dialog(ev, width, height, text);
        }
        self.handle_menu(ev, width, height, text)
    }

    fn handle_menu(
        &mut self,
        ev: ChromeEvent,
        _width: i32,
        _height: i32,
        text: &mut TextRenderer,
    ) -> ChromeOutcome {
        let tops = self.layout_bar(text);
        match ev {
            ChromeEvent::Click(x, y) => {
                // 点在菜单栏上：切换 / 打开
                if let Some(i) = tops.iter().position(|r| r.contains(x, y)) {
                    self.open = if self.open == Some(i) { None } else { Some(i) };
                    self.hover = None;
                    return ChromeOutcome::Redraw;
                }
                // 点在展开的下拉上
                if let Some(oi) = self.open {
                    if let Some(dd) = self.layout_dropdown(oi, text) {
                        if let Some((_, idx)) = dd.entries.iter().find(|(r, _)| r.contains(x, y)) {
                            let cmd = match self.items.get(oi) {
                                Some(MenuItem::Submenu { items, .. }) => match items.get(*idx) {
                                    Some(MenuItem::Entry { cmd, .. }) => Some(*cmd),
                                    _ => None,
                                },
                                _ => None,
                            };
                            self.open = None;
                            self.hover = None;
                            if let Some(c) = cmd {
                                return ChromeOutcome::Command(c);
                            }
                            return ChromeOutcome::Redraw;
                        }
                        if dd.rect.contains(x, y) {
                            return ChromeOutcome::Redraw; // 点在下拉的空白处，吞掉
                        }
                    }
                }
                // 点在其他地方：收起菜单，并把事件交给游戏
                if self.open.is_some() {
                    self.open = None;
                    self.hover = None;
                    return ChromeOutcome::Redraw;
                }
                ChromeOutcome::Ignored
            }
            ChromeEvent::Move(x, y) => {
                // 鼠标扫过一级项时，若已有菜单打开则跟着切换（XP 行为）
                if let Some(i) = tops.iter().position(|r| r.contains(x, y)) {
                    if self.open.is_some() && self.open != Some(i) {
                        self.open = Some(i);
                        self.hover = None;
                        return ChromeOutcome::Redraw;
                    }
                }
                if let Some(oi) = self.open {
                    if let Some(dd) = self.layout_dropdown(oi, text) {
                        let h = dd
                            .entries
                            .iter()
                            .find(|(r, _)| r.contains(x, y))
                            .map(|(_, i)| *i);
                        if h != self.hover {
                            self.hover = h;
                            return ChromeOutcome::Redraw;
                        }
                    }
                }
                ChromeOutcome::Ignored
            }
            ChromeEvent::Escape => {
                if self.open.is_some() {
                    self.open = None;
                    self.hover = None;
                    return ChromeOutcome::Redraw;
                }
                ChromeOutcome::Ignored
            }
            _ => ChromeOutcome::Ignored,
        }
    }

    fn handle_dialog(
        &mut self,
        ev: ChromeEvent,
        width: i32,
        height: i32,
        text: &mut TextRenderer,
    ) -> ChromeOutcome {
        // 先把几何算完（要 &mut TextRenderer 来量文本），再可变借用 dialog。
        let Some(layout) = self.dialog_layout(width, height, text) else {
            return ChromeOutcome::Ignored;
        };
        let frame = layout.frame;
        let buttons = match self.dialog.as_ref() {
            Some(d) => d.buttons(frame),
            None => return ChromeOutcome::Ignored,
        };

        match ev {
            ChromeEvent::Escape => {
                self.close_dialog(DialogResult::Dismissed);
                ChromeOutcome::Redraw
            }
            ChromeEvent::Enter => {
                let r = self.submit_dialog();
                self.close_dialog(r);
                ChromeOutcome::Redraw
            }
            ChromeEvent::Tab => {
                if let Some(d) = self.dialog.as_mut() {
                    if let DialogKind::Custom { focus, .. } = &mut d.kind {
                        *focus = (*focus + 1) % 3;
                    }
                }
                ChromeOutcome::Redraw
            }
            ChromeEvent::Click(x, y) => {
                let hit = buttons
                    .iter()
                    .position(|(r, _)| r.contains(x, y))
                    .or_else(|| {
                        // 自定义雷区：点某个数值框切换焦点
                        if let Some(d) = self.dialog.as_mut() {
                            if let DialogKind::Custom { focus, .. } = &mut d.kind {
                                for i in 0..3 {
                                    let ly = frame.y + 28 + 4 + i as i32 * 28;
                                    if Rect::new(frame.x + 84, ly + 2, 82, 20).contains(x, y) {
                                        *focus = i;
                                        return Some(usize::MAX);
                                    }
                                }
                            }
                        }
                        None
                    });
                match hit {
                    Some(usize::MAX) => ChromeOutcome::Redraw,
                    Some(i) => {
                        if let Some(d) = self.dialog.as_mut() {
                            d.pressed = Some(i);
                        }
                        ChromeOutcome::Redraw
                    }
                    None => ChromeOutcome::Redraw, // 模态：吞掉框外的点击
                }
            }
            ChromeEvent::Release(x, y) => {
                let pressed = self.dialog.as_ref().and_then(|d| d.pressed);
                if let Some(d) = self.dialog.as_mut() {
                    d.pressed = None;
                }
                let Some(i) = pressed else {
                    return ChromeOutcome::Redraw;
                };
                let still_over = buttons
                    .get(i)
                    .map(|(r, _)| r.contains(x, y))
                    .unwrap_or(false);
                if !still_over {
                    return ChromeOutcome::Redraw;
                }
                let is_primary = i == 0;
                let is_reset = !is_primary
                    && matches!(
                        self.dialog.as_ref().map(|d| &d.kind),
                        Some(DialogKind::Scores { .. })
                    );
                if is_reset {
                    return ChromeOutcome::Redraw;
                }
                if is_primary {
                    let r = self.submit_dialog();
                    self.close_dialog(r);
                } else {
                    self.close_dialog(DialogResult::Dismissed);
                }
                ChromeOutcome::Redraw
            }
            ChromeEvent::Text(s) => {
                self.edit_field(|f| f.insert(&s));
                ChromeOutcome::Redraw
            }
            ChromeEvent::Backspace => {
                self.edit_field(|f| f.backspace());
                ChromeOutcome::Redraw
            }
            ChromeEvent::Delete => {
                self.edit_field(|f| f.delete());
                ChromeOutcome::Redraw
            }
            ChromeEvent::Left => {
                self.edit_field(|f| f.left());
                ChromeOutcome::Redraw
            }
            ChromeEvent::Right => {
                self.edit_field(|f| f.right());
                ChromeOutcome::Redraw
            }
            ChromeEvent::Home => {
                self.edit_field(|f| f.home());
                ChromeOutcome::Redraw
            }
            ChromeEvent::End => {
                self.edit_field(|f| f.end());
                ChromeOutcome::Redraw
            }
            ChromeEvent::Move(_, _) => ChromeOutcome::Redraw, // 模态：不把移动漏给游戏
        }
    }

    fn edit_field(&mut self, f: impl FnOnce(&mut TextField)) {
        let Some(d) = self.dialog.as_mut() else {
            return;
        };
        match &mut d.kind {
            DialogKind::Name { field, .. } => f(field),
            DialogKind::Custom {
                rows,
                cols,
                mines,
                focus,
            } => {
                let target = match focus {
                    0 => rows,
                    1 => cols,
                    _ => mines,
                };
                f(target);
            }
            _ => {}
        }
    }

    /// 点「确定」时的取值。校验不通过则退回「取消」语义。
    fn submit_dialog(&mut self) -> DialogResult {
        let Some(d) = self.dialog.as_ref() else {
            return DialogResult::Dismissed;
        };
        match &d.kind {
            DialogKind::Name { field, .. } => {
                let s = field.text().trim().to_string();
                DialogResult::Name(if s.is_empty() { "匿名".into() } else { s })
            }
            DialogKind::Custom {
                rows, cols, mines, ..
            } => {
                let r = rows.value().unwrap_or(game_min_rows());
                let c = cols.value().unwrap_or(8);
                let m = mines.value().unwrap_or(10);
                DialogResult::Custom {
                    rows: r,
                    cols: c,
                    mines: m,
                }
            }
            _ => DialogResult::Dismissed,
        }
    }

    // ── 绘制 ────────────────────────────────────────────────────────────────

    /// 画菜单栏、展开的下拉、以及对话框（若有）。
    pub fn paint(&mut self, dst: &mut Canvas, width: i32, text: &mut TextRenderer) {
        let tops = self.layout_bar(text);

        dst.fill_rect(Rect::new(0, 0, width, Self::BAR_HEIGHT), MENU_BG);
        dst.fill_rect(Rect::new(0, Self::BAR_HEIGHT - 1, width, 1), theme::SHADOW);

        for (i, (it, r)) in self.items.iter().zip(tops.iter()).enumerate() {
            let label = it.label_main().to_string();
            let selected = self.open == Some(i);
            if selected {
                dst.fill_rect(*r, MENU_SEL_BG);
            }
            let col = if selected { MENU_SEL_FG } else { theme::INK };
            text.draw_centered(dst, *r, &label, self.font_px, col);
        }

        if let Some(oi) = self.open {
            self.paint_dropdown(dst, oi, text);
        }
        // 对话框要最后画（盖在菜单之上）。
        if self.dialog.is_some() {
            if let Some(layout) = self.dialog_layout(width, dst.height(), text) {
                let d = self.dialog.take().expect("刚判断过存在");
                self.paint_dialog(dst, &d, &layout, text);
                self.dialog = Some(d);
            }
        }
    }

    fn paint_dropdown(&self, dst: &mut Canvas, open: usize, text: &mut TextRenderer) {
        let Some(dd) = self.layout_dropdown(open, text) else {
            return;
        };
        let Some(MenuItem::Submenu { items, .. }) = self.items.get(open) else {
            return;
        };

        // 阴影 + 底 + 边框
        dst.fill_rect(
            Rect::new(dd.rect.x + 2, dd.rect.y + 2, dd.rect.w, dd.rect.h),
            theme::SHADOW,
        );
        dst.fill_rect(dd.rect, MENU_BG);
        dst.stroke_rect(dd.rect, 1, MENU_BORDER);

        for (ir, i) in &dd.entries {
            let MenuItem::Entry { checked, radio, .. } = &items[*i] else {
                continue;
            };
            let sel = self.hover == Some(*i);
            if sel {
                dst.fill_rect(*ir, MENU_SEL_BG);
            }
            let fg = if sel { MENU_SEL_FG } else { theme::INK };
            if *checked {
                if *radio {
                    let (cx, cy) = (ir.x + 10, ir.y + ir.h / 2);
                    dst.fill_ellipse(Rect::new(cx - 3, cy - 3, 6, 6), fg, false);
                } else {
                    let (cx, cy) = (ir.x + 5, ir.y + ir.h / 2);
                    dst.stroke_line(cx, cy, cx + 3, cy + 4, 2, fg);
                    dst.stroke_line(cx + 3, cy + 4, cx + 9, cy - 4, 2, fg);
                }
            }
            let main = items[*i].label_main();
            text.draw_in(dst, *ir, main, self.font_px, fg, 22);
            if let Some(a) = items[*i].label_accel() {
                let aw = text.measure(a, self.font_px).width;
                let m = text.measure(a, self.font_px);
                text.draw(
                    dst,
                    ir.right() - 12 - aw,
                    ir.y + (ir.h - m.height()) / 2 + m.ascent,
                    a,
                    self.font_px,
                    fg,
                );
            }
        }

        // 分隔线画在条目之间（上面循环跳过了 Separator）
        let mut y = dd.rect.y + 3;
        for it in items {
            if matches!(it, MenuItem::Separator) {
                dst.fill_rect(
                    Rect::new(dd.rect.x + 4, y + 3, dd.rect.w - 8, 1),
                    MENU_BORDER,
                );
            }
            y += match it {
                MenuItem::Separator => 7,
                _ => Self::ITEM_HEIGHT,
            };
        }
    }

    fn paint_dialog(
        &self,
        dst: &mut Canvas,
        d: &Dialog,
        layout: &DialogLayout,
        text: &mut TextRenderer,
    ) {
        let frame = layout.frame;
        // 投影：对话框常压在浅色棋盘上，没有投影边界会糊在一起。
        dst.fill_rect(
            Rect::new(frame.x + 3, frame.y + 3, frame.w, frame.h),
            theme::SHADOW,
        );
        dst.fill_rect(frame, DIALOG_BG);
        dst.fill_rect(Rect::new(frame.x, frame.y, frame.w, 2), theme::HILITE);
        dst.fill_rect(Rect::new(frame.x, frame.y, 2, frame.h), theme::HILITE);
        dst.fill_rect(
            Rect::new(frame.x, frame.bottom() - 2, frame.w, 2),
            theme::SHADOW,
        );
        dst.fill_rect(
            Rect::new(frame.right() - 2, frame.y, 2, frame.h),
            theme::SHADOW,
        );

        let title_r = Rect::new(frame.x + 4, frame.y + 4, frame.w - 8, 20);
        dst.fill_rect(title_r, MENU_SEL_BG);
        text.draw_in(dst, title_r, &d.title, self.font_px, MENU_SEL_FG, 6);

        let body = Rect::new(frame.x, title_r.bottom() + 6, frame.w, frame.h - 36);
        // 正文左内边距：16，行高 18，基线在行内偏下 13。
        let draw_lines = |dst: &mut Canvas, text: &mut TextRenderer, lines: &[String]| {
            for (i, line) in lines.iter().enumerate() {
                if line.is_empty() {
                    continue;
                }
                let ly = body.y + i as i32 * 18;
                text.draw(dst, body.x + 16, ly + 13, line, self.font_px, theme::INK);
            }
        };
        match &d.kind {
            DialogKind::Message { .. } => {
                draw_lines(dst, text, &layout.lines);
            }
            DialogKind::Name { field, .. } => {
                draw_lines(dst, text, &layout.lines);
                // 输入框紧跟在折行后的正文下面。
                let fy = body.y + layout.lines.len().max(1) as i32 * 18 + 8;
                let fr = Rect::new(body.x + 16, fy, body.w - 32, 22);
                self.paint_field(dst, fr, field, true, text);
            }
            DialogKind::Custom {
                rows,
                cols,
                mines,
                focus,
            } => {
                let labels = ["高度(R):", "宽度(D):", "雷数(M):"];
                let fields = [rows, cols, mines];
                for i in 0..3 {
                    let ly = body.y + i as i32 * 28;
                    text.draw(
                        dst,
                        body.x + 12,
                        ly + 14,
                        labels[i],
                        self.font_px,
                        theme::INK,
                    );
                    let fr = Rect::new(body.x + 84, ly + 2, 82, 20);
                    self.paint_field(dst, fr, fields[i], *focus == i, text);
                    let sr = Rect::new(fr.right(), fr.y, 14, fr.h);
                    dst.fill_rect(sr, theme::FACE);
                    dst.stroke_rect(sr, 1, theme::SHADOW);
                    dst.fill_polygon(
                        &[
                            (sr.x + 4, sr.y + 8),
                            (sr.x + 10, sr.y + 8),
                            (sr.x + 7, sr.y + 3),
                        ],
                        theme::INK,
                    );
                    dst.fill_polygon(
                        &[
                            (sr.x + 4, sr.y + 12),
                            (sr.x + 10, sr.y + 12),
                            (sr.x + 7, sr.y + 17),
                        ],
                        theme::INK,
                    );
                }
            }
            DialogKind::Scores { rows } => {
                for (i, (label, value)) in rows.iter().enumerate() {
                    let ly = body.y + i as i32 * 24;
                    text.draw(dst, body.x + 14, ly + 14, label, self.font_px, theme::INK);
                    text.draw(dst, body.x + 90, ly + 14, value, self.font_px, theme::INK);
                }
            }
        }

        let m = Metrics::new(Size::Small);
        for (i, (r, label)) in d.buttons(frame).iter().enumerate() {
            crate::pix::draw_button(dst, *r, &m, d.pressed == Some(i));
            text.draw_centered(dst, *r, label, self.font_px, theme::INK);
        }
    }

    fn paint_field(
        &self,
        dst: &mut Canvas,
        r: Rect,
        f: &TextField,
        focused: bool,
        text: &mut TextRenderer,
    ) {
        dst.fill_rect(r, FIELD_BG);
        dst.stroke_rect(r, 1, if focused { MENU_SEL_BG } else { MENU_BORDER });
        let s = f.text();
        let inner = Rect::new(r.x + 4, r.y + 1, r.w - 8, r.h - 2);
        let m = text.measure(&s, self.font_px);
        let baseline = inner.y + (inner.h - m.height()) / 2 + m.ascent;
        text.draw(dst, inner.x, baseline, &s, self.font_px, theme::INK);
        if focused {
            let before: String = f.chars[..f.caret].iter().collect();
            let cx = inner.x + text.measure(&before, self.font_px).width;
            dst.fill_rect(Rect::new(cx, r.y + 3, 1, r.h - 6), theme::INK);
        }
    }
}

/// 自定义雷区的行数下限。与 `game::Difficulty::MIN_ROWS` 一致；
/// 这里复制一份是为了不让 chrome 依赖 game（chrome 只管画与输入）。
fn game_min_rows() -> i32 {
    5
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chrome() -> (Chrome, TextRenderer) {
        let mut c = Chrome::new(menu_items());
        c.sync(Options::default());
        (c, TextRenderer::load_default())
    }

    /// 菜单结构：两个顶级菜单，游戏菜单 20 项（含 6 条分隔符），帮助菜单 3 项。
    ///
    /// 比原版多一项 —— 「单击快速展开」，是本项目唯一的**玩法扩展**，见设计文档 §3.3 D7。
    #[test]
    fn menu_shape_matches_the_original() {
        let items = menu_items();
        assert_eq!(items.len(), 2);
        let MenuItem::Submenu { label, items: game } = &items[0] else {
            panic!("第一项应是子菜单");
        };
        assert!(label.starts_with("游戏"));
        assert_eq!(game.len(), 20);
        let MenuItem::Submenu { items: help, .. } = &items[1] else {
            panic!("第二项应是子菜单");
        };
        assert_eq!(help.len(), 3);
    }

    /// 每个 Command 在菜单里恰好出现一次 —— 防止漏接命令。
    #[test]
    fn every_command_appears_exactly_once() {
        fn collect(items: &[MenuItem], out: &mut Vec<Command>) {
            for it in items {
                match it {
                    MenuItem::Entry { cmd, .. } => out.push(*cmd),
                    MenuItem::Submenu { items, .. } => collect(items, out),
                    MenuItem::Separator => {}
                }
            }
        }
        let mut got = Vec::new();
        collect(&menu_items(), &mut got);
        for c in [
            Command::New,
            Command::Beginner,
            Command::Intermediate,
            Command::Expert,
            Command::Custom,
            Command::Marks,
            Command::Colored,
            Command::Sound,
            Command::Scores,
            Command::Exit,
            Command::SizeSmall,
            Command::SizeMedium,
            Command::SizeLarge,
            Command::LeftClickChord,
            Command::HelpUsing,
            Command::HelpAbout,
        ] {
            assert_eq!(
                got.iter().filter(|&&x| x == c).count(),
                1,
                "{c:?} 出现次数不为 1"
            );
        }
    }

    /// 点一级菜单 → 展开；再点同一项 → 收起。
    #[test]
    fn clicking_the_bar_toggles_the_dropdown() {
        let (mut c, mut t) = chrome();
        assert_eq!(
            c.handle(ChromeEvent::Click(10, 8), 800, 700, &mut t),
            ChromeOutcome::Redraw
        );
        assert!(c.menu_open());
        assert_eq!(
            c.handle(ChromeEvent::Click(10, 8), 800, 700, &mut t),
            ChromeOutcome::Redraw
        );
        assert!(!c.menu_open());
    }

    /// 点下拉里的「开局」必须发出 New 命令，并收起菜单。
    #[test]
    fn clicking_new_game_emits_the_command() {
        let (mut c, mut t) = chrome();
        c.handle(ChromeEvent::Click(10, 8), 800, 700, &mut t);
        let dd = c.layout_dropdown(0, &mut t).expect("下拉应有布局");
        let (r, _) = dd.entries[0]; // 第 0 项是「开局」
        let out = c.handle(ChromeEvent::Click(r.x + 5, r.y + 5), 800, 700, &mut t);
        assert_eq!(out, ChromeOutcome::Command(Command::New));
        assert!(!c.menu_open(), "发出命令后菜单应收起");
    }

    /// 点下拉里的「退出」必须发出 Exit。
    #[test]
    fn clicking_exit_emits_exit() {
        let (mut c, mut t) = chrome();
        c.handle(ChromeEvent::Click(10, 8), 800, 700, &mut t);
        let dd = c.layout_dropdown(0, &mut t).unwrap();
        // 最后一项
        let (r, idx) = *dd.entries.last().unwrap();
        let items = menu_items();
        let MenuItem::Submenu { items: game, .. } = &items[0] else {
            unreachable!()
        };
        let MenuItem::Entry { cmd: last, .. } = &game[idx] else {
            unreachable!()
        };
        assert_eq!(*last, Command::Exit);
        let out = c.handle(ChromeEvent::Click(r.x + 5, r.y + 5), 800, 700, &mut t);
        assert_eq!(out, ChromeOutcome::Command(Command::Exit));
    }

    /// 菜单展开时，落在游戏区域的点击必须被吞掉（不能穿透）。
    #[test]
    fn open_menu_swallows_clicks_outside() {
        let (mut c, mut t) = chrome();
        c.handle(ChromeEvent::Click(10, 8), 800, 700, &mut t);
        let out = c.handle(ChromeEvent::Click(400, 400), 800, 700, &mut t);
        assert_eq!(out, ChromeOutcome::Redraw, "展开时应吞掉外部点击");
        assert!(!c.menu_open(), "外部点击应收起菜单");
    }

    /// 菜单收起时，落在游戏区域的点击必须放行。
    #[test]
    fn closed_menu_lets_game_clicks_through() {
        let (mut c, mut t) = chrome();
        let out = c.handle(ChromeEvent::Click(400, 400), 800, 700, &mut t);
        assert_eq!(out, ChromeOutcome::Ignored);
    }

    /// 模态对话框必须吞掉所有点击。
    #[test]
    fn modal_dialog_swallows_everything() {
        let (mut c, mut t) = chrome();
        c.open_message("关于扫雷", "MineKeeper\n测试");
        assert!(c.is_modal());
        let out = c.handle(ChromeEvent::Click(5, 5), 800, 700, &mut t);
        assert_eq!(out, ChromeOutcome::Redraw);
        assert!(c.is_modal(), "点框外不该关掉对话框");
    }

    /// 消息框点「确定」应关闭。
    #[test]
    fn message_dialog_ok_closes_it() {
        let (mut c, mut t) = chrome();
        c.open_message("关于扫雷", "MineKeeper");
        let frame = c.dialog_layout(800, 700, &mut t).unwrap().frame;
        let (ok, _) = c.dialog.as_ref().unwrap().buttons(frame)[0];
        c.handle(ChromeEvent::Click(ok.x + 5, ok.y + 5), 800, 700, &mut t);
        c.handle(ChromeEvent::Release(ok.x + 5, ok.y + 5), 800, 700, &mut t);
        assert!(!c.is_modal());
        assert_eq!(c.take_dialog_result(), Some(DialogResult::Dismissed));
    }

    /// Esc 取消对话框。
    #[test]
    fn escape_cancels_the_dialog() {
        let (mut c, mut t) = chrome();
        c.open_name("请留尊姓大名。", "匿名");
        c.handle(ChromeEvent::Escape, 800, 700, &mut t);
        assert!(!c.is_modal());
        assert_eq!(c.take_dialog_result(), Some(DialogResult::Dismissed));
    }

    /// 姓名输入：键入 + 退格 + 回车提交。
    #[test]
    fn name_dialog_edits_and_submits() {
        let (mut c, mut t) = chrome();
        c.open_name("请留尊姓大名。", "");
        c.handle(ChromeEvent::Text("张三".into()), 800, 700, &mut t);
        c.handle(ChromeEvent::Text("丰".into()), 800, 700, &mut t);
        c.handle(ChromeEvent::Backspace, 800, 700, &mut t);
        c.handle(ChromeEvent::Enter, 800, 700, &mut t);
        assert_eq!(
            c.take_dialog_result(),
            Some(DialogResult::Name("张三".into()))
        );
    }

    /// 姓名留空 → 匿名。
    #[test]
    fn empty_name_becomes_anonymous() {
        let (mut c, mut t) = chrome();
        c.open_name("请留尊姓大名。", "");
        c.handle(ChromeEvent::Enter, 800, 700, &mut t);
        assert_eq!(
            c.take_dialog_result(),
            Some(DialogResult::Name("匿名".into()))
        );
    }

    /// 姓名长度上限 24。
    #[test]
    fn name_is_length_limited() {
        let (mut c, mut t) = chrome();
        c.open_name("x", "");
        c.handle(ChromeEvent::Text("字".repeat(100)), 800, 700, &mut t);
        c.handle(ChromeEvent::Enter, 800, 700, &mut t);
        let Some(DialogResult::Name(n)) = c.take_dialog_result() else {
            panic!("应有姓名结果");
        };
        assert_eq!(n.chars().count(), 24);
    }

    /// 自定义雷区：数值框只收数字。
    #[test]
    fn custom_fields_are_numeric() {
        let (mut c, mut t) = chrome();
        c.open_custom(9, 9, 10);
        c.handle(ChromeEvent::Text("abc7".into()), 800, 700, &mut t);
        c.handle(ChromeEvent::Enter, 800, 700, &mut t);
        let Some(DialogResult::Custom { rows, .. }) = c.take_dialog_result() else {
            panic!("应有自定义结果");
        };
        assert_eq!(rows, 97, "字母应被丢掉，只留数字");
    }

    /// 每个对话框都必须能画出来且不 panic。
    #[test]
    fn all_dialogs_render() {
        let mut t = TextRenderer::load_default();
        if t.is_empty() {
            return;
        }
        type Setup = Box<dyn Fn(&mut Chrome)>;
        let mut cases: Vec<Setup> = vec![
            Box::new(|c: &mut Chrome| c.open_message("关于扫雷", "第一行\n第二行\n第三行")),
            Box::new(|c: &mut Chrome| c.open_name("请留尊姓大名。", "匿名")),
            Box::new(|c: &mut Chrome| c.open_custom(24, 30, 199)),
            Box::new(|c: &mut Chrome| {
                c.open_scores([
                    ("12 秒".into(), "甲".into()),
                    ("45 秒".into(), "乙".into()),
                    ("99 秒".into(), "丙".into()),
                ])
            }),
        ];
        for setup in cases.drain(..) {
            let mut c = Chrome::new(menu_items());
            setup(&mut c);
            let mut cv = Canvas::new(800, 700).unwrap();
            cv.clear(theme::FACE);
            c.paint(&mut cv, 800, &mut t);
            // 标题条是蓝色，用它证明对话框真的画上去了。
            let (_, _, w, h) = cv.ink_bounds();
            assert!(w > 100 && h > 100, "画面几乎空白");
        }
    }

    /// 帮助菜单只保留「使用帮助」与「关于扫雷」两项，且内容互不相同。
    #[test]
    fn help_menu_has_two_distinct_entries() {
        let items = menu_items();
        let MenuItem::Submenu { items: help, .. } = &items[1] else {
            panic!("第二个顶级菜单应是帮助");
        };
        let entries: Vec<&Command> = help
            .iter()
            .filter_map(|i| match i {
                MenuItem::Entry { cmd, .. } => Some(cmd),
                _ => None,
            })
            .collect();
        assert_eq!(entries, vec![&Command::HelpUsing, &Command::HelpAbout]);
        assert_ne!(super::HELP_USING, super::ABOUT_TEXT, "帮助与关于内容相同");
        assert!(!super::HELP_USING.trim().is_empty());
    }

    /// 对话框无论正文多长、窗口多小，都不得画到窗口之外。
    #[test]
    fn dialog_layout_always_fits_inside_the_window() {
        let mut t = TextRenderer::load_default();
        if t.is_empty() {
            return;
        }
        let mut c = Chrome::new(menu_items());
        c.open_message("标题", &"很长的正文。".repeat(200));
        for (w, h) in [(747, 688), (273, 316), (320, 240), (200, 140)] {
            let l = c.dialog_layout(w, h, &mut t).expect("应有布局");
            assert!(l.frame.x >= 0, "{w}x{h} 左边越界");
            assert!(
                l.frame.right() <= w,
                "{w}x{h} 右边越界（{} > {w}）",
                l.frame.right()
            );
            assert!(l.frame.y >= Chrome::BAR_HEIGHT, "{w}x{h} 压住菜单栏");
            assert!(
                l.frame.bottom() <= h,
                "{w}x{h} 下边越界（{} > {h}）",
                l.frame.bottom()
            );
        }
    }

    /// 菜单栏与下拉都必须画出可见内容。
    #[test]
    fn menu_bar_renders() {
        let mut t = TextRenderer::load_default();
        if t.is_empty() {
            return;
        }
        let (mut c, _) = chrome();
        let mut cv = Canvas::new(800, 700).unwrap();
        cv.clear(theme::FACE);
        c.paint(&mut cv, 800, &mut t);
        // 菜单栏区域应该有非面色的像素
        let mut ink = 0;
        for y in 0..Chrome::BAR_HEIGHT {
            for x in 0..200 {
                let i = ((y as u32 * 800 + x as u32) * 4) as usize;
                let px = (cv.data()[i], cv.data()[i + 1], cv.data()[i + 2]);
                if px != (theme::FACE.0, theme::FACE.1, theme::FACE.2) {
                    ink += 1;
                }
            }
        }
        assert!(ink > 50, "菜单栏几乎是空的（{ink} 个非底色像素）");
    }
}
