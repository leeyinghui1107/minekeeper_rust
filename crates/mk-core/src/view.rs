//! 「绘制 + 输入」的唯一状态机。
//!
//! 整块画面（状态栏 + 雷区）都在这里画；输入也在这里处理，因为左右键同按 / 中键的
//! chord 检测**必须和按下态渲染共用一份状态**。
//!
//! 移植自 Go 版 `internal/view/view.go`。这一层不认识窗口、不认识消息循环 ——
//! 宿主只要把 [`Button`] / [`Key`] 事件投进来，再调用 [`View::paint`] 即可。

use std::collections::HashSet;

use crate::game::{self, Board, Difficulty, Mark, Mood, Phase, Pos, Rand};
use crate::pix;
use crate::raster::Canvas;
use crate::theme::{self, Metrics, Rect, Size};

/// 鼠标键。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Button {
    Left,
    Right,
    Middle,
}

/// 面板认得的按键。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Space,
    F,
    /// 面板本身不处理 F1 / F2（那是宿主层的「帮助」与「开局」），
    /// 但它们要能穿过这一层，所以在这里占位。
    F1,
    F2,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Mode {
    #[default]
    None,
    Left,
    Right,
    Chord,
}

/// View 要求宿主播放的提示音。View 不持有音频设备，只抛事件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SoundCue {
    Open,
    Boom,
    Cheer,
}

/// 给宿主窗口的回调。
#[derive(Default)]
pub struct Events {
    pub first_move_made: Option<Box<dyn FnMut()>>,
    pub game_finished: Option<Box<dyn FnMut(bool)>>,
    pub new_game_requested: Option<Box<dyn FnMut()>>,
    /// 指针捕获开关。宿主必须实现 —— 见设计文档 §6.2 约束 3。
    pub capture: Option<Box<dyn FnMut(bool)>>,
    pub play_sound: Option<Box<dyn FnMut(SoundCue)>>,
}

/// 面板状态。
pub struct View {
    board: Board,
    m: Metrics,
    pub events: Events,

    pub allow_question: bool,
    pub colored: bool,
    /// **扩展**：左键单独点一个已翻开的数字也触发快速展开。默认 `false`，
    /// 此时行为与原版逐位一致（左键点已翻开的格子什么都不做）。
    pub left_click_chord: bool,

    left_down: bool,
    right_down: bool,
    middle_down: bool,
    pressed: Pos,
    mode: Mode,
    smiley_down: bool,

    focus: Pos,
    show_focus: bool,

    press_set: HashSet<Pos>,
    chord_mines: HashSet<Pos>,
    chord_source: Pos,
    overlay_shown: HashSet<Pos>,

    dirty: Vec<Rect>,
    repaint_all: bool,
}

impl View {
    /// 建一个默认高级难度、中尺寸（24px 单元格）的面板状态。
    pub fn new() -> View {
        View {
            board: Board::new(
                Difficulty::EXPERT.rows,
                Difficulty::EXPERT.cols,
                Difficulty::EXPERT.mines,
                Rand::new(0),
            ),
            m: Metrics::new(Size::Medium),
            events: Events::default(),
            allow_question: true,
            colored: true,
            left_click_chord: false,
            left_down: false,
            right_down: false,
            middle_down: false,
            pressed: Pos::NONE,
            mode: Mode::None,
            smiley_down: false,
            focus: Pos::new(0, 0),
            show_focus: false,
            press_set: HashSet::new(),
            chord_mines: HashSet::new(),
            chord_source: Pos::NONE,
            overlay_shown: HashSet::new(),
            dirty: Vec::new(),
            repaint_all: false,
        }
    }

    pub fn board(&self) -> &Board {
        &self.board
    }
    pub fn board_mut(&mut self) -> &mut Board {
        &mut self.board
    }
    pub fn metrics(&self) -> Metrics {
        self.m
    }
    pub fn client_width(&self) -> i32 {
        self.m.client_width(self.board.cols)
    }
    pub fn client_height(&self) -> i32 {
        self.m.client_height(self.board.rows)
    }

    /// 换方格尺寸并重算尺寸。
    pub fn apply_size(&mut self, size: Size) {
        self.m = Metrics::new(size);
        self.invalidate_all();
    }

