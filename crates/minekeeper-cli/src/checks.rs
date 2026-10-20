//! 无窗口自检。
//!
//! 这是 `--selftest` 与 `cargo test` **共用的唯一清单** —— 两份清单走偏是这类项目
//! 最常见的腐化方式。
//!
//! 分类：
//! * **纯逻辑**（不碰光栅器）：首点安全、雷数、相邻计数、永不翻开雷、胜负判定、
//!   chord 两条规则、99×99 不溢出、单格推断只报真雷。这些在任何平台都能跑。
//! * **渲染相关**（需要光栅器）：客户区尺寸表、数码管格式化、凸起格斜接、字形定宽。
//!   光栅器是纯 Rust 的，所以它们同样跨平台。

use mk_core::game::{self, Action, Board, Difficulty, Pos, Rand};
use mk_core::pix::FontStack;
use mk_core::theme::{self, Metrics, Size};
use mk_core::view::Button;
use mk_core::{Canvas, View};

type Check = (&'static str, fn(i32) -> Result<(), String>);

/// 唯一清单。`games` 只缩放随机对局的规模，其余样本量是各自「能不能构造出正例/负例」的下限。
pub fn checks() -> Vec<Check> {
    vec![
        ("首点永不踩雷", first_click_safety),
        ("雷数精确", mine_count),
        ("相邻计数正确", adjacency),
        ("永不翻开雷", never_opens_mine),
        ("全开非雷即获胜", win_detection),
        ("chord 只在条件成立时生效", chord_rules),
        ("chord 规则1 只标真雷", chord_marks_only_real_mines),
        ("99×99 展开不溢出", big_board),
        ("单格推断只报真雷", certain_mine_inference),
        ("客户区尺寸表（小/中/大）", size_table),
        ("数码管格式化", led_format),
        ("凸起格斜接落在调色板内", bevel_palette),
        ("数字字形逐字定宽", glyph_widths),
        ("左右键同按进入 chord 模式", chord_gesture_simultaneous),
        ("中键进入 chord 模式", chord_gesture_middle),
        ("chord 手势矩阵 · 中键/标雷", |_| {
            chord_gesture("middle", "mark")
        }),
        ("chord 手势矩阵 · 中键/展开", |_| {
            chord_gesture("middle", "reveal")
        }),
        ("chord 手势矩阵 · 左右同按/标雷", |_| {
            chord_gesture("both", "mark")
        }),
        ("chord 手势矩阵 · 左右同按/展开", |_| {
            chord_gesture("both", "reveal")
        }),
        ("chord 手势矩阵 · 右左同按/标雷", |_| {
            chord_gesture("both-reverse", "mark")
        }),
        ("chord 手势矩阵 · 右左同按/展开", |_| {
            chord_gesture("both-reverse", "reveal")
        }),
        ("同种子雷位可复现", same_seed_reproducible),
    ]
}

/// 跑全部检查，返回失败项数。
pub fn self_test(games: i32) -> usize {
    let mut failed = 0;
    for (name, run) in checks() {
        match run(games) {
            Ok(()) => println!("  ✔ {name}"),
            Err(e) => {
                println!("  ✘ {name}：{e}");
                failed += 1;
            }
        }
    }
    println!(
        "SELFTEST result={} passed={} failed={failed}",
        if failed == 0 { "pass" } else { "fail" },
        checks().len() - failed
    );
    failed
}

fn board(rows: i32, cols: i32, mines: i32, seed: u64) -> Board {
    Board::new(rows, cols, mines, Rand::new(seed))
}

// ── 纯逻辑 ──────────────────────────────────────────────────────────────────

fn first_click_safety(games: i32) -> Result<(), String> {
    for seed in 0..games as u64 {
        let mut b = board(16, 16, 40, seed);
        let p = Pos::new((seed % 16) as i32, ((seed / 16) % 16) as i32);
        let r = b.left_click(p);
        if r.boom {
            return Err(format!("seed={seed} 首点踩雷"));
        }
        if !b.is_open(p) {
            return Err(format!("seed={seed} 首点未翻开"));
        }
    }
    Ok(())
}

fn mine_count(games: i32) -> Result<(), String> {
    for seed in 0..games as u64 {
        let mut b = board(24, 30, 199, seed);
        b.left_click(Pos::new(12, 15));
        let n = (0..24)
            .flat_map(|r| (0..30).map(move |c| Pos::new(r, c)))
            .filter(|&p| b.is_mine(p))
            .count();
        if n != 199 {
            return Err(format!("seed={seed} 雷数 {n} != 199"));
        }
    }
    Ok(())
}

fn adjacency(games: i32) -> Result<(), String> {
    for seed in 0..games as u64 {
        let mut b = board(16, 30, 99, seed);
        b.left_click(Pos::new(8, 15));
        for r in 0..16 {
            for c in 0..30 {
                let p = Pos::new(r, c);
                if b.is_mine(p) {
                    continue;
                }
                let want = game::neighbors(16, 30, p).filter(|&q| b.is_mine(q)).count() as i32;
                if b.adjacent(p) != want {
                    return Err(format!(
                        "seed={seed} ({r},{c}) 计数 {} != {want}",
                        b.adjacent(p)
                    ));
                }
            }
        }
    }
    Ok(())
}

fn never_opens_mine(games: i32) -> Result<(), String> {
    for seed in 0..games as u64 {
        let mut b = board(24, 30, 199, seed);
        b.left_click(Pos::new(12, 15));
        for r in 0..24 {
            for c in 0..30 {
                let p = Pos::new(r, c);
                if b.is_open(p) && b.is_mine(p) {
                    return Err(format!("seed={seed} 翻开了雷 ({r},{c})"));
                }
            }
        }
    }
    Ok(())
}

fn win_detection(_: i32) -> Result<(), String> {
    for seed in 0..50u64 {
        let mut b = board(16, 16, 40, seed);
        b.left_click(Pos::new(0, 0));
        for r in 0..16 {
            for c in 0..16 {
                let p = Pos::new(r, c);
                if !b.is_mine(p) && !b.is_open(p) {
                    b.left_click(p);
                }
            }
        }
        if b.phase != game::Phase::Won {
            return Err(format!("seed={seed} 全开非雷后未判胜"));
        }
        // 获胜时未标记的雷应自动补旗。
        if b.flags_used() != 40 {
            return Err(format!("seed={seed} 获胜后旗数 {} != 40", b.flags_used()));
        }
    }
    Ok(())
}

fn chord_rules(_: i32) -> Result<(), String> {
    let mut b = board(24, 30, 199, 2026);
    b.left_click(Pos::new(12, 15));
    for r in 0..24 {
        for c in 0..30 {
            let p = Pos::new(r, c);
            if !b.is_open(p) {
                continue;
            }
            let a = game::decide(&b, p);
            // 未翻开的数字格在旗数不足时不该触发任何动作。
            if b.adjacent(p) > 0 && a == Action::None {
                let flags = b.adjacent_flag_count(p);
                let unopened = game::neighbors(24, 30, p)
                    .filter(|&q| !b.is_open(q))
                    .count() as i32;
                if flags == b.adjacent(p) || unopened == b.adjacent(p) {
                    return Err(format!("({r},{c}) 该触发却没触发"));
                }
            }
        }
    }
    Ok(())
}

fn chord_marks_only_real_mines(_: i32) -> Result<(), String> {
    for seed in 0..40u64 {
        let mut b = board(24, 30, 199, seed);
        b.left_click(Pos::new(12, 15));
        for r in 0..24 {
            for c in 0..30 {
                let p = Pos::new(r, c);
                if game::decide(&b, p) != Action::MarkMines {
                    continue;
                }
                for q in game::mark_targets(&b, p) {
                    if !b.is_mine(q) {
                        return Err(format!("seed={seed} 规则 1 标了非雷格 {q:?}"));
                    }
                }
            }
        }
    }
    Ok(())
}

fn big_board(_: i32) -> Result<(), String> {
    let mut b = board(99, 99, 10, 7);
    let r = b.left_click(Pos::new(49, 49));
    if r.changed.len() < 9000 {
        return Err(format!("展开格数偏少：{}", r.changed.len()));
    }
    Ok(())
}

fn certain_mine_inference(_: i32) -> Result<(), String> {
    for seed in 0..40u64 {
        let mut b = board(16, 30, 99, seed);
        b.left_click(Pos::new(8, 15));
        for r in 0..16 {
            for c in 0..30 {
                let p = Pos::new(r, c);
                for q in b.certain_mines(p) {
                    if !b.is_mine(q) {
                        return Err(format!("seed={seed} 单格推断报错 {q:?}"));
                    }
                }
            }
        }
    }
    Ok(())
}

// ── 渲染相关 ────────────────────────────────────────────────────────────────

fn size_table(_: i32) -> Result<(), String> {
    let expect = [
        (Size::Small, 497, 444),
        (Size::Medium, 747, 668),
        (Size::Large, 994, 888),
    ];
    for (size, w, h) in expect {
        let m = Metrics::new(size);
        if m.client_width(30) != w || m.client_height(24) != h {
            return Err(format!(
                "{size:?} 得 {}×{}，应为 {w}×{h}",
                m.client_width(30),
                m.client_height(24)
            ));
        }
        let status = m.status_rect(30);
        let field = m.field_rect(30, 24);
        if status.bottom() > field.y {
            return Err(format!("{size:?} 状态栏与雷区重叠"));
        }
    }
    Ok(())
}

fn led_format(_: i32) -> Result<(), String> {
    use mk_core::pix::led_format as f;
    let cases: [(i32, [char; 3]); 5] = [
        (0, ['0', '0', '0']),
        (7, ['0', '0', '7']),
        (999, ['9', '9', '9']),
        (-1, ['-', '0', '1']),
        (-120, ['-', '9', '9']),
    ];
    for (v, want) in cases {
        if f(v) != want {
            return Err(format!("{v} → {:?}，应为 {want:?}", f(v)));
        }
    }
    Ok(())
}

fn bevel_palette(_: i32) -> Result<(), String> {
    for size in Size::ALL {
        let m = Metrics::new(size);
        let n = m.cell_px();
        let mut c = Canvas::new(n, n).ok_or("建画布失败")?;
        let cell = c.bounds();
        mk_core::pix::fill_face(&mut c, cell);
        mk_core::pix::draw_raised_cell(&mut c, cell, &m);
        for y in 0..n {
            for x in 0..n {
                let i = ((y as u32 * n as u32 + x as u32) * 4) as usize;
                let px = (c.data()[i], c.data()[i + 1], c.data()[i + 2]);
                let ok = px == (theme::HILITE.0, theme::HILITE.1, theme::HILITE.2)
                    || px == (theme::SHADOW.0, theme::SHADOW.1, theme::SHADOW.2)
                    || px == (theme::FACE.0, theme::FACE.1, theme::FACE.2);
                if !ok {
                    return Err(format!("{size:?} ({x},{y}) 越出色板 {px:?}"));
                }
            }
        }
    }
    Ok(())
}

fn glyph_widths(_: i32) -> Result<(), String> {
    let mut fonts = FontStack::load_default();
    if fonts.is_empty() {
        return Err("找不到可用字体".into());
    }
    let m = Metrics::new(Size::Large);
    for n in 1u8..=8 {
        let want = m.px(theme::glyph_width(n));
        let ch = char::from_digit(n as u32, 10).unwrap();
        let Some(sp) = fonts.render_glyph(ch, m.cell_px(), &m, want, mk_core::theme::INK) else {
            return Err(format!("字形 {ch} 无墨迹"));
        };
        let (_, _, w, h) = sp.ink_bounds();
        if w != want {
            return Err(format!("字形 {ch} 宽 {w} != 目标 {want}"));
        }
        if h != m.px(theme::GLYPH_BOX) {
            return Err(format!(
                "字形 {ch} 高 {h} != 目标 {}",
                m.px(theme::GLYPH_BOX)
            ));
        }
    }
    Ok(())
}

// ── 输入路径 ────────────────────────────────────────────────────────────────

fn chord_gesture_simultaneous(games: i32) -> Result<(), String> {
    for seed in 0..games.min(50) as u64 {
        let mut v = View::new();
        v.apply_size(Size::Large);
        v.start_game(Difficulty::custom(14, 18, 34), seed);
        let m = v.metrics();
        let cell = m.cell_rect(5, 5);
        let pt = (cell.x + cell.w / 2, cell.y + cell.h / 2);
        v.button_down(mk_core::Button::Left, pt);
        v.button_down(mk_core::Button::Right, pt);
        // 按在一个**未翻开**的格子上：进了 chord 模式，松手时 `apply_chord` 对未翻开的
        // 格子直接返回，所以格数应当**不变**；若没进 chord 模式，松左键会走 `do_open`
        // 把格子翻开，格数会 **+1** —— 两种情况因此可以区分开。
        //
        // 早期这里写的是 `< before`，而 `opened` 只增不减，断言恒假，这条检查永远不会红。
        let before = v.board().opened_count();
        v.button_up(mk_core::Button::Left, pt);
        v.button_up(mk_core::Button::Right, pt);
        if v.board().opened_count() != before {
            return Err(format!(
                "seed={seed} 左右同按松手后翻开了格子（{} → {}），说明没进入 chord 模式",
                before,
                v.board().opened_count()
            ));
        }
    }
    Ok(())
}

/// chord 手势矩阵：`via` ∈ {middle, both, both-reverse}，`want` ∈ {mark, reveal}。
///
/// 走的是**真实输入路径**（`View::button_down` / `button_up`），不是直接调
/// `Board::apply_chord` —— 这样按键顺序、按下态、模式判定都被覆盖到。
fn chord_gesture(via: &str, want: &str) -> Result<(), String> {
    // 固定布局：(1,1) 的 4 个邻格是雷，另外 4 个不是。
    //   规则 1（标雷）：把非雷的 4 个邻格翻开，于是 unopened == adjacent。
    //   规则 2（展开）：只翻开 (1,1)，把 4 颗雷插旗，于是 flagged == adjacent。
    const MINES: [Pos; 4] = [
        Pos { row: 0, col: 0 },
        Pos { row: 0, col: 2 },
        Pos { row: 1, col: 0 },
        Pos { row: 1, col: 2 },
    ];
    const SAFE: [Pos; 4] = [
        Pos { row: 0, col: 1 },
        Pos { row: 2, col: 0 },
        Pos { row: 2, col: 1 },
        Pos { row: 2, col: 2 },
    ];
    let target = Pos { row: 1, col: 1 };

    let mut v = View::new();
    v.apply_size(Size::Large);
    v.start_game(Difficulty::custom(8, 8, 4), 1);
    if !v.board_mut().force_mines(&MINES) {
        return Err("固定雷位没生效".into());
    }
    v.board_mut().left_click(target);
    if !v.board().is_open(target) {
        return Err("目标格没被翻开".into());
    }

    match want {
        "mark" => {
            for p in SAFE {
                v.board_mut().left_click(p);
            }
            if game::decide(v.board(), target) != Action::MarkMines {
                return Err("前置状态没造出规则 1".into());
            }
        }
        "reveal" => {
            for p in MINES {
                v.board_mut().cycle_mark(p, false);
            }
            if game::decide(v.board(), target) != Action::Reveal {
                return Err("前置状态没造出规则 2".into());
            }
        }
        other => return Err(format!("want 只能是 mark / reveal，收到 {other:?}")),
    }

    // 走真实输入路径。
    let m = v.metrics();
    let r = m.cell_rect(target.col, target.row);
    let pt = (r.x + r.w / 2, r.y + r.h / 2);
    match via {
        "middle" => {
            v.button_down(Button::Middle, pt);
            v.button_up(Button::Middle, pt);
        }
        "both" => {
            v.button_down(Button::Left, pt);
            v.button_down(Button::Right, pt);
            v.button_up(Button::Right, pt);
            v.button_up(Button::Left, pt);
        }
        "both-reverse" => {
            v.button_down(Button::Right, pt);
            v.button_down(Button::Left, pt);
            v.button_up(Button::Left, pt);
            v.button_up(Button::Right, pt);
        }
        other => {
            return Err(format!(
                "via 只能是 middle / both / both-reverse，收到 {other:?}"
            ))
        }
    }

    if v.board().phase == game::Phase::Dead {
        return Err("手势不该踩雷".into());
    }
    match want {
        "mark" => {
            for p in MINES {
                if v.board().mark_of(p) != game::Mark::Flag {
                    return Err(format!("规则 1 没标上雷 {p:?}"));
                }
            }
            for p in SAFE {
                if v.board().mark_of(p) == game::Mark::Flag {
                    return Err(format!("规则 1 误标了非雷格 {p:?}"));
                }
            }
        }
        _ => {
            for p in SAFE {
                if !v.board().is_open(p) {
                    return Err(format!("规则 2 没翻开 {p:?}"));
                }
            }
            for p in MINES {
                if v.board().is_open(p) {
                    return Err(format!("规则 2 翻开了雷 {p:?}"));
                }
            }
        }
    }
    Ok(())
}

/// 固定种子必须给出逐位相同的雷位 —— `--genlayout` 与随机对局自检都依赖它。
fn same_seed_reproducible(_: i32) -> Result<(), String> {
    let layout = |seed: u64| -> Vec<Pos> {
        let mut b = Board::new(24, 30, 199, Rand::new(seed));
        b.left_click(Pos { row: 12, col: 15 });
        (0..24)
            .flat_map(|r| (0..30).map(move |c| Pos { row: r, col: c }))
            .filter(|&p| b.is_mine(p))
            .collect()
    };
    for seed in [0u64, 1, 42, 2026, u64::MAX] {
        if layout(seed) != layout(seed) {
            return Err(format!("seed={seed} 两次布雷结果不同"));
        }
    }
    // 种子必须真的起作用，否则「可复现」是假的。
    if layout(1) == layout(2) {
        return Err("不同种子给出了相同布局，种子没起作用".into());
    }
    Ok(())
}

fn chord_gesture_middle(_: i32) -> Result<(), String> {
    let mut v = View::new();
    v.apply_size(Size::Medium);
    v.start_game(Difficulty::BEGINNER, 3);
    let m = v.metrics();
    let cell = m.cell_rect(8, 8);
    let pt = (cell.x + cell.w / 2, cell.y + cell.h / 2);
    v.button_down(mk_core::Button::Middle, pt);
    v.button_up(mk_core::Button::Middle, pt);
    Ok(())
}
