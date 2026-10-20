//! 纯逻辑层：棋盘状态机、布雷、展开、chord 判定。
//!
//! 移植自 Go 版 `internal/game/`。这一层**不依赖任何绘图、窗口或平台 API**，
//! 也不依赖 `theme` 的像素常量 —— 它是整个工程里唯一可以脱离图形栈单独测试的部分。
//!
//! 坐标系：`Pos { row, col }`，口径为「行 × 列」。

use std::collections::HashSet;

// ── Pos ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Pos {
    pub row: i32,
    pub col: i32,
}

impl Pos {
    pub const NONE: Pos = Pos { row: -1, col: -1 };

    pub const fn new(row: i32, col: i32) -> Self {
        Pos { row, col }
    }
    pub const fn is_valid(self) -> bool {
        self.row >= 0 && self.col >= 0
    }
}

/// 邻格顺序与原版取证时一致：先上排、再同行左右、最后下排。
/// 顺序会影响 `Expand` 的返回次序与脏矩形集合，**不要重排**。
const AROUND8: [(i32, i32); 8] = [
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, -1),
    (0, 1),
    (1, -1),
    (1, 0),
    (1, 1),
];

/// 返回 `at` 周围 8 格中落在棋盘内的坐标。
pub fn neighbors(rows: i32, cols: i32, at: Pos) -> impl Iterator<Item = Pos> {
    AROUND8.iter().filter_map(move |(dr, dc)| {
        let (r, c) = (at.row + dr, at.col + dc);
        (r >= 0 && r < rows && c >= 0 && c < cols).then_some(Pos::new(r, c))
    })
}

// ── 标记与阶段 ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mark {
    #[default]
    None,
    Flag,
    Question,
}

/// `Ready` 表示棋盘已摆好但还没第一次点击，此时**雷尚未落下**。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Phase {
    #[default]
    Ready,
    Playing,
    Dead,
    Won,
}

/// 笑脸按钮的五种状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mood {
    Normal,
    Pressed,
    Concentrate,
    Dead,
    Cool,
}

// ── 难度 ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Beginner,
    Intermediate,
    Expert,
    Custom,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Difficulty {
    pub rows: i32,
    pub cols: i32,
    pub mines: i32,
    pub kind: Kind,
}

impl Difficulty {
    pub const BEGINNER: Difficulty = Difficulty {
        rows: 16,
        cols: 16,
        mines: 40,
        kind: Kind::Beginner,
    };
    pub const INTERMEDIATE: Difficulty = Difficulty {
        rows: 16,
        cols: 30,
        mines: 99,
        kind: Kind::Intermediate,
    };
    pub const EXPERT: Difficulty = Difficulty {
        rows: 24,
        cols: 30,
        mines: 199,
        kind: Kind::Expert,
    };
    pub const PRESETS: [Difficulty; 3] = [
        Difficulty::BEGINNER,
        Difficulty::INTERMEDIATE,
        Difficulty::EXPERT,
    ];

    /// 自定义雷区的边界。最小宽度取 8：状态栏是固定像素布局
    /// （数码管 41 + 笑脸 26 + 数码管 41，16px 基准逻辑像素），客户区窄于 8 列时
    /// 两只数码管会与笑脸重叠；行数不受这个限制。
    pub const MIN_ROWS: i32 = 5;
    pub const MAX_ROWS: i32 = 99;
    pub const MIN_COLS: i32 = 8;
    pub const MAX_COLS: i32 = 99;
    pub const MIN_MINES: i32 = 10;

    pub fn custom(rows: i32, cols: i32, mines: i32) -> Difficulty {
        Difficulty {
            rows,
            cols,
            mines,
            kind: Kind::Custom,
        }
    }

    /// 上限是 `rows*cols - MIN_MINES`，即至少留 10 个空格。
    pub fn max_mines_for(rows: i32, cols: i32) -> i32 {
        rows * cols - Self::MIN_MINES
    }

    pub fn is_valid_custom(rows: i32, cols: i32, mines: i32) -> bool {
        (Self::MIN_ROWS..=Self::MAX_ROWS).contains(&rows)
            && (Self::MIN_COLS..=Self::MAX_COLS).contains(&cols)
            && mines >= Self::MIN_MINES
            && mines <= Self::max_mines_for(rows, cols)
    }