    /// 按难度开局。
    pub fn start_game(&mut self, d: Difficulty, seed: u64) {
        self.board = Board::new(d.rows, d.cols, d.mines, Rand::new(seed));
        self.pressed = Pos::NONE;
        self.mode = Mode::None;
        self.focus = Pos::new(0, 0);
        self.show_focus = false;
        self.rebuild_press_set();
        self.invalidate_all();
    }

    /// 把面板置于「按住做快速展开」的预览态，供离屏渲染与输入路径测试使用。
    pub fn preview_chord(&mut self, at: Pos) {
        self.pressed = at;
        self.mode = if at.is_valid() {
            Mode::Chord
        } else {
            Mode::None
        };
        self.rebuild_press_set();
    }

    /// 计时器每秒跳一格，上限 999（原版语义）。
    pub fn tick_second(&mut self) {
        self.board.seconds = (self.board.seconds + 1).min(999);
        let r = self.m.led_right_rect(self.board.cols);
        self.invalidate(r);
    }

    // ── 命中测试 ────────────────────────────────────────────────────────────

    fn cell_at(&self, pt: (i32, i32)) -> Pos {
        let p = Pos::new(
            self.m.row_at(pt.1).unwrap_or(-1),
            self.m.col_at(pt.0).unwrap_or(-1),
        );
        if self.board.in_bounds(p) {
            p
        } else {
            Pos::NONE
        }
    }

    fn smiley_rect(&self) -> Rect {
        self.m.smiley_rect(self.board.cols)
    }
    fn led_left_rect(&self) -> Rect {
        self.m.led_left_rect()
    }
    fn led_right_rect(&self) -> Rect {
        self.m.led_right_rect(self.board.cols)
    }
    fn status_rect(&self) -> Rect {
        self.m.status_rect(self.board.cols)
    }
    fn on_smiley(&self, pt: (i32, i32)) -> bool {
        self.smiley_rect().contains(pt.0, pt.1)
    }

    // ── 鼠标 ────────────────────────────────────────────────────────────────

    /// 处理一次按下。原始鼠标消息由宿主逐条投进来，
    /// 因此「一个键已按下时再来第二个键」不会被吞掉 —— 这是 chord 的全部依赖。
    pub fn button_down(&mut self, b: Button, pt: (i32, i32)) {
        match b {
            Button::Left => self.left_down = true,
            Button::Right => self.right_down = true,
            Button::Middle => self.middle_down = true,
        }
        self.show_focus = false;

        if matches!(self.board.phase, Phase::Dead | Phase::Won) {
            if b == Button::Left && self.on_smiley(pt) {
                self.smiley_down = true;
                let r = self.smiley_rect();
                self.invalidate(r);
            }
            return;
        }

        if b == Button::Left && self.on_smiley(pt) {
            self.smiley_down = true;
            self.capture(true);
            let r = self.smiley_rect();
            self.invalidate(r);
            return;
        }

        let cell = self.cell_at(pt);
        if !cell.is_valid() {
            return;
        }
        self.pressed = cell;
        self.capture(true);
        self.update_press_mode();
    }

    /// 处理移动：拖到笑脸要取消按下态（原版行为）。
    pub fn mouse_move(&mut self, pt: (i32, i32)) {
        if self.mode == Mode::None {
            if self.smiley_down && !self.on_smiley(pt) {
                let old = self.smiley_rect();
                self.smiley_down = false;
                self.invalidate(old);
            }
            return;
        }

        if self.on_smiley(pt) {
            let visual = self.press_visual_cells(self.pressed);
            self.mode = Mode::None;
            self.pressed = Pos::NONE;
            self.rebuild_press_set();
            self.invalidate_cells(&visual);
            self.smiley_down = true;
            let r = self.smiley_rect();
            self.invalidate(r);
            return;
        }

        let cell = self.cell_at(pt);
        if cell != self.pressed {
            let mut affected = self.press_visual_cells(self.pressed);
            self.pressed = cell;
            affected.extend(self.press_visual_cells(cell));
            self.rebuild_press_set();
            for p in game::distinct(&affected) {
                self.invalidate_cell(p);
            }
        }
    }

