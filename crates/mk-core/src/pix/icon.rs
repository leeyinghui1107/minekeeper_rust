//! 应用图标。
//!
//! 用与游戏内**完全相同**的绘制代码现画（凸起格 + 雷），不附带任何 `.ico` / `.icns` 资源。
//! 平台侧只负责把这块缓冲变成平台图标格式（macOS: icns；Windows: `CreateIcon`；
//! Linux: PNG + WM_CLASS）。

use crate::raster::Canvas;
use crate::theme::Metrics;

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

/// 默认图标尺寸。
pub const DEFAULT_ICON_SIZE: i32 = 32;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_renders_at_any_size() {
        for size in [16, 32, 128, 512] {
            let c = app_icon(size).expect("应能建画布");
            assert_eq!(c.width(), size);
            assert_eq!(c.height(), size);
            // 必须真的画了东西。按底色判定 —— fill_face 铺满后画布不透明，
            // ink_bounds（按 alpha）恒为整块画布，那样断言是恒真的。
            let (_, _, w, h) = c.ink_bounds_on(crate::theme::FACE);
            assert!(w > size / 2 && h > size / 2, "{size}: 图标几乎空白 {w}x{h}");
        }
    }

    #[test]
    fn degenerate_size_is_rejected() {
        assert!(app_icon(0).is_none());
        assert!(app_icon(-1).is_none());
    }
}
