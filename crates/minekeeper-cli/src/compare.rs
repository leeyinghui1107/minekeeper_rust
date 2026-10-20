//! 两张 PNG 的逐像素比较，按画面分区报告差异。
//!
//! 分区口径：状态栏 / 雷区 / 其它。跨后端比较时，允许的差异只应出现在
//! 「格内字形」与「笑脸面部」两处 —— 判定规则见设计文档 §10.3。

use std::path::Path;

use mk_core::theme::{Metrics, Size};

/// 分区差异统计。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Diff {
    pub total: usize,
    pub status: usize,
    pub field: usize,
    pub other: usize,
}

impl Diff {
    pub fn is_identical(&self) -> bool {
        self.total == 0
    }
}

/// 返回差异像素数；尺寸不同视为整体失败。
pub fn compare(a: &Path, b: &Path, rows: i32, cols: i32, size: Size) -> Result<usize, String> {
    let ia = load(a)?;
    let ib = load(b)?;
    let d = diff_images(&ia, &ib, rows, cols, size)?;
    if !d.is_identical() {
        println!("总差异 {} 像素（{}×{}）", d.total, ia.width(), ia.height());
        println!("  状态栏 {}", d.status);
        println!("  雷区   {}", d.field);
        println!("  其它   {}", d.other);
    }
    Ok(d.total)
}

/// 比较两张已解码的图，不打印。`--golden` 走这条。
pub fn diff_images(
    ia: &image::RgbaImage,
    ib: &image::RgbaImage,
    rows: i32,
    cols: i32,
    size: Size,
) -> Result<Diff, String> {
    if ia.dimensions() != ib.dimensions() {
        return Err(format!(
            "尺寸不同：{:?} vs {:?}",
            ia.dimensions(),
            ib.dimensions()
        ));
    }
    let (w, h) = ia.dimensions();
    let (pa, pb) = (ia.as_raw(), ib.as_raw());

    let m = Metrics::new(size);
    let status = m.status_rect(cols);
    let field = m.field_rect(cols, rows);

    let mut d = Diff::default();
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let i = ((y as u32 * w + x as u32) * 4) as usize;
            if pa[i..i + 4] == pb[i..i + 4] {
                continue;
            }
            d.total += 1;
            if status.contains(x, y) {
                d.status += 1;
            } else if field.contains(x, y) {
                d.field += 1;
            } else {
                d.other += 1;
            }
        }
    }
    Ok(d)
}

/// 读一张 PNG。
pub fn load(p: &Path) -> Result<image::RgbaImage, String> {
    image::open(p)
        .map(|i| i.to_rgba8())
        .map_err(|e| format!("{}: {e}", p.display()))
}