    /// 处理抬起，并在这里真正落子。
    pub fn button_up(&mut self, b: Button, pt: (i32, i32)) {
        match b {
            Button::Left => self.left_down = false,
            Button::Right => self.right_down = false,
            Button::Middle => self.middle_down = false,
        }
        self.capture(false);

        if self.smiley_down {
            let over = self.on_smiley(pt);
            self.smiley_down = false;
            let r = self.smiley_rect();
            self.invalidate(r);
            if over {
                if let Some(cb) = self.events.new_game_requested.as_mut() {
                    cb();
                }
            }
            return;
        }

        let cell = self.pressed;
        let m = self.mode;
        let visual = self.press_visual_cells(cell);

        self.mode = Mode::None;
        self.pressed = Pos::NONE;
        self.rebuild_press_set();

        if m == Mode::None {
            return;
        }
        self.invalidate_cells(&visual);
        if !cell.is_valid() || self.cell_at(pt) != cell {
            return;
        }

        match m {
            Mode::Chord => {
                let res = self.board.apply_chord(cell);
                self.apply(res);
            }
            Mode::Left if b == Button::Left => self.do_open(cell),
            Mode::Right if b == Button::Right => self.do_mark(cell),
            _ => {}
        }
    }

    /// 指针离开客户区：松开按下态。
    pub fn mouse_leave(&mut self) {
        if self.mode == Mode::None && !self.smiley_down {
            return;
        }
        let visual = self.press_visual_cells(self.pressed);
        self.mode = Mode::None;
        self.pressed = Pos::NONE;
        self.smiley_down = false;
        self.rebuild_press_set();
        self.invalidate_cells(&visual);
        let r = self.smiley_rect();
        self.invalidate(r);
    }

    fn update_press_mode(&mut self) {
        let old_visual = self.press_visual_cells(self.pressed);

        // 「单击快速展开」：左键单独按在一个**已翻开的数字**上时直接进 chord 模式。
        //
        // 这里不必排除 `right_down` —— 左右同按在上面那条已经进了 Chord，结果相同。
        // 开关关闭时这个表达式恒为 false，所以原版路径一个字节都没变。
        //
        // 注意模式**只在按下时判定**：`mouse_move` 不会重算，所以从别的格子拖进来
        // 不会中途变成 chord。这是有意的 —— 免得「本来想翻开、拖了一下变成 chord」。
        let chord_by_left_click = self.left_click_chord
            && self.left_down
            && self.pressed.is_valid()
            && self.board.is_open(self.pressed);

        self.mode =
            if self.middle_down || (self.left_down && self.right_down) || chord_by_left_click {
                Mode::Chord
            } else if self.left_down {
                Mode::Left
            } else if self.right_down {
                Mode::Right
            } else {
                Mode::None
            };

        if self.mode == Mode::None {
            self.pressed = Pos::NONE;
            self.rebuild_press_set();
            self.invalidate_all();
            return;
        }

        self.rebuild_press_set();
        self.invalidate_cells(&old_visual);
        let cur = self.press_visual_cells(self.pressed);
        self.invalidate_cells(&cur);
        let r = self.smiley_rect();
        self.invalidate(r);
    }

    /// 按下态在视觉上受影响的格子（chord 会连带把周围未插旗的格子一起压下去）。
    fn press_visual_cells(&self, at: Pos) -> Vec<Pos> {
        let mut list = Vec::new();
        if !at.is_valid() {
            return list;
        }
        list.push(at);
        if self.mode == Mode::Chord && self.board.is_open(at) && self.board.adjacent(at) > 0 {
            list.extend(game::reveal_targets(&self.board, at));
        }
        list
    }

    fn rebuild_press_set(&mut self) {
        self.press_set.clear();
        self.chord_mines.clear();
        self.chord_source = Pos::NONE;

        if self.mode != Mode::None {
            for p in self.press_visual_cells(self.pressed) {
                self.press_set.insert(p);
            }
            if self.mode == Mode::Chord {
                self.chord_source = self.pressed;
                for p in self.board.certain_mines(self.pressed) {
                    self.chord_mines.insert(p);
                }
            }
        }

        let next: HashSet<Pos> = self.chord_overlay().into_iter().collect();
        // 上一帧画过的框必须一起失效，否则松手后会留下残影。
        let stale: Vec<Pos> = self.overlay_shown.iter().copied().collect();
        for p in stale {
            self.invalidate_cell(p);
        }
        for &p in &next {
            self.invalidate_cell(p);
        }
        self.overlay_shown = next;
    }