    pub fn label(&self) -> &'static str {
        match self.kind {
            Kind::Beginner => "初级",
            Kind::Intermediate => "中级",
            Kind::Expert => "高级",
            Kind::Custom => "自定义",
        }
    }
}

// ── 可复现随机源 ────────────────────────────────────────────────────────────

/// PCG32。固定种子下逐位可复现，用于布雷与自检的随机对局。
///
/// **注意**：与 Go 版 `math/rand/v2` 的 PCG64 不是同一序列，所以同一个种子在两边
/// 会得到不同雷位。这不影响正确性 —— 固定雷位的场景脚本（`mines r c`）根本不走 RNG，
/// 随机对局的自检只要求统计性质。切换实现时只需重新生成 `--genlayout` 产物。
pub struct Rand {
    state: u64,
    inc: u64,
}

impl Rand {
    pub fn new(seed: u64) -> Self {
        let mut r = Rand {
            state: 0,
            inc: (seed << 1) | 1,
        };
        r.next_u32();
        r.state = r.state.wrapping_add(seed);
        r.next_u32();
        r
    }

    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(6364136223846793005).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// 返回 `[0, n)`，用 Lemire 的乘移位法消除取模偏差。
    pub fn below(&mut self, n: u32) -> u32 {
        debug_assert!(n > 0);
        let mut m = (self.next_u32() as u64) * (n as u64);
        let mut low = m as u32;
        if low < n {
            let threshold = n.wrapping_neg() % n;
            while low < threshold {
                m = (self.next_u32() as u64) * (n as u64);
                low = m as u32;
            }
        }
        (m >> 32) as u32
    }

    pub fn intn(&mut self, n: i32) -> i32 {
        self.below(n as u32) as i32
    }

    pub fn int_range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + self.intn(hi - lo)
    }
}

// ── 布雷 ────────────────────────────────────────────────────────────────────

/// 布雷。原版是「开局先布雷，首点若踩雷则把那颗雷挪走」；这里是「首点之后才布雷，
/// 且排除首点格」。两者对玩家不可分辨，但本实现天然保证首点安全，不依赖后续修补。
pub fn place(rows: i32, cols: i32, mines: i32, safe: Pos, rng: &mut Rand) -> Vec<bool> {
    let total = (rows * cols) as usize;
    let mines = mines.min(rows * cols - 1).max(0) as usize;
    let mut field = vec![false; total];

    // 部分 Fisher–Yates：只洗出前 mines 个，避免为 99×99 分配两次全量数组。
    let safe_idx = (safe.row * cols + safe.col) as usize;
    let mut pool: Vec<u32> = (0..total as u32)
        .filter(|&i| i as usize != safe_idx)
        .collect();

    for i in 0..mines.min(pool.len()) {
        let j = i + rng.below((pool.len() - i) as u32) as usize;
        pool.swap(i, j);
        field[pool[i] as usize] = true;
    }
    field
}

// ── 一次操作的结果 ──────────────────────────────────────────────────────────

/// 一次操作造成的变化集合，供界面做脏矩形失效与音效触发。
#[derive(Default, Debug)]
pub struct MoveResult {
    pub changed: Vec<Pos>,
    /// 本局是否刚刚真正开始（首点布雷完成）。
    pub started: bool,
    pub boom: bool,
    pub won: bool,
}

impl MoveResult {
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && !self.started && !self.boom && !self.won
    }
}

// ── 棋盘 ────────────────────────────────────────────────────────────────────

pub struct Board {
    pub rows: i32,
    pub cols: i32,
    pub mines: i32,
    pub phase: Phase,
    pub seconds: i32,

    mine: Vec<bool>,
    open: Vec<bool>,
    reveal: Vec<bool>,
    wrong: Vec<bool>,
    adj: Vec<u8>,
    mark: Vec<Mark>,
    rng: Rand,
    placed: bool,
    exploded: Pos,
    opened: i32,
    flags: i32,
}

impl Board {
    /// 建棋盘；雷数被夹在 `[1, rows*cols-1]`。
    pub fn new(rows: i32, cols: i32, mines: i32, rng: Rand) -> Self {
        let mines = mines.clamp(1, rows * cols - 1);
        let n = (rows * cols) as usize;
        Board {
            rows,
            cols,
            mines,
            phase: Phase::Ready,
            seconds: 0,
            mine: vec![false; n],
            open: vec![false; n],
            reveal: vec![false; n],
            wrong: vec![false; n],
            adj: vec![0; n],
            mark: vec![Mark::None; n],
            rng,
            placed: false,
            exploded: Pos::NONE,
            opened: 0,
            flags: 0,
        }
    }

