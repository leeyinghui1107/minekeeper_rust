//! 雷、旗、问号叉、笑脸 —— 全部用图元现画。
//!
//! 形状只参照截图目视特征，不描摹原版位图。逻辑坐标一律以 16px 基准像素书写、
//! 经 `Metrics` 换算成设备像素，因此三档下顶点都是整数。
//!
//! **笑脸面部是全局唯一使用抗锯齿的地方** —— 原版美术本身就是圆的。
//! 这也是跨平台视觉差异最敏感的图元，改动前先看渲染基线。

use crate::game::Mood;
use crate::raster::Canvas;
use crate::theme::{self, Metrics, Rect};

/// 画一颗雷。雷区用硬边。
pub fn draw_mine(dst: &mut Canvas, r: Rect, m: &Metrics) {
    let cx = r.x + r.w / 2;
    let cy = r.y + r.h / 2;
    let rad = m.div_px(9, 2); // 雷体半径 4.5 个逻辑像素
    let ext = m.px(2);
    let pen = m.px(1).max(1);

    // 先八根辐条，再压上雷体，最后高光 —— 顺序与原版观察一致。
    dst.stroke_line(cx, cy - rad - ext, cx, cy + rad + ext, pen, theme::INK);
    dst.stroke_line(cx - rad - ext, cy, cx + rad + ext, cy, pen, theme::INK);
    let d = ((rad + ext) as f32 / std::f32::consts::SQRT_2).round() as i32;
    dst.stroke_line(cx - d, cy - d, cx + d, cy + d, pen, theme::INK);
    dst.stroke_line(cx - d, cy + d, cx + d, cy - d, pen, theme::INK);

    dst.fill_ellipse(
        Rect::new(cx - rad, cy - rad, 2 * rad, 2 * rad),
        theme::INK,
        false,
    );

    let hs = m.px(2);
    dst.fill_rect(
        Rect::new(cx - rad / 2 - hs / 2, cy - rad / 2 - hs / 2, hs, hs),
        theme::HILITE,
    );
}

/// 画旗子。16px 基准逻辑包围盒：格内 `x 4..11`、`y 3..12`。
pub fn draw_flag(dst: &mut Canvas, r: Rect, m: &Metrics) {
    let ox = r.x + m.px(4);
    let oy = r.y + m.px(3);
    let pole = ox + m.px(4);

    dst.fill_polygon(
        &[(pole, oy), (ox, oy + m.px(2)), (pole, oy + m.px(4))],
        theme::FLAG_RED,
    );
    dst.fill_rect(Rect::new(pole, oy, m.px(1), m.px(8)), theme::INK);
    dst.fill_rect(
        Rect::new(ox + m.px(2), oy + m.px(8), m.px(5), m.px(1)),
        theme::INK,
    );
    dst.fill_rect(
        Rect::new(ox + m.px(1), oy + m.px(9), m.px(7), m.px(1)),
        theme::INK,
    );
}

/// 画问号。
pub fn draw_question(
    dst: &mut Canvas,
    r: Rect,
    col: theme::Rgb,
    m: &Metrics,
    font: &mut FontStack,
) {
    if let Some(sp) = font.render_glyph('?', r.w, m, 0, col) {
        dst.blit((r.x, r.y), &sp);
    }
}

/// 画插错旗时盖在雷上的红叉。
pub fn draw_wrong_flag_cross(dst: &mut Canvas, r: Rect, m: &Metrics) {
    let pen = m.px(1).max(1);
    let pad = m.px(2);
    dst.stroke_line(
        r.x + pad,
        r.y + pad,
        r.right() - pad,
        r.bottom() - pad,
        pen,
        theme::FLAG_RED,
    );
    dst.stroke_line(
        r.right() - pad,
        r.y + pad,
        r.x + pad,
        r.bottom() - pad,
        pen,
        theme::FLAG_RED,
    );
}