    fn chord_overlay(&self) -> Vec<Pos> {
        let mut out = Vec::new();
        if self.mode != Mode::Chord || !self.chord_source.is_valid() {
            return out;
        }
        out.push(self.chord_source);
        out.extend(self.chord_mines.iter().copied());
        out
    }

    fn is_visually_pressed(&self, p: Pos) -> bool {
        match self.mode {
            Mode::Left => self.press_set.contains(&p) && !self.board.is_open(p),
            Mode::Chord => self.press_set.contains(&p),
            _ => false,
        }
    }

    fn do_open(&mut self, p: Pos) {
        let res = self.board.left_click(p);
        if res.started {
            if let Some(cb) = self.events.first_move_made.as_mut() {
                cb();
            }
        }
        self.apply(res);
    }

    fn do_mark(&mut self, p: Pos) {
        let res = self.board.cycle_mark(p, self.allow_question);
        self.apply(res);
    }

    fn apply(&mut self, res: game::MoveResult) {
        if !res.changed.is_empty() {
            self.invalidate_cells(&res.changed);
        }
        let sr = self.status_rect();
        self.invalidate(sr);
        if res.boom || res.won {
            if res.won {
                self.play(SoundCue::Cheer);
            }
            if res.boom {
                self.play(SoundCue::Boom);
            }
            self.invalidate_all();
            if let Some(cb) = self.events.game_finished.as_mut() {
                cb(res.won);
            }
        } else if !res.changed.is_empty() {
            self.play(SoundCue::Open);
        }
    }

    fn play(&mut self, cue: SoundCue) {
        if let Some(cb) = self.events.play_sound.as_mut() {
            cb(cue);
        }
    }

    // ── 键盘 ────────────────────────────────────────────────────────────────

    /// 处理方向键 / Enter / 空格 / F。
    pub fn key_down(&mut self, k: Key) {
        let (mut r, mut c) = (self.focus.row, self.focus.col);

        match k {
            Key::Up => r = (r - 1).max(0),
            Key::Down => r = (r + 1).min(self.board.rows - 1),
            Key::Left => c = (c - 1).max(0),
            Key::Right => c = (c + 1).min(self.board.cols - 1),
            Key::Enter | Key::Space => {
                if self.show_focus {
                    self.do_open(self.focus);
                    return;
                }
                self.show_focus = true;
                self.invalidate_cell(self.focus);
                return;
            }
            Key::F => {
                if self.show_focus {
                    self.do_mark(self.focus);
                }
                return;
            }
            // 宿主层的命令，面板不处理。
            Key::F1 | Key::F2 => return,
        }

        let old = self.focus;
        self.focus = Pos::new(r, c);
        self.show_focus = true;
        self.invalidate_cell(old);
        self.invalidate_cell(self.focus);
    }

    // ── 绘制 ────────────────────────────────────────────────────────────────

    /// 把画面画进 `dst`（离屏渲染与窗口绘制共用同一套代码）。
    pub fn paint(&self, dst: &mut Canvas, clip: Rect, font: &mut pix::FontStack) {
        dst.clear(theme::FACE);
        self.draw_status(dst, font);
        self.draw_field(dst, clip, font);
    }

    fn draw_status(&self, dst: &mut Canvas, font: &mut pix::FontStack) {
        let sr = self.status_rect();
        pix::draw_sunken_rect(dst, sr, self.m.status_border_px());
        pix::draw_led(dst, self.led_left_rect(), self.board.flags_left(), &self.m);
        pix::draw_led(dst, self.led_right_rect(), self.board.seconds, &self.m);
        self.draw_smiley(dst, font);
    }

    fn draw_smiley(&self, dst: &mut Canvas, font: &mut pix::FontStack) {
        let rect = self.smiley_rect();
        pix::draw_button(dst, rect, &self.m, self.smiley_down);
        let fr = self.m.frame_px();
        let face = rect.inset(fr);
        pix::draw_face(dst, face, self.current_mood(), &self.m);
        let _ = font;
    }

    fn current_mood(&self) -> Mood {
        if self.board.phase == Phase::Dead {
            return Mood::Dead;
        }
        if self.board.phase == Phase::Won {
            return Mood::Cool;
        }
        if self.smiley_down {
            return Mood::Pressed;
        }
        if matches!(self.mode, Mode::Left | Mode::Chord)
            && self.pressed.is_valid()
            && self.touches_mine(self.pressed)
        {
            return Mood::Concentrate;
        }
        Mood::Normal
    }