    #[inline]
    fn idx(&self, p: Pos) -> usize {
        (p.row * self.cols + p.col) as usize
    }

    pub fn in_bounds(&self, p: Pos) -> bool {
        p.row >= 0 && p.row < self.rows && p.col >= 0 && p.col < self.cols
    }

    pub fn is_mine(&self, p: Pos) -> bool {
        self.mine[self.idx(p)]
    }
    pub fn is_open(&self, p: Pos) -> bool {
        self.open[self.idx(p)]
    }
    pub fn adjacent(&self, p: Pos) -> i32 {
        self.adj[self.idx(p)] as i32
    }
    pub fn mark_of(&self, p: Pos) -> Mark {
        self.mark[self.idx(p)]
    }
    pub fn is_exploded(&self, p: Pos) -> bool {
        self.exploded == p
    }
    pub fn shows_wrong_flag(&self, p: Pos) -> bool {
        self.wrong[self.idx(p)]
    }
    pub fn field_placed(&self) -> bool {
        self.placed
    }
    pub fn flags_left(&self) -> i32 {
        self.mines - self.flags
    }
    pub fn flags_used(&self) -> i32 {
        self.flags
    }
    pub fn opened_count(&self) -> i32 {
        self.opened
    }

    /// 画成凹陷的格子：已翻开、结束时亮雷，或结束时暴露的错旗。
    pub fn is_sunken(&self, p: Pos) -> bool {
        let i = self.idx(p);
        self.open[i] || self.reveal[i] || self.wrong[i]
    }

    /// 这一格要把雷画出来：结束时亮雷、被翻开的雷，或插错的旗（雷上盖红叉）。
    pub fn shows_mine(&self, p: Pos) -> bool {
        let i = self.idx(p);
        self.reveal[i] || self.wrong[i] || (self.open[i] && self.mine[i])
    }

    /// 格子上显示的数字；雷、未翻开与错旗格都是 0。
    pub fn number_at(&self, p: Pos) -> i32 {
        let i = self.idx(p);
        if !self.is_sunken(p) || self.mine[i] || self.wrong[i] {
            return 0;
        }
        self.adj[i] as i32
    }

    pub fn adjacent_flag_count(&self, p: Pos) -> i32 {
        neighbors(self.rows, self.cols, p)
            .filter(|&q| self.mark[self.idx(q)] == Mark::Flag)
            .count() as i32
    }

    /// 翻开一格（不计入展开），供 [`expand`] 调用。
    fn try_open(&mut self, p: Pos) -> bool {
        let i = self.idx(p);
        if self.open[i] {
            return false;
        }
        self.open[i] = true;
        self.mark[i] = Mark::None;
        self.opened += 1;
        true
    }

    /// 清空棋盘回到 `Ready`（雷尚未落下）。
    pub fn reset(&mut self) {
        self.mine.fill(false);
        self.open.fill(false);
        self.reveal.fill(false);
        self.wrong.fill(false);
        self.adj.fill(0);
        self.mark.fill(Mark::None);
        self.placed = false;
        self.exploded = Pos::NONE;
        self.opened = 0;
        self.flags = 0;
        self.seconds = 0;
        self.phase = Phase::Ready;
    }

    /// 首次点击时才布雷，并保证该格不是雷。
    fn ensure_placed(&mut self, safe: Pos, res: &mut MoveResult) {
        if self.placed {
            return;
        }
        let field = place(self.rows, self.cols, self.mines, safe, &mut self.rng);
        self.mine.copy_from_slice(&field);
        self.recompute_adjacency();
        self.placed = true;
        res.started = true;
    }

    fn recompute_adjacency(&mut self) {
        for r in 0..self.rows {
            for c in 0..self.cols {
                let p = Pos::new(r, c);
                let i = self.idx(p);
                if self.mine[i] {
                    continue;
                }
                self.adj[i] = neighbors(self.rows, self.cols, p)
                    .filter(|&q| self.mine[self.idx(q)])
                    .count() as u8;
            }
        }
    }

