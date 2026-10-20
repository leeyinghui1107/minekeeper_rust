//! XP 3D 边框原语。
//!
//! 凸起格的斜接规则是从截图逐像素反推出来的：
//! `白 = (X<B || Y<B) && X+Y <= N-B`，`灰 = (X>=N-B || Y>=N-B) && X+Y >= N`。
//! 全部用整数多边形表达，坐标只经 `Metrics` 取整，因此三档下都不出现半像素。
//!
//! 注意 [`draw_raised_cell`] 走的是**偶奇填充**（`Canvas::fill_polygon`），
//! 四个梯形两两相邻但不重叠 —— 这是与原版逐像素对齐的关键。

use crate::raster::Canvas;
use crate::theme::{self, Metrics, Rect};

/// 铺一片窗体灰。
pub fn fill_face(dst: &mut Canvas, r: Rect) {
    dst.fill_rect(r, theme::FACE);
}

/// 画未翻开的凸起格。
pub fn draw_raised_cell(dst: &mut Canvas, r: Rect, m: &Metrics) {
    let n = r.w;
    let b = m.bevel_px();
    if n <= 0 || b <= 0 || b * 2 >= n {
        // 退化尺寸（极小档或非法行数）直接铺面色，避免多边形自交。
        fill_face(dst, r);
        return;
    }

    // 四个梯形的顶点按 16px 基准逻辑书写后换算成设备像素，再整体平移到格子原点。
    let top = [(0, 0), (n - b + 1, 0), (n - 2 * b + 1, b), (0, b)];
    let left = [(0, 0), (b, 0), (b, n - 2 * b + 1), (0, n - b + 1)];
    let right = [(n - b, b), (n, 0), (n, n), (n - b, n)];
    let bottom = [(b, n - b), (n, n - b), (n, n), (0, n)];

    dst.fill_polygon(&translate(r, &top), theme::HILITE);
    dst.fill_polygon(&translate(r, &left), theme::HILITE);
    dst.fill_polygon(&translate(r, &right), theme::SHADOW);
    dst.fill_polygon(&translate(r, &bottom), theme::SHADOW);
}

/// 画已翻开的格子：上、左各一条灰线。
pub fn draw_sunken_cell(dst: &mut Canvas, r: Rect, m: &Metrics) {
    let t = m.sunken_line_px();
    dst.fill_rect(Rect::new(r.x, r.y, r.w, t), theme::SHADOW);
    dst.fill_rect(Rect::new(r.x, r.y, t, r.h), theme::SHADOW);
}

/// 画通用凹槽矩形（雷区 3、状态栏 2、数码管 1）。灰在左上，白在右下。
pub fn draw_sunken_rect(dst: &mut Canvas, r: Rect, t: i32) {
    let t = t.max(1);
    dst.fill_rect(Rect::new(r.x, r.y, r.w, t), theme::SHADOW);
    dst.fill_rect(Rect::new(r.x, r.y, t, r.h), theme::SHADOW);
    dst.fill_rect(Rect::new(r.x, r.bottom() - t, r.w, t), theme::HILITE);
    dst.fill_rect(Rect::new(r.right() - t, r.y, t, r.h), theme::HILITE);
}

/// 画通用凸起矩形。
pub fn draw_raised_rect(dst: &mut Canvas, r: Rect, t: i32) {
    let t = t.max(1);
    dst.fill_rect(Rect::new(r.x, r.y, r.w, t), theme::HILITE);
    dst.fill_rect(Rect::new(r.x, r.y, t, r.h), theme::HILITE);
    dst.fill_rect(Rect::new(r.x, r.bottom() - t, r.w, t), theme::SHADOW);
    dst.fill_rect(Rect::new(r.right() - t, r.y, t, r.h), theme::SHADOW);
}

/// 画笑脸按钮：外圈 1px 灰框 + 内圈凸起（按下时改凹槽）。
pub fn draw_button(dst: &mut Canvas, r: Rect, m: &Metrics, pressed: bool) {
    let f = m.frame_px();
    dst.fill_rect(Rect::new(r.x, r.y, r.w, f), theme::SHADOW);
    dst.fill_rect(Rect::new(r.x, r.bottom() - f, r.w, f), theme::SHADOW);
    dst.fill_rect(Rect::new(r.x, r.y, f, r.h), theme::SHADOW);
    dst.fill_rect(Rect::new(r.right() - f, r.y, f, r.h), theme::SHADOW);

    let inner = r.inset(f);
    if pressed {
        draw_sunken_rect(dst, inner, m.bevel_px());
    } else {
        draw_raised_rect(dst, inner, m.bevel_px());
    }
}

fn translate(r: Rect, pts: &[(i32, i32)]) -> Vec<(i32, i32)> {
    pts.iter().map(|&(x, y)| (r.x + x, r.y + y)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster::Canvas;
    use crate::theme::{Size, HILITE, SHADOW};

    fn px(c: &Canvas, x: i32, y: i32) -> (u8, u8, u8) {
        let i = ((y as u32 * c.width() as u32 + x as u32) * 4) as usize;
        (c.data()[i], c.data()[i + 1], c.data()[i + 2])
    }

    /// 小档（16px，B=2）下斜接规则必须与实测逐像素相符。
    #[test]
    fn small_bevel_matches_measured_rule() {
        let m = Metrics::new(Size::Small);
        let n = 16;
        let mut c = Canvas::new(n, n).unwrap();
        c.clear(theme::FACE);
        draw_raised_cell(&mut c, Rect::new(0, 0, n, n), &m);

        let b = m.bevel_px();
        for y in 0..n {
            for x in 0..n {
                let want_white = (x < b || y < b) && x + y <= n - b;
                let want_shadow = (x >= n - b || y >= n - b) && x + y >= n;
                let got = px(&c, x, y);
                if want_white {
                    assert_eq!(got, (HILITE.0, HILITE.1, HILITE.2), "({x},{y}) 应为白");
                } else if want_shadow {
                    assert_eq!(got, (SHADOW.0, SHADOW.1, SHADOW.2), "({x},{y}) 应为灰");
                } else {
                    assert_eq!(
                        got,
                        (theme::FACE.0, theme::FACE.1, theme::FACE.2),
                        "({x},{y}) 应为面色"
                    );
                }
            }
        }
    }

    /// 中/大档只要求落在调色板内（取整后斜接会变形，不逐像素断言）。
    #[test]
    fn medium_and_large_bevel_stay_in_palette() {
        for size in [Size::Medium, Size::Large] {
            let m = Metrics::new(size);
            let n = m.cell_px();
            let mut c = Canvas::new(n, n).unwrap();
            c.clear(theme::FACE);
            draw_raised_cell(&mut c, Rect::new(0, 0, n, n), &m);
            for y in 0..n {
                for x in 0..n {
                    let got = px(&c, x, y);
                    let ok = got == (HILITE.0, HILITE.1, HILITE.2)
                        || got == (SHADOW.0, SHADOW.1, SHADOW.2)
                        || got == (theme::FACE.0, theme::FACE.1, theme::FACE.2);
                    assert!(
                        got != (0, 0, 0) && ok,
                        "{size:?} ({x},{y}) 越出色板 {got:?}"
                    );
                }
            }
        }
    }
}
