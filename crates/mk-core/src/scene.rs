//! 场景脚本：固定雷位 + 操作序列，跑完得到一幅可渲染的画面。
//!
//! 语法与 Go 版完全一致（`testdata/scenes/*.txt` 可以直接复用，不必改写）：
//!
//! ```text
//! board 24 30 199        # 行 列 雷数
//! size medium            # 方格尺寸：small / medium / large（或 16 / 24 / 32）
//! mines 0 0              # 固定雷位，可重复
//! open 12 20             # 左键翻开
//! flag 3 7 / question 4 8
//! chord 12 20            # 走一次 chord 手势
//! markview 12 20         # 只画「按住」预览叠加层，不落子
//! mark 20 15             # 行优先给前 20 个雷位插旗、随后 15 个设为问号
//! safe / autolost / autoview / solve
//! colored 0 | marks 0    # 关掉颜色 / 关掉问号
//! ```

use std::path::Path;

use crate::game::{self, Difficulty, Pos};
use crate::theme::Size;
use crate::view::View;

/// 一条脚本跑完之后的画面状态。
pub struct Scene {
    pub view: View,
    pub rows: i32,
    pub cols: i32,
    pub size: Size,
}

/// 解析并执行脚本文件。
pub fn run_script(path: impl AsRef<Path>, seed: u64) -> Result<Scene, String> {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    run_script_text(&text, seed)
}

/// 解析并执行脚本文本（供测试直接喂字符串）。
pub fn run_script_text(text: &str, seed: u64) -> Result<Scene, String> {
    let mut rows = Difficulty::EXPERT.rows;
    let mut cols = Difficulty::EXPERT.cols;
    let mut mines = Difficulty::EXPERT.mines;
    let mut size = Size::Large; // 脚本没写 size 时按大档，与既有基线一致
    let mut layout: Vec<Pos> = Vec::new();
    let mut cmds: Vec<(String, Vec<String>)> = Vec::new();

    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        let at = |i: usize| -> Result<i32, String> {
            f.get(i)
                .ok_or_else(|| format!("第 {} 行：字段不足", lineno + 1))?
                .parse::<i32>()
                .map_err(|_| format!("第 {} 行：{:?} 不是整数", lineno + 1, f[i]))
        };
        match f[0] {
            "board" => {
                if f.len() < 4 {
                    return Err(format!(
                        "第 {} 行：board 需要「rows cols mines」三个数",
                        lineno + 1
                    ));
                }
                rows = at(1)?;
                cols = at(2)?;
                mines = at(3)?;
            }
            "size" => {
                size = Size::parse(f.get(1).copied().unwrap_or(""))
                    .map_err(|e| format!("第 {} 行：{e}", lineno + 1))?;
            }
            "mines" => {
                if f.len() < 3 {
                    return Err(format!("第 {} 行：mines 需要行和列", lineno + 1));
                }
                layout.push(Pos::new(at(1)?, at(2)?));
            }
            _ => cmds.push((
                f[0].to_string(),
                f[1..].iter().map(|s| s.to_string()).collect(),
            )),
        }
    }

    let mut view = View::new();
    view.apply_size(size);
    view.start_game(Difficulty::custom(rows, cols, mines), seed);
    if !layout.is_empty() && !view.board_mut().force_mines(&layout) {
        return Err(format!(
            "雷位数量 {} 与 board 声明的 {mines} 不符",
            layout.len()
        ));
    }

    for (op, args) in &cmds {
        apply_cmd(&mut view, op, args)?;
    }

    Ok(Scene {
        view,
        rows,
        cols,
        size,
    })
}