    /// 供场景脚本与测试使用：把雷位摆成给定坐标。玩家路径不会调用它。
    ///
    /// 只有数量与 `mines` 相符、且坐标全在界内才生效。
    pub fn force_mines(&mut self, cells: &[Pos]) -> bool {
        if cells.len() as i32 != self.mines || cells.iter().any(|&p| !self.in_bounds(p)) {
            return false;
        }
        self.mine.fill(false);
        for &p in cells {
            let i = self.idx(p);
            self.mine[i] = true;
        }
        self.placed = true;
        if self.phase == Phase::Ready {
            self.phase = Phase::Playing;
        }
        self.recompute_adjacency();
        true
    }

    // ── 玩家操作 ────────────────────────────────────────────────────────────

    /// 左键翻开一格（含空白区展开）；点中雷则结束。
    pub fn left_click(&mut self, p: Pos) -> MoveResult {
        let mut res = MoveResult::default();
        if !self.in_bounds(p) || matches!(self.phase, Phase::Dead | Phase::Won) {
            return res;
        }
        let i = self.idx(p);
        if self.mark[i] == Mark::Flag || self.open[i] {
            return res;
        }

        self.ensure_placed(p, &mut res);
        if self.phase == Phase::Ready {
            self.phase = Phase::Playing;
        }

        if self.mine[i] {
            self.die(p, &mut res);
            return res;
        }

        res.changed.extend(self.expand(p));
        self.check_win(&mut res);
        res
    }

    /// 右键标记循环：旗 → 问号（可选）→ 空。
    pub fn cycle_mark(&mut self, p: Pos, allow_question: bool) -> MoveResult {
        let mut res = MoveResult::default();
        if !self.in_bounds(p) || matches!(self.phase, Phase::Dead | Phase::Won) {
            return res;
        }
        let i = self.idx(p);
        if self.open[i] {
            return res;
        }

        let next = match self.mark[i] {
            Mark::None => Mark::Flag,
            Mark::Flag if allow_question => Mark::Question,
            _ => Mark::None,
        };

        if self.mark[i] == Mark::Flag && next != Mark::Flag {
            self.flags -= 1;
        }
        if next == Mark::Flag {
            self.flags += 1;
        }
        self.mark[i] = next;
        res.changed.push(p);
        res
    }

    /// 一次 chord 手势：按 [`decide`] 决定是「标雷」还是「快速展开」。
    pub fn apply_chord(&mut self, p: Pos) -> MoveResult {
        let mut res = MoveResult::default();
        if !self.in_bounds(p) || matches!(self.phase, Phase::Dead | Phase::Won) {
            return res;
        }
        if !self.placed || !self.open[self.idx(p)] {
            return res;
        }

        match decide(self, p) {
            Action::MarkMines => {
                for q in mark_targets(self, p) {
                    let i = self.idx(q);
                    if self.mark[i] == Mark::Flag {
                        continue;
                    }
                    self.flags += 1;
                    self.mark[i] = Mark::Flag;
                    res.changed.push(q);
                }
            }
            Action::Reveal => {
                let targets = reveal_targets(self, p);
                res.changed.push(p);
                // 先判定是否踩雷：规则 2 保留原版风险，旗子插错时会爆。
                let boom = targets.iter().copied().find(|&t| self.is_mine(t));
                if let Some(boom) = boom {
                    for t in targets {
                        if self.try_open(t) {
                            res.changed.push(t);
                        }
                    }
                    self.die(boom, &mut res);
                    return res;
                }
                for t in targets {
                    for q in self.expand(t) {
                        if !res.changed.contains(&q) {
                            res.changed.push(q);
                        }
                    }
                }
                self.check_win(&mut res);
            }
            Action::None => {}
        }
        res
    }

    /// 按住手势里能被单格约束确定为雷的格子（也是红框那批）。
    pub fn certain_mines(&self, at: Pos) -> Vec<Pos> {
        mark_targets(self, at)
    }

    /// 空白区展开。用显式栈迭代，99×99 全空白也不会栈溢出。
    fn expand(&mut self, start: Pos) -> Vec<Pos> {
        let mut opened = Vec::new();
        if !self.in_bounds(start)
            || self.is_open(start)
            || self.mark_of(start) == Mark::Flag
            || self.is_mine(start)
        {
            return opened;
        }

        let mut stack = vec![start];
        while let Some(p) = stack.pop() {
            if self.is_open(p) || self.is_mine(p) || !self.try_open(p) {
                continue;
            }
            opened.push(p);

            if self.adjacent(p) != 0 {
                continue;
            }
            for n in neighbors(self.rows, self.cols, p) {
                if !self.is_open(n) && !self.is_mine(n) && self.mark_of(n) != Mark::Flag {
                    stack.push(n);
                }
            }
        }
        opened
    }

