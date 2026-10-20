//! 光栅层：一块 **RGBA8** 缓冲 + 全部图元（字节序 R,G,B,A —— 与 `tiny-skia` 一致，
//! 也与 PNG 编码器直接兼容；早期注释里写的 BGRA 是错的）。
//!
//! # 为什么只有一个后端
//!
//! 如果按平台各用一个光栅器（Windows 走 GDI+、其他平台自研），就会得到
//! **两套基线、跨后端只能做容差比较**。这里反过来：三个平台共用 `tiny-skia`
//! （纯 Rust 的 Skia 子集），于是
//!
//! * 渲染结果**逐像素确定且跨平台一致** —— 一套基线，CI 上精确比对；
//! * 不需要 `Canvas` trait，具体类型即可，省掉动态派发与 vtable；
//! * 与显示设备完全解耦 —— 离屏出图与窗口呈现走的是同一条路径。
//!
//! # 两条契约
//!
//! 1. **硬边优先**：除笑脸面部外，所有图元都以 `anti_alias = false` 绘制。
//!    抗锯齿会引入灰阶，破坏「边缘保持整数像素」的承诺。
//! 2. **填充规则显式指定**：凸起格斜边依赖**偶奇规则**（`FillRule::EvenOdd`），
//!    与 GDI+ 的 `FillModeAlternate` 同口径；其余用非零环绕（`Winding`）。

use tiny_skia::{
    FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Rect as SkRect, Shader, Stroke,
    Transform,
};

use crate::theme::Rgb;

/// 离屏画布。内部是 premultiplied RGBA8、自上而下的连续缓冲。
pub struct Canvas {
    pix: Pixmap,
    /// 绘制原点。菜单栏占据窗口顶部时，游戏画面整体下移 —— 用平移变换实现，
    /// 这样 `view` 里的坐标**一个都不用改**。
    origin: Transform,
}

impl Canvas {
    /// 建 `w × h` 画布。尺寸非正或过大时返回 `None`。
    pub fn new(w: i32, h: i32) -> Option<Canvas> {
        if w <= 0 || h <= 0 {
            return None;
        }
        Pixmap::new(w as u32, h as u32).map(|pix| Canvas {
            pix,
            origin: Transform::identity(),
        })
    }

    /// 设定绘制原点（设备像素）。所有图元与合成都会整体平移。
    pub fn set_origin(&mut self, x: i32, y: i32) {
        self.origin = Transform::from_translate(x as f32, y as f32);
    }

    pub fn width(&self) -> i32 {
        self.pix.width() as i32
    }
    pub fn height(&self) -> i32 {
        self.pix.height() as i32
    }
    pub fn bounds(&self) -> crate::theme::Rect {
        crate::theme::Rect::new(0, 0, self.width(), self.height())
    }

    /// 只读访问原始像素（premultiplied RGBA8）。
    pub fn data(&self) -> &[u8] {
        self.pix.data()
    }
    pub fn data_mut(&mut self) -> &mut [u8] {
        self.pix.data_mut()
    }

    /// 铺满整块画布。
    pub fn clear(&mut self, c: Rgb) {
        self.pix.fill(color_of(c));
    }

    /// 填充矩形（左上含、右下不含）。
    pub fn fill_rect(&mut self, r: crate::theme::Rect, c: Rgb) {
        if r.is_empty() {
            return;
        }
        let Some(sk) = SkRect::from_xywh(r.x as f32, r.y as f32, r.w as f32, r.h as f32) else {
            return;
        };
        let path = PathBuilder::from_rect(sk);
        self.pix.fill_path(
            &path,
            &paint(c, false),
            FillRule::Winding,
            self.origin,
            None,
        );
    }