    fn touches_mine(&self, p: Pos) -> bool {
        if !self.board.field_placed() {
            return false;
        }
        game::neighbors(self.board.rows, self.board.cols, p).any(|n| self.board.is_mine(n))
    }

    fn draw_field(&self, dst: &mut Canvas, clip: Rect, font: &mut pix::FontStack) {
        let fr = self.m.field_rect(self.board.cols, self.board.rows);
        pix::draw_sunken_rect(dst, fr, self.m.field_border_px());

        let first_row = self.m.row_at(clip.y).unwrap_or(0).saturating_sub(1).max(0);
        let last_row = self
            .m
            .row_at(clip.bottom())
            .unwrap_or(self.board.rows)
            .min(self.board.rows - 1);
        let first_col = self.m.col_at(clip.x).unwrap_or(0).saturating_sub(1).max(0);
        let last_col = self
            .m
            .col_at(clip.right())
            .unwrap_or(self.board.cols)
            .min(self.board.cols - 1);

        for r in first_row..=last_row {
            for c in first_col..=last_col {
                let rect = self.m.cell_rect(c, r);
                if !rect.overlaps(&clip) {
                    continue;
                }
                self.draw_cell(dst, Pos::new(r, c), rect, font);
            }
        }

        self.draw_chord_overlay(dst, clip);

        if self.show_focus && self.focus.is_valid() {
            let fr = self.m.cell_rect(self.focus.col, self.focus.row);
            let pen = self.m.px(1).max(1);
            let inset = self.m.px(1);
            dst.stroke_rect(
                Rect::new(
                    fr.x + inset,
                    fr.y + inset,
                    fr.w - 2 * inset - 1,
                    fr.h - 2 * inset - 1,
                ),
                pen,
                theme::INK,
            );
        }
    }

    /// 画按住中键 / 左右键时的推断叠加层：源格描橙框，
    /// 能被单格约束确定为雷的未翻开格描红框。只是提示，不改变格子状态。
    fn draw_chord_overlay(&self, dst: &mut Canvas, clip: Rect) {
        if self.mode != Mode::Chord || !self.chord_source.is_valid() {
            return;
        }
        let inset = self.m.px(2);
        let pen = self.m.px(1).max(2);

        let src = self
            .m
            .cell_rect(self.chord_source.col, self.chord_source.row);
        if src.overlaps(&clip) {
            dst.stroke_rect(
                Rect::new(src.x + 1, src.y + 1, src.w - 3, src.h - 3),
                pen,
                theme::CHORD_SOURCE,
            );
        }
        for &p in &self.chord_mines {
            let r = self.m.cell_rect(p.col, p.row);
            if !r.overlaps(&clip) {
                continue;
            }
            dst.stroke_rect(
                Rect::new(r.x + inset, r.y + inset, r.w - 2 * inset, r.h - 2 * inset),
                pen,
                theme::CHORD_MINE,
            );
        }
    }

    fn draw_cell(&self, dst: &mut Canvas, p: Pos, rect: Rect, font: &mut pix::FontStack) {
        pix::fill_face(dst, rect);
        let sunken = self.board.is_sunken(p) || self.is_visually_pressed(p);

        if sunken {
            pix::draw_sunken_cell(dst, rect, &self.m);
            if self.board.is_exploded(p) {
                let t = self.m.frame_px();
                dst.fill_rect(rect.inset(t), theme::FLAG_RED);
            }
            if self.board.shows_mine(p) {
                pix::draw_mine(dst, rect, &self.m);
                if self.board.shows_wrong_flag(p) {
                    pix::draw_wrong_flag_cross(dst, rect, &self.m);
                }
            } else if self.board.number_at(p) > 0 {
                self.draw_number(dst, rect, self.board.number_at(p) as u8, font);
            }
            return;
        }

        pix::draw_raised_cell(dst, rect, &self.m);
        match self.board.mark_of(p) {
            Mark::Flag => pix::draw_flag(dst, rect, &self.m),
            Mark::Question => {
                let col = if self.colored {
                    theme::QUESTION_BLUE
                } else {
                    theme::INK
                };
                pix::draw_question(dst, rect, col, &self.m, font);
            }
            Mark::None => {}
        }
    }

