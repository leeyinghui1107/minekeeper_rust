//! MineKeeper 无窗口验证栈。
//!
//! **不依赖窗口系统** —— 可以在没有显示设备的 CI 上跑全部自检、出图与逐像素比对。
//! 这是整个跨平台方案里最先能落地、也最有价值的一块。
//!
//! ```text
//! minekeeper-cli --selftest [n]                无窗口自检
//! minekeeper-cli --script IN OUT               按脚本渲染一帧 PNG
//! minekeeper-cli --compare A B [rows cols size] 逐像素比较两张 PNG
//! minekeeper-cli --genlayout OUT [rows cols mines seed]
//! minekeeper-cli --glyphdiag                   打印字形适配结果
//! ```

use std::path::Path;
use std::process::ExitCode;

use mk_core::pix::FontStack;
use mk_core::scene;
use mk_core::{Canvas, Metrics, Size};

mod checks;
mod compare;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        usage();
        return ExitCode::from(2);
    }

    match args[0].as_str() {
        "--selftest" => {
            let n = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1000);
            if checks::self_test(n) > 0 {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        "--script" => {
            if args.len() < 3 {
                eprintln!("用法：--script <脚本> <输出png>");
                return ExitCode::from(2);
            }
            match render_script(Path::new(&args[1]), Path::new(&args[2])) {
                Ok((w, h)) => {
                    println!("已写出 {}  {w}×{h}", args[2]);
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("脚本失败：{e}");
                    ExitCode::FAILURE
                }
            }
        }
        "--compare" => {
            if args.len() < 3 {
                eprintln!("用法：--compare <a.png> <b.png> [rows cols size]");
                return ExitCode::from(2);
            }
            let rows = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(24);
            let cols = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(30);
            let size = args
                .get(5)
                .map(|s| Size::parse(s))
                .transpose()
                .unwrap_or(Some(Size::Large))
                .unwrap_or(Size::Large);
            match compare::compare(Path::new(&args[1]), Path::new(&args[2]), rows, cols, size) {
                Ok(0) => ExitCode::SUCCESS,
                Ok(n) => {
                    eprintln!("差异像素 {n}");
                    ExitCode::from(3)
                }
                Err(e) => {
                    eprintln!("比较失败：{e}");
                    ExitCode::FAILURE
                }
            }
        }
        "--genlayout" => {
            if args.len() < 2 {
                eprintln!("用法：--genlayout <输出> [rows cols mines seed]");
                return ExitCode::from(2);
            }
            let g = |i: usize, d: i32| args.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
            let rows = g(2, 24);
            let cols = g(3, 30);
            let mines = g(4, 199);
            let seed = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(12345u64);
            let text = scene::gen_layout(rows, cols, mines, seed);
            match std::fs::write(&args[1], text) {
                Ok(()) => {
                    println!("已生成 {}", args[1]);
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("生成失败：{e}");
                    ExitCode::FAILURE
                }
            }
        }
        "--ui" => {
            if args.len() < 2 {
                eprintln!("用法：--ui <输出png> [menu|help|search|using|about|name|custom|scores]");
                return ExitCode::from(2);
            }
            let what = args.get(2).map(|s| s.as_str()).unwrap_or("menu");
            match render_ui(Path::new(&args[1]), what) {
                Ok(()) => {
                    println!("已写出 {}", args[1]);
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("出图失败：{e}");
                    ExitCode::FAILURE
                }
            }
        }
        "--golden" => {
            let update = args.iter().any(|a| a == "--update");
            let paths: Vec<&str> = args[1..]
                .iter()
                .filter(|a| !a.starts_with("--"))
                .map(String::as_str)
                .collect();
            let scenes = paths
                .first()
                .map(Path::new)
                .unwrap_or(Path::new("testdata/scenes"));
            let golden = paths
                .get(1)
                .map(Path::new)
                .unwrap_or(Path::new("testdata/golden"));
            match golden_run(scenes, golden, update) {
                Ok(0) => ExitCode::SUCCESS,
                Ok(n) => {
                    eprintln!("{n} 个场景与基线不符");
                    ExitCode::from(3)
                }
                Err(e) => {
                    eprintln!("基线失败：{e}");
                    ExitCode::FAILURE
                }
            }
        }
        "--icon" => {
            if args.len() < 2 {
                eprintln!("用法：--icon <输出png> [size]");
                return ExitCode::from(2);
            }
            let size = args
                .get(2)
                .and_then(|s| s.parse().ok())
                .unwrap_or(mk_core::pix::DEFAULT_ICON_SIZE);
            let Some(cv) = mk_core::pix::app_icon(size) else {
                eprintln!("无法建 {size}×{size} 画布");
                return ExitCode::FAILURE;
            };
            match write_png(Path::new(&args[1]), &cv) {
                Ok(()) => {
                    println!("已写出 {}  {size}×{size}", args[1]);
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("写图失败：{e}");
                    ExitCode::FAILURE
                }
            }
        }
        "--glyphdiag" => {
            let m = Metrics::new(Size::Large);
            let mut fonts = FontStack::load_default();
            if fonts.is_empty() {
                eprintln!("找不到可用字体");
                return ExitCode::FAILURE;
            }
            for line in fonts.diag(&m) {
                println!("{line}");
            }
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("未知参数：{other}");
            usage();
            ExitCode::from(2)
        }
    }
}