    /// 用**偶奇规则**填充多边形 —— 凸起格斜边依赖它。
    pub fn fill_polygon(&mut self, pts: &[(i32, i32)], c: Rgb) {
        if pts.len() < 3 {
            return;
        }
        let mut pb = PathBuilder::new();
        pb.move_to(pts[0].0 as f32, pts[0].1 as f32);
        for p in &pts[1..] {
            pb.line_to(p.0 as f32, p.1 as f32);
        }
        pb.close();
        let Some(path) = pb.finish() else { return };
        self.pix.fill_path(
            &path,
            &paint(c, false),
            FillRule::EvenOdd,
            self.origin,
            None,
        );
    }

    /// 填充椭圆（外接矩形口径）。`aa` 只在笑脸面部为 true。
    pub fn fill_ellipse(&mut self, r: crate::theme::Rect, c: Rgb, aa: bool) {
        if r.is_empty() {
            return;
        }
        let Some(sk) = SkRect::from_xywh(r.x as f32, r.y as f32, r.w as f32, r.h as f32) else {
            return;
        };
        let Some(path) = PathBuilder::from_oval(sk) else {
            return;
        };
        self.pix
            .fill_path(&path, &paint(c, aa), FillRule::Winding, self.origin, None);
    }

    /// 描边椭圆。
    pub fn stroke_ellipse(&mut self, r: crate::theme::Rect, pen: i32, c: Rgb) {
        if r.is_empty() {
            return;
        }
        let Some(sk) = SkRect::from_xywh(r.x as f32, r.y as f32, r.w as f32, r.h as f32) else {
            return;
        };
        let Some(path) = PathBuilder::from_oval(sk) else {
            return;
        };
        self.pix.stroke_path(
            &path,
            &paint(c, false),
            &Stroke {
                width: pen.max(1) as f32,
                ..Default::default()
            },
            self.origin,
            None,
        );
    }

