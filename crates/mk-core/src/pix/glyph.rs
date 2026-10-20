//! 格内数字 1..8 与问号的字形适配。
//!
//! # 为什么不用系统文本光栅
//!
//! Win32 的 GDI 硬边光栅（`CreateFontW` + `ExtTextOutW`）取到的墨迹带 hinting，
//! 那是 Windows 专有的结果，**在其他平台无法复现**。
//! 这里改用 `fontdue` 从系统字体轮廓栅格化出覆盖度位图，再按阈值二值化 ——
//! 三平台同一份代码，结果逐像素一致。
//!
//! **闭环校正逻辑与源墨迹无关**（它只要求拿到「某个字符的二值掩码」），
//! 所以换掉掩码来源之后，适配与校正那一整套原样保留，无需重调参。

use std::collections::HashMap;

use crate::raster::Canvas;
use crate::theme::{self, Metrics, GLYPH_BOX, GLYPH_OFFSET};

/// 取轮廓时的字符高度：放大若干倍取墨迹，再回采到目标框，
/// 这样细笔画不会在量化时消失。
const PROBE_EM: f32 = 96.0;

/// **内嵌**的格内数字字体。
///
/// 为什么不走系统字体：三平台会加载到三个不同的粗体（macOS Arial Bold /
/// Windows arialbd / Linux DejaVuSans-Bold），字形不同 → 像素不同 →
/// **一套渲染基线无法在三平台同时成立**。
/// 实测：把回退表里的 Arial Bold 换成 Verdana Bold，15 张基线里 9 张失败。
///
/// 选 Liberation Sans Bold 是因为 SIL OFL 1.1 允许原样再分发，且与 Arial
/// **度量兼容**，字形观感与原先最接近。详见 `assets/fonts/README.md`。
const DIGIT_FONT: &[u8] = include_bytes!("../../../../assets/fonts/LiberationSans-Bold.ttf");
const DIGIT_FONT_NAME: &str = "内嵌 Liberation Sans Bold";

/// 系统字体的回退顺序。**只用于排查**（`--glyphdiag` 对比、测试）。
///
/// 生产路径走 [`FontStack::load_default`]，用的是内嵌字体 —— 那是三平台一致的唯一办法。
pub const DEFAULT_FAMILIES: &[&str] = &[
    // Windows
    "C:/Windows/Fonts/arialbd.ttf",
    "C:/Windows/Fonts/segoeuib.ttf",
    // macOS
    "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
    "/System/Library/Fonts/Supplemental/Verdana Bold.ttf",
    "/System/Library/Fonts/Helvetica.ttc",
    // Linux
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
];

/// 一个字符的二值墨迹（已裁到包围盒）。
pub struct GlyphMask {
    pub mask: Vec<bool>,
    pub w: usize,
    pub h: usize,
    pub family: String,
}

/// 系统字体栈。构造时一次性读入并缓存，之后按字符取墨迹。
pub struct FontStack {
    faces: Vec<(String, fontdue::Font)>,
    masks: std::collections::HashMap<char, Option<GlyphMask>>,
    sprites: std::collections::HashMap<SpriteKey, Canvas>,
}

#[derive(PartialEq, Eq, Hash)]
struct SpriteKey {
    ch: char,
    cell_px: i32,
    target_w: i32,
    rgba: u32,
}

impl FontStack {
    /// 按给定路径顺序加载；加载失败的跳过。全部失败时返回空栈，
    /// 此时 [`FontStack::render_glyph`] 一律返回 `None`，画面缺数字但不崩溃。
    pub fn load(paths: &[&str]) -> FontStack {
        let mut faces = Vec::new();
        for p in paths {
            let Ok(data) = std::fs::read(p) else { continue };
            if let Ok(f) = fontdue::Font::from_bytes(
                data,
                fontdue::FontSettings {
                    collection_index: 0,
                    ..Default::default()
                },
            ) {
                // 格内数字只用 ASCII，第一个能画「1」的字体就够了。
                // 继续往下读会把整张回退表都留在内存里。
                let usable = f.lookup_glyph_index('1') != 0;
                faces.push(((*p).to_string(), f));
                if usable {
                    break;
                }
            }
        }
        FontStack {
            faces,
            masks: Default::default(),
            sprites: Default::default(),
        }
    }