/// 一帧渲染结果 + 它所属场景的几何（分区比对要用）。
struct Rendered {
    canvas: Canvas,
    rows: i32,
    cols: i32,
    size: Size,
}

/// 按脚本渲染一帧。`--script` 与 `--golden` 共用这条路径。
fn render_scene(input: &Path) -> Result<Rendered, String> {
    let s = scene::run_script(input, 1)?;
    let mut fonts = FontStack::load_default();
    let (w, h) = (s.view.client_width(), s.view.client_height());
    let mut cv = Canvas::new(w, h).ok_or_else(|| format!("无法建 {w}×{h} 画布"))?;
    let bounds = cv.bounds();
    s.view.paint(&mut cv, bounds, &mut fonts);
    Ok(Rendered {
        canvas: cv,
        rows: s.rows,
        cols: s.cols,
        size: s.size,
    })
}

fn render_script(input: &Path, output: &Path) -> Result<(i32, i32), String> {
    let r = render_scene(input)?;
    let (w, h) = (r.canvas.width(), r.canvas.height());
    write_png(output, &r.canvas)?;
    Ok((w, h))
}

/// 基线目录里不产基线的场景前缀。
///
/// `layout-*` 只是 `--genlayout` 产出的雷位输入，`bevel-*` 是调试凸起格斜接用的
/// 8×8 小图 —— 两者都不该有基线，与 Go 版的划分一致。
const NO_GOLDEN_PREFIXES: [&str; 2] = ["layout-", "bevel-"];

/// 逐场景渲染并与基线比对；`update` 为真时改写基线。
///
/// 返回失败场景数。
fn golden_run(scenes: &Path, golden: &Path, update: bool) -> Result<usize, String> {
    let mut names: Vec<String> = std::fs::read_dir(scenes)
        .map_err(|e| format!("{}：{e}", scenes.display()))?
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|n| n.ends_with(".txt"))
        .filter(|n| !NO_GOLDEN_PREFIXES.iter().any(|p| n.starts_with(p)))
        .collect();
    names.sort();
    if names.is_empty() {
        return Err(format!("{} 里没有场景脚本", scenes.display()));
    }

    let mut failed = 0usize;
    for name in &names {
        let stem = name.trim_end_matches(".txt");
        let golden_path = golden.join(format!("{stem}.png"));
        let r = render_scene(&scenes.join(name))?;

        if update {
            write_png(&golden_path, &r.canvas)?;
            println!("  重录 {stem}.png");
            continue;
        }

        if !golden_path.exists() {
            println!("  ✘ {stem}：基线缺失（用 --golden --update 重录）");
            failed += 1;
            continue;
        }

        let want = compare::load(&golden_path)?;
        let got = image::RgbaImage::from_raw(
            r.canvas.width() as u32,
            r.canvas.height() as u32,
            r.canvas.to_rgba(),
        )
        .ok_or("画布尺寸与缓冲长度不符")?;
        match compare::diff_images(&want, &got, r.rows, r.cols, r.size) {
            Ok(d) if d.is_identical() => {}
            Ok(d) => {
                println!(
                    "  ✘ {stem}：{} 像素不同（状态栏 {} / 雷区 {} / 其它 {}）",
                    d.total, d.status, d.field, d.other
                );
                failed += 1;
            }
            Err(e) => {
                println!("  ✘ {stem}：{e}");
                failed += 1;
            }
        }
    }

    if update {
        println!("已重录 {} 张基线到 {}", names.len(), golden.display());
    } else {
        println!(
            "基线校验：{} 张，通过 {}，失败 {failed}",
            names.len(),
            names.len() - failed
        );
    }
    Ok(failed)
}