    /// 画一条带宽度（设备像素）的直线。
    pub fn stroke_line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, w: i32, c: Rgb) {
        let mut pb = PathBuilder::new();
        pb.move_to(x1 as f32, y1 as f32);
        pb.line_to(x2 as f32, y2 as f32);
        let Some(path) = pb.finish() else { return };
        self.pix.stroke_path(
            &path,
            &paint(c, false),
            &Stroke {
                width: w.max(1) as f32,
                line_cap: LineCap::Butt,
                line_join: LineJoin::Miter,
                ..Default::default()
            },
            self.origin,
            None,
        );
    }

    /// 描边整数矩形。
    pub fn stroke_rect(&mut self, r: crate::theme::Rect, pen: i32, c: Rgb) {
        if r.is_empty() {
            return;
        }
        let Some(sk) = SkRect::from_xywh(r.x as f32, r.y as f32, r.w as f32, r.h as f32) else {
            return;
        };
        let path = PathBuilder::from_rect(sk);
        self.pix.stroke_path(
            &path,
            &paint(c, false),
            &Stroke {
                width: pen.max(1) as f32,
                ..Default::default()
            },
            self.origin,
            None,
        );
    }

    /// 沿浮点外接矩形画弧，角度口径与 GDI+ 一致（0° 在 3 点方向，顺时针为正）。
    ///
    /// 用折线逼近：`tiny-skia` 没有 `DrawArc`，而 GDI+ 的弧线量化本来也无法复现，
    /// 所以这里按固定步长采样，保证**跨平台确定**。
    #[allow(clippy::too_many_arguments)]
    pub fn stroke_arc(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        start_deg: f32,
        sweep_deg: f32,
        pen: i32,
        c: Rgb,
    ) {
        const STEPS: usize = 48;
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        let (rx, ry) = (w / 2.0, h / 2.0);
        let mut pb = PathBuilder::new();
        for i in 0..=STEPS {
            let deg = start_deg + sweep_deg * (i as f32 / STEPS as f32);
            let a = deg.to_radians();
            let px = cx + rx * a.cos();
            let py = cy + ry * a.sin();
            if i == 0 {
                pb.move_to(px, py);
            } else {
                pb.line_to(px, py);
            }
        }
        let Some(path) = pb.finish() else { return };
        self.pix.stroke_path(
            &path,
            &paint(c, pen.max(1) as f32 > 1.0),
            &Stroke {
                width: pen.max(1) as f32,
                ..Default::default()
            },
            self.origin,
            None,
        );
    }

    /// 按覆盖率把一个颜色混进单个像素（`alpha` 取 0..=255）。
    ///
    /// **只给界面文本用**（菜单栏、对话框）—— 小字号下二值化会把中文笔画糊成一团。
    /// 游戏画面不走这里：见模块文档的两条契约。
    ///
    /// 缓冲是 premultiplied RGBA，但画布全程 alpha = 255，所以此时 premultiplied
    /// 与直通 RGBA 等价，直接按字节混合即可。
    pub fn blend_pixel(&mut self, x: i32, y: i32, c: Rgb, alpha: u8) {
        let (x, y) = (x + self.origin.tx as i32, y + self.origin.ty as i32);
        if alpha == 0 || x < 0 || y < 0 || x >= self.width() || y >= self.height() {
            return;
        }
        let a = alpha as u32;
        let inv = 255 - a;
        let w = self.width() as u32;
        let i = ((y as u32 * w + x as u32) * 4) as usize;
        let d = self.data_mut();
        d[i] = ((c.0 as u32 * a + d[i] as u32 * inv) / 255) as u8;
        d[i + 1] = ((c.1 as u32 * a + d[i + 1] as u32 * inv) / 255) as u8;
        d[i + 2] = ((c.2 as u32 * a + d[i + 2] as u32 * inv) / 255) as u8;
        d[i + 3] = 255;
    }

    /// 1:1 色键合成：把 `src` 贴在 `at`，**只写非透明像素**。
    ///
    /// 等价于一次不做缩放的位图拷贝。数字精灵只有硬边填充，像素只会是「未着墨」或
    /// 画笔色，所以逐像素比 alpha 即可，不涉及混合、也不会引入灰边。
    pub fn blit(&mut self, at: (i32, i32), src: &Canvas) {
        let at = (at.0 + self.origin.tx as i32, at.1 + self.origin.ty as i32);
        let (dw, dh) = (self.width(), self.height());
        let (sw, sh) = (src.width(), src.height());
        let src_px = src.data();
        let dst_px = self.data_mut();
        for y in 0..sh {
            let dy = at.1 + y;
            if dy < 0 || dy >= dh {
                continue;
            }
            for x in 0..sw {
                let dx = at.0 + x;
                if dx < 0 || dx >= dw {
                    continue;
                }
                let si = ((y as u32 * sw as u32 + x as u32) * 4) as usize;
                if src_px[si + 3] == 0 {
                    continue;
                }
                let di = ((dy as u32 * dw as u32 + dx as u32) * 4) as usize;
                dst_px[di..di + 4].copy_from_slice(&src_px[si..si + 4]);
            }
        }
    }

    /// 报告一个像素是否着墨（alpha 非零）。供字形墨迹包围盒使用。
    pub fn inked(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x >= self.width() || y >= self.height() {
            return false;
        }
        let i = ((y as u32 * self.width() as u32 + x as u32) * 4) as usize;
        self.data()[i + 3] != 0
    }

    /// 扫出真正着墨的包围盒，返回 `(x, y, w, h)`；全空返回 `(0, 0, 0, 0)`。
    pub fn ink_bounds(&self) -> (i32, i32, i32, i32) {
        let (w, h) = (self.width(), self.height());
        let data = self.data();
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, -1i32, -1i32);
        for y in 0..h {
            for x in 0..w {
                if data[((y as u32 * w as u32 + x as u32) * 4) as usize + 3] == 0 {
                    continue;
                }
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
        if x1 < x0 {
            return (0, 0, 0, 0);
        }
        (x0, y0, x1 - x0 + 1, y1 - y0 + 1)
    }

    /// 与给定**底色**不同的像素的包围盒。
    ///
    /// [`Canvas::ink_bounds`] 按 alpha 判定「着墨」，但 [`Canvas::clear`] 会把整块画布
    /// 写成 alpha=255 —— 在已 clear 的画布上，`ink_bounds` 恒等于整块画布，
    /// 拿它断言「画了东西没有」是恒真的。判断这个请用本函数。
    pub fn ink_bounds_on(&self, bg: Rgb) -> (i32, i32, i32, i32) {
        let (w, h) = (self.width(), self.height());
        let data = self.data();
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, -1i32, -1i32);
        for y in 0..h {
            for x in 0..w {
                let i = ((y as u32 * w as u32 + x as u32) * 4) as usize;
                // 缓冲是 RGBA（R 在字节 0）
                if data[i] == bg.0 && data[i + 1] == bg.1 && data[i + 2] == bg.2 {
                    continue;
                }
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
        if x1 < x0 {
            return (0, 0, 0, 0);
        }
        (x0, y0, x1 - x0 + 1, y1 - y0 + 1)
    }

    /// 导出为直通 RGBA8（给 PNG 编码器用）。
    ///
    /// `tiny-skia` 是 premultiplied，而画布全程 alpha=255（见 [`Canvas::new`] 的说明），
    /// 两者此时等价，所以只需交换 R/B 之外什么都不做 —— 实际连交换都不需要，
    /// 因为 `tiny-skia` 的内存序就是 RGBA。
    pub fn to_rgba(&self) -> Vec<u8> {
        self.data().to_vec()
    }
}

#[inline]
fn color_of(c: Rgb) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(c.0, c.1, c.2, 255)
}