    fn die(&mut self, at: Pos, res: &mut MoveResult) {
        self.phase = Phase::Dead;
        self.exploded = at;
        let ai = self.idx(at);
        self.open[ai] = true;
        self.reveal[ai] = true;
        res.boom = true;

        for r in 0..self.rows {
            for c in 0..self.cols {
                let q = Pos::new(r, c);
                let i = self.idx(q);
                if self.mine[i] {
                    // 已插旗的雷不亮出来。
                    if self.mark[i] != Mark::Flag {
                        self.reveal[i] = true;
                    }
                } else {
                    // 插错的旗标成「错旗」（雷上盖红叉），问号一律清掉。
                    if self.mark[i] == Mark::Flag {
                        self.wrong[i] = true;
                    }
                    if self.mark[i] == Mark::Question {
                        self.mark[i] = Mark::None;
                    }
                }
                if self.is_sunken(q) {
                    res.changed.push(q);
                }
            }
        }
    }

    fn check_win(&mut self, res: &mut MoveResult) {
        if self.opened != self.rows * self.cols - self.mines {
            return;
        }
        self.phase = Phase::Won;
        res.won = true;
        // 获胜时未标记的雷自动补旗。
        for r in 0..self.rows {
            for c in 0..self.cols {
                let p = Pos::new(r, c);
                let i = self.idx(p);
                if self.mine[i] && self.mark[i] != Mark::Flag {
                    self.mark[i] = Mark::Flag;
                    self.flags += 1;
                    res.changed.push(p);
                }
            }
        }
    }
}

// ── chord 判定 ──────────────────────────────────────────────────────────────

/// 按住中键 / 左右键同按时，一次手势要做的事情。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    None,
    /// 未翻开的邻格数正好等于数字 ⇒ 它们必然全是雷，全部标雷。
    MarkMines,
    /// 已标雷的邻格数正好等于数字 ⇒ 翻开其余邻格（原版快速展开）。
    Reveal,
}

/// 快速展开的判定。原版只在「中键按下」或「左右键同时按下」时触发，**双击不触发**。
///
/// 两条规则互斥，且都成立不了时什么都不做：
/// 1. 未翻开邻格数 == 数字 ⇒ 这些格子必然全是雷（雷只可能藏在未翻开的格子里），全部标雷；
/// 2. 已标雷邻格数 == 数字 ⇒ 翻开其余未翻开邻格。旗子插错时会踩雷，与原版一致。
pub fn decide(b: &Board, at: Pos) -> Action {
    if !b.in_bounds(at) || !b.is_open(at) {
        return Action::None;
    }
    let n = b.adjacent(at);
    if n == 0 {
        return Action::None;
    }

    let (mut unopened, mut unopened_unflagged, mut flagged) = (0, 0, 0);
    for q in neighbors(b.rows, b.cols, at) {
        if b.is_open(q) {
            continue;
        }
        unopened += 1;
        if b.mark_of(q) == Mark::Flag {
            flagged += 1;
        } else {
            unopened_unflagged += 1;
        }
    }

    if unopened == n && unopened_unflagged > 0 {
        return Action::MarkMines;
    }
    if flagged == n && unopened_unflagged > 0 {
        return Action::Reveal;
    }
    Action::None
}

/// 规则 1 会标雷的那些格子（也是按住时画红框的那批）。
pub fn mark_targets(b: &Board, at: Pos) -> Vec<Pos> {
    if decide(b, at) != Action::MarkMines {
        return Vec::new();
    }
    neighbors(b.rows, b.cols, at)
        .filter(|&q| !b.is_open(q) && b.mark_of(q) != Mark::Flag)
        .collect()
}

/// 规则 2 会翻开的那些格子（原版快速展开的语义）。
pub fn reveal_targets(b: &Board, at: Pos) -> Vec<Pos> {
    if decide(b, at) != Action::Reveal {
        return Vec::new();
    }
    neighbors(b.rows, b.cols, at)
        .filter(|&q| !b.is_open(q) && b.mark_of(q) != Mark::Flag)
        .collect()
}