/// 离屏渲染「菜单栏 + 游戏画面（+ 对话框）」的一帧，用于目视核对界面。
///
/// 走的是与 GUI 完全相同的合成路径：游戏画进自己的画布，再贴到菜单栏之下。
fn render_ui(output: &Path, what: &str) -> Result<(), String> {
    use mk_core::chrome::{Chrome, ChromeEvent};
    use mk_core::text::TextRenderer;
    use mk_core::view::View;

    let mut view = View::new();
    view.apply_size(Size::Medium);
    view.start_game(mk_core::game::Difficulty::EXPERT, 2026);
    // 先开几个格子，让画面有内容。
    for r in 3..6 {
        for c in 3..12 {
            view.board_mut().left_click(mk_core::game::Pos::new(r, c));
        }
    }

    let mut chrome = Chrome::new(mk_core::chrome::menu_items());
    chrome.sync(mk_core::chrome::Options {
        size_index: 1,
        ..Default::default()
    });
    let mut text = TextRenderer::load_default();
    let mut fonts = FontStack::load_default();

    let (gw, gh) = (view.client_width(), view.client_height());
    let (ww, wh) = (gw, gh + Chrome::BAR_HEIGHT);

    match what {
        "menu" => {
            // 展开「游戏」菜单并高亮「中级」
            chrome.handle(ChromeEvent::Click(10, 8), ww, wh, &mut text);
            chrome.handle(ChromeEvent::Move(40, 60), ww, wh, &mut text);
        }
        // 与 GUI 共用同一份文案 —— 早期这里是一段缩短的副本，两边会走偏。
        "help" => chrome.open_message("扫雷帮助 · 目录", mk_core::chrome::HELP_CONTENTS),
        "search" => chrome.open_message("扫雷帮助 · 查找主题", mk_core::chrome::HELP_SEARCH),
        "using" => chrome.open_message("扫雷帮助 · 使用帮助", mk_core::chrome::HELP_USING),
        "about" => chrome.open_message("关于扫雷", mk_core::chrome::ABOUT_TEXT),
        "name" => chrome.open_name("已破高级记录。\n请留尊姓大名。", "匿名"),
        "custom" => chrome.open_custom(24, 30, 199),
        "scores" => chrome.open_scores([
            ("12 秒".into(), "张三".into()),
            ("45 秒".into(), "李四".into()),
            ("999 秒".into(), "匿名".into()),
        ]),
        other => return Err(format!("未知界面 {other:?}")),
    }

    let mut game = Canvas::new(gw, gh).ok_or("建游戏画布失败")?;
    let bounds = game.bounds();
    view.paint(&mut game, bounds, &mut fonts);

    let mut win = Canvas::new(ww, wh).ok_or("建窗口画布失败")?;
    win.clear(mk_core::theme::FACE);
    win.set_origin(0, Chrome::BAR_HEIGHT);
    win.blit((0, 0), &game);
    win.set_origin(0, 0);
    chrome.paint(&mut win, ww, &mut text);

    write_png(output, &win)
}

/// 把画布写成 PNG（直通 RGBA8）。
fn write_png(output: &Path, cv: &Canvas) -> Result<(), String> {
    if let Some(dir) = output.parent() {
        if !dir.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(dir);
        }
    }
    let file = std::fs::File::create(output).map_err(|e| e.to_string())?;
    let enc = image::codecs::png::PngEncoder::new(std::io::BufWriter::new(file));
    image::ImageEncoder::write_image(
        enc,
        cv.data(),
        cv.width() as u32,
        cv.height() as u32,
        image::ExtendedColorType::Rgba8,
    )
    .map_err(|e| e.to_string())
}

fn usage() {
    println!(
        "minekeeper-cli —— 无窗口验证栈\n\
         \n\
         \x20 --selftest [n]                              无窗口自检（默认 1000 局）\n\
         \x20 --script <脚本> <输出png>                   按脚本渲染一帧\n\
         \x20 --compare <a.png> <b.png> [rows cols size]  逐像素比较\n\
         \x20 --genlayout <输出> [rows cols mines seed]   生成固定雷位脚本头\n\
         \x20 --golden [--update] [场景目录] [基线目录]   逐像素校验渲染基线\n\
         \x20 --icon <输出png> [size]                     画出应用图标（默认 32）\n\
         \x20 --ui <输出png> [menu|help|search|using|about|name|custom|scores]  离屏渲染界面\n\
         \x20 --glyphdiag                                 打印字形适配结果"
    );
}

#[cfg(test)]
mod tests {
    /// `cargo test` 与 `--selftest` 必须读**同一份**清单。
    ///
    /// 两份清单走偏是这类项目最常见的腐化方式 —— 自检绿了，但 `cargo test` 覆盖的是
    /// 另一批检查。这条测试把两边钉在一起。
    #[test]
    fn self_checks_are_the_single_source_of_truth() {
        let mut failed = Vec::new();
        for (name, run) in super::checks::checks() {
            if let Err(e) = run(200) {
                failed.push(format!("{name}: {e}"));
            }
        }
        assert!(failed.is_empty(), "自检失败：\n{}", failed.join("\n"));
    }

    /// 自检清单不能是空的 —— 空清单会让上一条测试永远通过。
    #[test]
    fn self_check_list_is_not_empty() {
        assert!(super::checks::checks().len() >= 15);
    }
}