    /// 用**内嵌**字体建栈。这是生产路径 —— 三平台字形因此完全一致。
    ///
    /// 内嵌字体解析失败时返回空栈，此时 [`FontStack::render_glyph`] 一律返回 `None`，
    /// 画面缺数字但不崩溃。
    pub fn load_default() -> FontStack {
        let mut faces = Vec::new();
        if let Ok(f) = fontdue::Font::from_bytes(
            DIGIT_FONT,
            fontdue::FontSettings {
                collection_index: 0,
                ..Default::default()
            },
        ) {
            faces.push((DIGIT_FONT_NAME.to_string(), f));
        }
        FontStack {
            faces,
            masks: HashMap::new(),
            sprites: HashMap::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    /// 取一个字符的硬边二值墨迹（已裁到包围盒），按字族回退。
    ///
    /// 阈值取 128/255：面积覆盖 >= 50% 判定着墨，等价于把轮廓缩放后做硬边填充，
    /// 不产生任何灰阶。
    pub fn ink(&mut self, ch: char) -> Option<&GlyphMask> {
        if !self.masks.contains_key(&ch) {
            let mut found = None;
            for (name, f) in &self.faces {
                let (metrics, bitmap) = f.rasterize(ch, PROBE_EM);
                if metrics.width == 0 || metrics.height == 0 {
                    continue;
                }
                let mask: Vec<bool> = bitmap.iter().map(|&v| v >= 128).collect();
                if !mask.iter().any(|&b| b) {
                    continue;
                }
                found = Some(GlyphMask {
                    mask,
                    w: metrics.width,
                    h: metrics.height,
                    family: name.clone(),
                });
                break;
            }
            self.masks.insert(ch, found);
        }
        self.masks.get(&ch).and_then(|o| o.as_ref())
    }

    /// 取（并缓存）适配到目标框后的字形精灵。
    ///
    /// `target_w > 0` 时强制水平目标宽度（数字 2–8 撑满、`1` 只有 7 宽）；
    /// 为 0 时保持字体的自然宽高比（问号）。
    pub fn render_glyph(
        &mut self,
        ch: char,
        cell_px: i32,
        m: &Metrics,
        target_w: i32,
        col: theme::Rgb,
    ) -> Option<Canvas> {
        let key = SpriteKey {
            ch,
            cell_px,
            target_w,
            rgba: ((col.0 as u32) << 16) | ((col.1 as u32) << 8) | col.2 as u32,
        };
        if let Some(c) = self.sprites.get(&key) {
            // 缓存命中：拷一份出去，调用方按需合成。
            return Some(clone_canvas(c));
        }

        let box_ = m.px(GLYPH_BOX);
        let off = m.px(GLYPH_OFFSET);
        let info = self.ink(ch)?;
        let (iw, ih) = (info.w, info.h);

        let mut sp = Canvas::new(cell_px, cell_px)?;

        let sy0 = box_ as f64 / ih as f64;
        let mut sy = sy0;
        let mut sx = sy0;
        let want_w = if target_w > 0 {
            target_w
        } else {
            (iw as f64 * sx).round() as i32
        };
        if target_w > 0 {
            sx = target_w as f64 / iw as f64;
        }
        let left = off + (box_ - want_w) / 2;
        let top = off;
        let (mut dx, mut dy) = (left as f64, top as f64);

        // 闭环校正：画完量实际墨迹，把误差补回缩放与平移，再画一次。
        // 两轮就够，且结果与 PROBE_EM 无关。
        let mask = &self.masks[&ch].as_ref().unwrap().mask;
        for _ in 0..4 {
            paint_mask(&mut sp, mask, iw, ih, sx, sy, dx, dy, col);
            let (gx, gy, gw, gh) = sp.ink_bounds();
            if gw == 0 {
                break;
            }
            if gw == want_w && gh == box_ && gx == left && gy == top {
                break;
            }
            sx *= want_w as f64 / gw as f64;
            sy *= box_ as f64 / gh as f64;
            dx += (left - gx) as f64;
            dy += (top - gy) as f64;
        }

        self.sprites.insert(key, clone_canvas(&sp));
        Some(sp)
    }

    /// 打印每个字形的源墨迹与最终精灵墨迹，用于核对目标框适配（`--glyphdiag`）。
    pub fn diag(&mut self, m: &Metrics) -> Vec<String> {
        let mut out = Vec::new();
        for ch in ['1', '2', '3', '4', '5', '6', '7', '8', '?'] {
            let Some(info) = self.ink(ch) else {
                out.push(format!("{ch} 无墨迹"));
                continue;
            };
            let (iw, ih, fam) = (info.w, info.h, info.family.clone());
            let tw = if ch == '?' {
                0
            } else {
                m.px(theme::glyph_width(ch as u8 - b'0'))
            };
            let bounds = self
                .render_glyph(ch, m.cell_px(), m, tw, theme::INK)
                .map(|c| c.ink_bounds())
                .unwrap_or((0, 0, 0, 0));
            out.push(format!(
                "{ch:<2} 字族={fam:<40} 源墨迹={iw}x{ih} 目标宽={tw} 精灵墨迹={bounds:?}"
            ));
        }
        out
    }
}

fn clone_canvas(c: &Canvas) -> Canvas {
    let mut out = Canvas::new(c.width(), c.height()).expect("尺寸有效");
    out.data_mut().copy_from_slice(c.data());
    out
}

/// 把源墨迹按 `(sx,sy)` 缩放、平移到 `(dx,dy)`，用面积覆盖 >= 50% 判定着墨 ——
/// 等价于把轮廓缩放后做硬边填充，不产生任何灰阶。
#[allow(clippy::too_many_arguments)]
fn paint_mask(
    sp: &mut Canvas,
    mask: &[bool],
    mw: usize,
    mh: usize,
    sx: f64,
    sy: f64,
    dx: f64,
    dy: f64,
    col: theme::Rgb,
) {
    let (w, h) = (sp.width(), sp.height());
    let data = sp.data_mut();
    for y in 0..h {
        let v0 = (y as f64 - dy) / sy;
        let v1 = (y as f64 + 1.0 - dy) / sy;
        for x in 0..w {
            let u0 = (x as f64 - dx) / sx;
            let u1 = (x as f64 + 1.0 - dx) / sx;
            let area = (u1 - u0) * (v1 - v0);
            if area <= 0.0 {
                continue;
            }
            let mut hit = 0.0f64;
            let (j0, j1) = (v0.floor() as i64, v1.ceil() as i64);
            let (i0, i1) = (u0.floor() as i64, u1.ceil() as i64);
            for j in j0..j1 {
                if j < 0 || j >= mh as i64 {
                    continue;
                }
                let oy = v1.min(j as f64 + 1.0) - v0.max(j as f64);
                for i in i0..i1 {
                    if i < 0 || i >= mw as i64 || !mask[j as usize * mw + i as usize] {
                        continue;
                    }
                    hit += (u1.min(i as f64 + 1.0) - u0.max(i as f64)) * oy;
                }
            }
            if hit / area >= 0.5 {
                let idx = ((y as u32 * w as u32 + x as u32) * 4) as usize;
                data[idx] = col.0;
                data[idx + 1] = col.1;
                data[idx + 2] = col.2;
                data[idx + 3] = 255;
            }
        }
    }
}