    fn draw_number(&self, dst: &mut Canvas, rect: Rect, n: u8, font: &mut pix::FontStack) {
        let col = theme::digit_color(n as usize, self.colored);
        let target_w = self.m.px(theme::glyph_width(n));
        let ch = char::from_digit(n as u32, 10).unwrap();
        // 颜色参与精灵缓存键，所以不必事后换色。
        if let Some(sp) = font.render_glyph(ch, rect.w, &self.m, target_w, col) {
            dst.blit((rect.x, rect.y), &sp);
        }
    }

    // ── 失效 ────────────────────────────────────────────────────────────────

    fn invalidate_cell(&mut self, p: Pos) {
        if !p.is_valid() {
            return;
        }
        let r = self.m.cell_rect(p.col, p.row);
        self.invalidate(r);
    }

    fn invalidate_cells(&mut self, cells: &[Pos]) {
        for &p in cells {
            self.invalidate_cell(p);
        }
    }

    fn invalidate(&mut self, r: Rect) {
        self.dirty.push(r);
    }

    fn invalidate_all(&mut self) {
        self.repaint_all = true;
        self.dirty.clear();
    }

    /// 请求整块客户区重画（换尺寸、换难度、开局、切颜色时用）。
    pub fn invalidate_all_pub(&mut self) {
        self.invalidate_all();
    }

    /// 取走并清空待重绘矩形；第二个返回值表示整块重画。
    pub fn take_dirty(&mut self) -> (Vec<Rect>, bool) {
        let d = std::mem::take(&mut self.dirty);
        let all = self.repaint_all;
        self.repaint_all = false;
        (d, all)
    }

    fn capture(&mut self, on: bool) {
        if let Some(cb) = self.events.capture.as_mut() {
            cb(on);
        }
    }
}

impl Default for View {
    fn default() -> Self {
        View::new()
    }
}