/// 画笑脸面部。`face_box` 是按钮内部的 24×24（16px 基准逻辑）区域。
pub fn draw_face(dst: &mut Canvas, face_box: Rect, mood: Mood, m: &Metrics) {
    let ink = theme::INK;
    let pen = m.px(1).max(1);

    let fx = face_box.x + m.px(4);
    let fy = face_box.y + m.px(4);
    let d = m.px(16);
    let disc = Rect::new(fx, fy, d, d);

    // 这一组用抗锯齿（原版美术本身就是圆的）。
    dst.fill_ellipse(disc, theme::SMILE_YELLOW, true);
    dst.stroke_ellipse(disc, pen, ink);

    let cx = disc.x + disc.w / 2;
    let ey = disc.y + (disc.h as f32 * 0.32) as i32;
    let eye_dx = (disc.w as f32 * 0.22) as i32;
    let es = m.px(2).max(2);
    let eye_l = Rect::new(cx - eye_dx - es / 2, ey, es, es);
    let eye_r = Rect::new(cx + eye_dx - es / 2, ey, es, es);

    match mood {
        Mood::Normal | Mood::Pressed => {
            dst.fill_rect(eye_l, ink);
            dst.fill_rect(eye_r, ink);
            draw_smile(dst, disc, pen, false);
        }
        Mood::Concentrate => {
            dst.fill_rect(eye_l, ink);
            dst.fill_rect(eye_r, ink);
            let mx = cx - m.px(2);
            let my = disc.y + (disc.h as f32 * 0.60) as i32;
            dst.fill_ellipse(Rect::new(mx, my, m.px(4), m.px(4)), ink, true);
        }
        Mood::Dead => {
            draw_x_eye(dst, cx - eye_dx, ey, es);
            draw_x_eye(dst, cx + eye_dx, ey, es);
            draw_smile(dst, disc, pen, true);
        }
        Mood::Cool => {
            draw_shades(dst, disc, m);
            draw_smile(dst, disc, pen, false);
        }
    }
}

fn draw_x_eye(dst: &mut Canvas, cx: i32, cy: i32, size: i32) {
    let h = size;
    let pen = (h / 2).max(1);
    dst.stroke_line(cx - h, cy - h / 2, cx + h, cy + h, pen, theme::INK);
    dst.stroke_line(cx + h, cy - h / 2, cx - h, cy + h, pen, theme::INK);
}

fn draw_smile(dst: &mut Canvas, disc: Rect, pen: i32, up: bool) {
    let (frac, start, sweep) = if up {
        (0.52, 200.0, 140.0)
    } else {
        (0.34, 20.0, 140.0)
    };
    let x = (disc.x + (disc.w as f32 * 0.22) as i32) as f32;
    let y = (disc.y + (disc.h as f32 * frac) as i32) as f32;
    let w = ((disc.w as f32 * 0.56) as i32) as f32;
    let h = ((disc.h as f32 * 0.42) as i32) as f32;
    dst.stroke_arc(x, y, w, h, start, sweep, pen, theme::INK);
}

fn draw_shades(dst: &mut Canvas, disc: Rect, m: &Metrics) {
    let ink = theme::INK;
    let lens_h = (3 * m.px(1)).max((disc.h as f32 * 0.22) as i32);
    let lens_w = (disc.w as f32 * 0.32) as i32;
    let y = disc.y + (disc.h as f32 * 0.28) as i32;
    let left_x = disc.x + (disc.w as f32 * 0.10) as i32;
    let right_x = disc.right() - (disc.w as f32 * 0.10) as i32 - lens_w;
    let gap = m.px(1);

    dst.fill_rect(Rect::new(left_x, y, lens_w, lens_h), ink);
    dst.fill_rect(Rect::new(right_x, y, lens_w, lens_h), ink);

    let bridge_h = gap.max(lens_h / 3);
    dst.fill_rect(
        Rect::new(
            left_x + lens_w,
            y + lens_h / 5,
            right_x - left_x - lens_w,
            bridge_h,
        ),
        ink,
    );

    let arm_h = gap.max(lens_h / 4);
    dst.fill_rect(
        Rect::new(disc.x + gap, y + lens_h / 4, left_x - disc.x - gap, arm_h),
        ink,
    );
    dst.fill_rect(
        Rect::new(
            right_x + lens_w,
            y + lens_h / 4,
            disc.right() - gap - right_x - lens_w,
            arm_h,
        ),
        ink,
    );
}

use super::glyph::FontStack;
