//! 应用图标。
//!
//! 用与游戏内**完全相同**的绘制代码现画（凸起格 + 雷），不附带任何 `.ico` / `.icns` 资源。
//! 平台侧只负责把这块缓冲变成平台图标格式（macOS: icns；Windows: `CreateIcon`；
//! Linux: PNG + WM_CLASS）。

use crate::raster::Canvas;
use crate::theme::{Metrics, Size};

/// 画一张 `size × size` 的图标：面色底 + 凸起格 + 雷。
///
/// 用 [`Metrics::for_cell`] 而不是三档里的某一档，这样 16 / 32 / 128 / 512 各尺寸
/// 画出来的比例完全一致（斜边都是边长的 1/8）。
pub fn app_icon(size: i32) -> Option<Canvas> {
    if size <= 0 {
        return None;
    }
    let m = Metrics::for_cell(size);
    let mut c = Canvas::new(size, size)?;
    let cell = c.bounds();
    super::bevels::fill_face(&mut c, cell);
    super::bevels::draw_raised_cell(&mut c, cell, &m);
    super::sprites::draw_mine(&mut c, cell, &m);
    Some(c)
}

/// 与 Go 版 `createAppIcon` 一致的默认图标尺寸。
pub const DEFAULT_ICON_SIZE: i32 = 32;

/// 供自检使用的尺寸档位（小/中/大各取一张）。
pub fn icon_sizes() -> [i32; 3] {
    [
        Size::Small.cell_px(),
        Size::Medium.cell_px(),
        Size::Large.cell_px(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_renders_at_any_size() {
        for size in [16, 32, 128, 512] {
            let c = app_icon(size).expect("应能建画布");
            assert_eq!(c.width(), size);
            assert_eq!(c.height(), size);
            // 必须真的画了东西：墨迹包围盒应该覆盖相当大一块。
            let (_, _, w, h) = c.ink_bounds();
            assert!(w > size / 2 && h > size / 2, "{size}: 图标几乎空白 {w}x{h}");
        }
    }

    #[test]
    fn degenerate_size_is_rejected() {
        assert!(app_icon(0).is_none());
        assert!(app_icon(-1).is_none());
    }
}