/// 供自检与场景脚本使用的辅助：按行优先找第一个满足条件的格子。
pub fn find_cell(b: &Board, mut pred: impl FnMut(Pos) -> bool) -> Option<Pos> {
    for r in 0..b.rows {
        for c in 0..b.cols {
            let p = Pos::new(r, c);
            if pred(p) {
                return Some(p);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view_with(rows: i32, cols: i32, mines: i32) -> View {
        let mut v = View::new();
        v.start_game(Difficulty::custom(rows, cols, mines), 2026);
        v
    }

    /// 左右键同按必须进入 chord 模式 —— 这是整个玩法的核心。
    #[test]
    fn left_plus_right_enters_chord_mode() {
        let mut v = view_with(16, 16, 40);
        let m = v.metrics();
        let cell = m.cell_rect(3, 3);
        let pt = (cell.x + cell.w / 2, cell.y + cell.h / 2);

        v.button_down(Button::Left, pt);
        assert_eq!(v.mode, Mode::Left);
        v.button_down(Button::Right, pt);
        assert_eq!(v.mode, Mode::Chord, "左右键同按应进入 chord 模式");
        v.button_up(Button::Right, pt);
        v.button_up(Button::Left, pt);
    }

    /// 中键单独按下也必须进入 chord 模式。
    #[test]
    fn middle_button_enters_chord_mode() {
        let mut v = view_with(16, 16, 40);
        let m = v.metrics();
        let cell = m.cell_rect(3, 3);
        let pt = (cell.x + cell.w / 2, cell.y + cell.h / 2);
        v.button_down(Button::Middle, pt);
        assert_eq!(v.mode, Mode::Chord);
    }

    // ── 「单击快速展开」扩展 ────────────────────────────────────────────

    /// 造一个「规则 2 可成立」的棋盘：
    /// 顶行放 5 颗雷，`(1,1)` 的相邻计数是 3；把 `(0,0)(0,1)(0,2)` 插旗后
    /// `flagged == 3 == adjacent`，chord 规则 2 成立。
    fn view_chord_ready(left_click_chord: bool) -> View {
        let mut v = View::new();
        v.start_game(Difficulty::custom(10, 10, 5), 1);
        let mines: Vec<Pos> = (0..5).map(|c| Pos::new(0, c)).collect();
        assert!(v.board_mut().force_mines(&mines), "固定雷位应生效");
        v.left_click_chord = left_click_chord;

        // 翻开 (1,1) 并插好三面旗，让规则 2 成立。
        v.board_mut().left_click(Pos::new(1, 1));
        for c in 0..3 {
            v.board_mut().cycle_mark(Pos::new(0, c), false);
        }
        assert_eq!(v.board().adjacent(Pos::new(1, 1)), 3);
        // 探针格选 (1,0)：它是规则 2 的展开目标之一，且不是雷。
        // **不要**用 (0,1) 当探针 —— 那是已插旗的雷，本来就永远不该被翻开，
        // 拿它断言等于什么都没测。
        assert!(
            !v.board().is_open(Pos::new(1, 0)),
            "前置条件：(1,0) 还没翻开"
        );
        assert!(!v.board().is_mine(Pos::new(1, 0)), "前置条件：(1,0) 不是雷");
        v
    }

    /// 把格号换成格子中心的鼠标坐标。
    fn center(v: &View, row: i32, col: i32) -> (i32, i32) {
        let r = v.metrics().cell_rect(col, row);
        (r.x + r.w / 2, r.y + r.h / 2)
    }

    /// 扩展默认必须是关的 —— 否则原版行为在无声无息中变了。
    #[test]
    fn left_click_chord_is_off_by_default() {
        assert!(!View::new().left_click_chord);
    }

    /// 开关**关闭**时，左键点已翻开的数字什么都不做（原版行为）。
    #[test]
    fn left_click_chord_off_does_nothing() {
        let mut v = view_chord_ready(false);
        let pt = center(&v, 1, 1);
        v.button_down(Button::Left, pt);
        v.button_up(Button::Left, pt);
        assert!(
            !v.board().is_open(Pos::new(1, 0)),
            "开关关闭时左键点数字不该展开任何东西"
        );
    }

    /// 开关**打开**时，左键点已翻开的数字触发快速展开。
    #[test]
    fn left_click_chord_on_reveals() {
        let mut v = view_chord_ready(true);
        let pt = center(&v, 1, 1);
        v.button_down(Button::Left, pt);
        v.button_up(Button::Left, pt);
        assert!(
            v.board().is_open(Pos::new(1, 0)),
            "开关打开时左键点数字应触发快速展开"
        );
        for c in 0..3 {
            assert!(
                !v.board().is_open(Pos::new(0, c)),
                "已插旗的雷 ({c}) 不该被翻开"
            );
        }
    }

    /// 打开时按下必须进入 chord 预览态 —— 这是误触唯一的视觉逃生口。
    #[test]
    fn left_click_chord_shows_the_preview_while_pressed() {
        let mut v = view_chord_ready(true);
        let pt = center(&v, 1, 1);
        v.button_down(Button::Left, pt);
        assert_eq!(v.mode, Mode::Chord, "按下时应进入 chord 预览态");
        assert_eq!(v.chord_source, Pos::new(1, 1), "预览的源格应是按下的那一格");
        v.button_up(Button::Left, pt);
    }

    /// 打开开关**不得**改变未翻开格子的行为。
    #[test]
    fn left_click_chord_does_not_affect_closed_cells() {
        let mut v = view_chord_ready(true);
        let pt = center(&v, 5, 5);
        v.button_down(Button::Left, pt);
        assert_eq!(v.mode, Mode::Left, "未翻开的格子仍应是普通左键");
        v.button_up(Button::Left, pt);
        assert!(v.board().is_open(Pos::new(5, 5)), "未翻开的格子仍应被翻开");
    }

    /// 打开开关**不得**夺走中键与左右同按。
    #[test]
    fn left_click_chord_keeps_the_original_gestures() {
        for chord_opt in [false, true] {
            // 中键
            let mut v = view_chord_ready(chord_opt);
            let pt = center(&v, 1, 1);
            v.button_down(Button::Middle, pt);
            assert_eq!(v.mode, Mode::Chord, "中键应仍是 chord（开关={chord_opt}）");
            v.button_up(Button::Middle, pt);

            // 左右同按
            let mut v = view_chord_ready(chord_opt);
            let pt = center(&v, 1, 1);
            v.button_down(Button::Left, pt);
            v.button_down(Button::Right, pt);
            assert_eq!(
                v.mode,
                Mode::Chord,
                "左右同按应仍是 chord（开关={chord_opt}）"
            );
            v.button_up(Button::Right, pt);
            v.button_up(Button::Left, pt);
        }
    }

    /// 把指针拖出雷区再松手可以取消 —— 发现红框不对时的逃生口。
    #[test]
    fn left_click_chord_can_be_aborted_by_dragging_out() {
        let mut v = view_chord_ready(true);
        let pt = center(&v, 1, 1);
        v.button_down(Button::Left, pt);
        // 移到雷区左上角之外（客户区 0,0 附近）
        v.mouse_move((2, 2));
        v.button_up(Button::Left, (2, 2));
        assert!(
            !v.board().is_open(Pos::new(1, 0)),
            "拖出雷区后松手不该展开任何东西"
        );
    }

    /// 开关打开时点一个「没有可推断内容」的数字，应当什么都不发生
    /// （`decide` 返回 `Action::None`，与原版一致）。
    #[test]
    fn left_click_chord_on_a_plain_number_does_nothing() {
        let mut v = view_chord_ready(true);
        // (2,2) 是已翻开的空白格（相邻计数 0）
        v.board_mut().left_click(Pos::new(2, 2));
        let before = v.board().opened_count();
        let pt = center(&v, 2, 2);
        v.button_down(Button::Left, pt);
        v.button_up(Button::Left, pt);
        assert_eq!(v.board().opened_count(), before, "空白格上不该有动作");
    }

    /// 失败之后点笑脸必须请求开新局。
    ///
    /// 这条守的是「回调触发了但宿主没接线」那一类 bug —— 界面看起来正常，
    /// 但点下去什么都不发生。
    #[test]
    fn smiley_click_requests_new_game_after_loss() {
        use std::cell::Cell;
        use std::rc::Rc;

        // 用固定雷位，避免依赖随机布雷的结果。
        let mines = [Pos::new(0, 0), Pos::new(0, 7), Pos::new(7, 0)];

        for phase in [Phase::Dead, Phase::Won] {
            let mut v = View::new();
            v.start_game(Difficulty::custom(8, 8, 3), 1);
            assert!(v.board_mut().force_mines(&mines), "固定雷位应生效");

            if phase == Phase::Dead {
                v.board_mut().left_click(mines[0]);
            } else {
                let b = v.board_mut();
                for r in 0..8 {
                    for c in 0..8 {
                        let p = Pos::new(r, c);
                        if !b.is_mine(p) {
                            b.left_click(p);
                        }
                    }
                }
            }
            assert_eq!(v.board().phase, phase, "前置状态没造对");

            let fired = Rc::new(Cell::new(false));
            let f = fired.clone();
            v.events.new_game_requested = Some(Box::new(move || f.set(true)));

            let s = v.smiley_rect();
            let pt = (s.x + s.w / 2, s.y + s.h / 2);
            v.button_down(Button::Left, pt);
            v.button_up(Button::Left, pt);

            assert!(fired.get(), "{phase:?} 时点笑脸没有请求开新局");
        }
    }

    /// 笑脸按下后拖走再松手**不应**开新局（原版行为）。
    #[test]
    fn dragging_off_the_smiley_does_not_restart() {
        use std::cell::Cell;
        use std::rc::Rc;

        let mut v = view_with(8, 8, 3);
        let fired = Rc::new(Cell::new(false));
        let f = fired.clone();
        v.events.new_game_requested = Some(Box::new(move || f.set(true)));

        let s = v.smiley_rect();
        let m = v.metrics();
        let far = m.cell_rect(0, 0);

        v.button_down(Button::Left, (s.x + s.w / 2, s.y + s.h / 2));
        v.mouse_move((far.x + 2, far.y + 2));
        v.button_up(Button::Left, (far.x + 2, far.y + 2));

        assert!(!fired.get(), "拖离笑脸后松手不该开新局");
    }

    /// 拖动过程中换格必须同时失效新旧两格，否则会留下残影。
    #[test]
    fn moving_between_cells_invalidates_both() {
        let mut v = view_with(16, 16, 40);
        let m = v.metrics();
        let a = m.cell_rect(2, 2);
        let b = m.cell_rect(5, 5);
        v.button_down(Button::Left, (a.x + 2, a.y + 2));
        let _ = v.take_dirty();
        v.mouse_move((b.x + 2, b.y + 2));
        let (dirty, all) = v.take_dirty();
        assert!(!all);
        assert!(dirty.iter().any(|r| r.overlaps(&a)), "旧格未失效");
        assert!(dirty.iter().any(|r| r.overlaps(&b)), "新格未失效");
    }
}