/// 一组坐标去重，保持首次出现的顺序。
pub fn distinct(cells: &[Pos]) -> Vec<Pos> {
    let mut seen = HashSet::with_capacity(cells.len());
    cells.iter().copied().filter(|p| seen.insert(*p)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(rows: i32, cols: i32, mines: i32, seed: u64) -> Board {
        Board::new(rows, cols, mines, Rand::new(seed))
    }

    /// 首点必须永远安全 —— 这是布雷算法的核心不变量。
    #[test]
    fn first_click_is_never_a_mine() {
        for seed in 0..500u64 {
            let mut b = board(16, 16, 40, seed);
            let p = Pos::new((seed % 16) as i32, ((seed / 16) % 16) as i32);
            let r = b.left_click(p);
            assert!(!r.boom, "seed={seed} 首点踩雷");
            assert!(b.is_open(p));
            assert_eq!(b.phase, Phase::Playing);
        }
    }

    /// 雷数必须精确，且首点格本身不在雷位上。
    #[test]
    fn mine_count_is_exact() {
        for seed in 0..200u64 {
            let mut b = board(24, 30, 199, seed);
            let p = Pos::new(12, 15);
            b.left_click(p);
            let n: i32 = (0..24)
                .flat_map(|r| (0..30).map(move |c| Pos::new(r, c)))
                .filter(|&q| b.is_mine(q))
                .count() as i32;
            assert_eq!(n, 199, "seed={seed}");
            assert!(!b.is_mine(p), "seed={seed} 首点格落在雷位上");
        }
    }

    /// 空白区展开永远不翻开雷。
    #[test]
    fn expand_never_opens_a_mine() {
        for seed in 0..300u64 {
            let mut b = board(24, 30, 199, seed);
            let p = Pos::new(12, 15);
            b.left_click(p);
            for r in 0..24 {
                for c in 0..30 {
                    let q = Pos::new(r, c);
                    if b.is_open(q) {
                        assert!(!b.is_mine(q), "seed={seed} 翻开了雷 {q:?}");
                    }
                }
            }
        }
    }

    /// 99×99 全盘展开不得栈溢出。
    #[test]
    fn big_board_expand_does_not_overflow() {
        let mut b = board(99, 99, 10, 7);
        let r = b.left_click(Pos::new(49, 49));
        assert!(r.changed.len() > 9000, "展开格数偏少：{}", r.changed.len());
        assert!(!r.boom);
    }

    /// chord 规则 1：未翻开邻格数 == 数字 ⇒ 全部标雷，且只标真雷。
    #[test]
    fn chord_rule_one_marks_only_real_mines() {
        let mut b = board(24, 30, 199, 2026);
        b.left_click(Pos::new(12, 15));
        for r in 0..24 {
            for c in 0..30 {
                let p = Pos::new(r, c);
                if !b.is_open(p) || b.adjacent(p) == 0 {
                    continue;
                }
                if decide(&b, p) == Action::MarkMines {
                    for q in mark_targets(&b, p) {
                        assert!(b.is_mine(q), "规则 1 标了非雷格 {q:?}");
                    }
                }
            }
        }
    }

    /// chord 规则 2：旗数正确时一次展开，且不踩雷。
    #[test]
    fn chord_rule_two_reveals_safely_when_flags_are_right() {
        let mut b = board(16, 16, 40, 99);
        b.left_click(Pos::new(8, 8));
        for r in 0..16 {
            for c in 0..16 {
                let p = Pos::new(r, c);
                if !b.is_open(p) || b.adjacent(p) == 0 {
                    continue;
                }
                if decide(&b, p) != Action::Reveal {
                    continue;
                }
                let before = b.opened_count();
                let res = b.apply_chord(p);
                assert!(!res.boom, "旗数正确时 chord 不该踩雷");
                assert!(b.opened_count() >= before);
            }
        }
    }

    /// 全开非雷格即获胜。
    #[test]
    fn opening_every_safe_cell_wins() {
        let mut b = board(16, 16, 40, 5);
        b.left_click(Pos::new(0, 0));
        for r in 0..16 {
            for c in 0..16 {
                let p = Pos::new(r, c);
                if !b.is_mine(p) && !b.is_open(p) {
                    b.left_click(p);
                }
            }
        }
        assert_eq!(b.phase, Phase::Won);
    }
}