#[inline]
fn paint(c: Rgb, aa: bool) -> Paint<'static> {
    Paint {
        shader: Shader::SolidColor(color_of(c)),
        anti_alias: aa,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{Rect, FACE, INK};

    /// 硬边填充不得产生灰阶：任何像素的每个通道都必须是纯色之一。
    #[test]
    fn hard_edge_fill_has_no_greyscale() {
        let mut c = Canvas::new(32, 32).unwrap();
        c.clear(FACE);
        c.fill_rect(Rect::new(4, 4, 8, 8), INK);
        for y in 0..32 {
            for x in 0..32 {
                let i = ((y * 32 + x) * 4) as usize;
                let px = (c.data()[i], c.data()[i + 1], c.data()[i + 2]);
                assert!(
                    px == (INK.0, INK.1, INK.2) || px == (FACE.0, FACE.1, FACE.2),
                    "({x},{y}) 出现灰阶 {px:?}"
                );
            }
        }
    }

    /// 色键合成：未着墨处保留底图，着墨处覆盖。
    #[test]
    fn blit_only_writes_inked_pixels() {
        let mut base = Canvas::new(16, 16).unwrap();
        base.clear(FACE);
        let mut sp = Canvas::new(16, 16).unwrap(); // 未绘制处 alpha=0，即「未着墨」
        sp.fill_rect(Rect::new(4, 4, 4, 4), INK);

        base.blit((0, 0), &sp);

        let px = |c: &Canvas, x: u32, y: u32| {
            let i = ((y * 16 + x) * 4) as usize;
            (c.data()[i], c.data()[i + 1], c.data()[i + 2])
        };
        assert_eq!(
            px(&base, 0, 0),
            (FACE.0, FACE.1, FACE.2),
            "未着墨处不该被覆盖"
        );
        assert_eq!(px(&base, 5, 5), (INK.0, INK.1, INK.2), "着墨处应贴上去");
    }

    /// 偶奇规则：自相交多边形中间的洞必须被填成偶数层 → 不填充。
    #[test]
    fn polygon_uses_even_odd_rule() {
        let mut c = Canvas::new(16, 16).unwrap();
        // 一个「回」字形自交轮廓：外圈 0..16、内圈 4..12。
        let pts = [
            (0, 0),
            (16, 0),
            (16, 16),
            (0, 16),
            (0, 0),
            (4, 4),
            (4, 12),
            (12, 12),
            (12, 4),
            (4, 4),
        ];
        c.fill_polygon(&pts, INK);
        assert!(c.inked(2, 2), "外环应填充");
        assert!(!c.inked(8, 8), "内环在偶奇规则下应镂空");
    }
}