fn apply_cmd(view: &mut View, op: &str, args: &[String]) -> Result<(), String> {
    let num = |i: usize| -> i32 { args.get(i).and_then(|s| s.parse::<i32>().ok()).unwrap_or(0) };
    let pos = || -> Result<Pos, String> {
        if args.len() < 2 {
            return Err(format!("{op}: 需要行和列"));
        }
        Ok(Pos::new(num(0), num(1)))
    };

    match op {
        "open" => {
            let p = pos()?;
            view.board_mut().left_click(p);
        }
        "flag" => {
            let p = pos()?;
            view.board_mut().cycle_mark(p, false);
        }
        "question" => {
            let p = pos()?;
            view.board_mut().cycle_mark(p, true);
            view.board_mut().cycle_mark(p, true);
        }
        "chord" => {
            let p = pos()?;
            view.board_mut().apply_chord(p);
        }
        "markview" => {
            let p = pos()?;
            view.preview_chord(p);
        }
        "mark" => {
            // mark n m：按行优先给前 n 个雷位插旗、随后 m 个雷位设为问号。
            let (n, m) = (num(0), num(1));
            let b = view.board_mut();
            let (rows, cols) = (b.rows, b.cols);
            let mut placed = 0;
            'outer: for r in 0..rows {
                for c in 0..cols {
                    if placed >= n + m {
                        break 'outer;
                    }
                    let p = Pos::new(r, c);
                    if !b.is_mine(p) {
                        continue;
                    }
                    if placed < n {
                        b.cycle_mark(p, false);
                    } else {
                        b.cycle_mark(p, true);
                        b.cycle_mark(p, true);
                    }
                    placed += 1;
                }
            }
        }
        "safe" => {
            // 翻开行优先遇到的第一个非雷格。
            let b = view.board_mut();
            if let Some(p) = crate::view::find_cell(b, |p| !b.is_mine(p)) {
                b.left_click(p);
            }
        }
        "autolost" => {
            // 按行优先一直翻，直到踩雷结束。
            let b = view.board_mut();
            let (rows, cols) = (b.rows, b.cols);
            'outer: for r in 0..rows {
                for c in 0..cols {
                    if b.phase == game::Phase::Dead {
                        break 'outer;
                    }
                    b.left_click(Pos::new(r, c));
                }
            }
        }
        "autoview" => {
            // 最多展开 30 个安全格，再取行优先第一个「能推断出必雷格」的已翻开数字做按住预览。
            {
                let b = view.board_mut();
                let (rows, cols) = (b.rows, b.cols);
                let mut opened = 0;
                'outer: for r in 0..rows {
                    for c in 0..cols {
                        if opened >= 30 || b.phase != game::Phase::Playing {
                            break 'outer;
                        }
                        let p = Pos::new(r, c);
                        if b.is_mine(p) {
                            continue;
                        }
                        b.left_click(p);
                        opened += 1;
                    }
                }
            }
            let target = crate::view::find_cell(view.board(), |p| {
                view.board().is_open(p) && !view.board().certain_mines(p).is_empty()
            });
            match target {
                Some(p) => view.preview_chord(p),
                None => return Err("找不到可推断的按住预览位置".to_string()),
            }
        }
        "colored" => view.colored = num(0) != 0,
        "marks" => view.allow_question = num(0) != 0,
        "solve" => {
            // 把每个非雷格先插旗再取消，然后翻开。
            let b = view.board_mut();
            let (rows, cols) = (b.rows, b.cols);
            for r in 0..rows {
                for c in 0..cols {
                    let p = Pos::new(r, c);
                    if b.is_mine(p) || b.is_open(p) {
                        continue;
                    }
                    b.cycle_mark(p, false);
                    b.cycle_mark(p, false);
                    b.left_click(p);
                }
            }
        }
        other => return Err(format!("未知命令 {other:?}")),
    }
    Ok(())
}

/// 用当前布雷算法生成一份脚本头（`board` + `mines`），供生成新场景使用。
pub fn gen_layout(rows: i32, cols: i32, mines: i32, seed: u64) -> String {
    let mut b = game::Board::new(rows, cols, mines, game::Rand::new(seed));
    b.left_click(Pos::new(rows / 2, cols / 2));

    let mut out =
        format!("# 自动生成的固定雷位（seed={seed}）\nboard {rows} {cols} {mines}\nsize large\n");
    for r in 0..rows {
        for c in 0..cols {
            if b.is_mine(Pos::new(r, c)) {
                out.push_str(&format!("mines {r} {c}\n"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_script_runs() {
        let s = run_script_text("board 8 8 10\nsize small\nsafe\n", 1).unwrap();
        assert_eq!(s.rows, 8);
        assert_eq!(s.size, Size::Small);
        assert_eq!(s.view.board().mines, 10);
    }

    #[test]
    fn fixed_mine_layout_is_honoured() {
        // 5×5 只有两颗雷，(2,2) 的相邻计数为 0，一开就会展开到全盘并判胜 ——
        // 所以这里只断言雷位与「已经开局」，不断言具体的 phase。
        let text = "board 5 5 2\nmines 0 0\nmines 4 4\nopen 2 2\n";
        let s = run_script_text(text, 1).unwrap();
        assert!(s.view.board().is_mine(Pos::new(0, 0)));
        assert!(s.view.board().is_mine(Pos::new(4, 4)));
        assert_ne!(s.view.board().phase, game::Phase::Ready);
        assert!(s.view.board().field_placed());
    }

    /// 雷位声明与 board 的雷数不符时必须拒绝，而不是画出一幅对不上的画面。
    #[test]
    fn layout_and_count_must_agree() {
        let text = "board 8 8 3\nmines 0 0\nmines 7 7\n";
        assert!(run_script_text(text, 1).is_err());
    }

    #[test]
    fn wrong_mine_count_is_rejected() {
        let text = "board 5 5 2\nmines 0 0\n";
        assert!(run_script_text(text, 1).is_err());
    }

    #[test]
    fn unknown_command_is_rejected() {
        assert!(run_script_text("board 5 5 2\nnope 1 2\n", 1).is_err());
    }

    #[test]
    fn gen_layout_round_trips() {
        let head = gen_layout(8, 8, 10, 42);
        let scene = run_script_text(&head, 1).unwrap();
        let n = (0..8)
            .flat_map(|r| (0..8).map(move |c| Pos::new(r, c)))
            .filter(|&p| scene.view.board().is_mine(p))
            .count();
        assert_eq!(n, 10);
    }
}
