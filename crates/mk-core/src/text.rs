//! 界面文本渲染（菜单栏、对话框、提示）。
//!
//! # 与 `pix::glyph` 的区别
//!
//! `pix::glyph` 画的是**格内数字**：硬边二值墨迹、逐字定宽、闭环校正，
//! 为的是复刻原版的像素字形。那是游戏画面的一部分。
//!
//! 这里是**界面文字**：需要中文、需要抗锯齿（小字号下二值化会把中文笔画糊成一团），
//! 也需要按字体的自然宽度排版。XP 的菜单栏本来就用抗锯齿渲染，所以这不是妥协。
//!
//! # 为什么用 `ab_glyph` 而不是 `fontdue`
//!
//! `ab_glyph` 建在 `ttf-parser` 上，是**惰性**的：`FontVec::try_from_vec_and_index`
//! 近乎瞬时返回，只有真正画到的字形才会被解析和栅格化。
//!
//! 这一点对 CJK 是硬要求。实测（macOS，见 `loading_is_fast` 测试守住这条）：
//!
//! | 字体 | 体积 | fontdue 急切解析 |
//! |---|---|---|
//! | Hiragino Sans GB.ttc | 22.4 MB | **5 s** |
//! | STHeiti Light.ttc | 53.2 MB | **3 s** |
//! | Songti.ttc | 63.8 MB | **9 s** |
//!
//! fontdue 会急切解析全部字形，用在菜单栏上等于启动卡死十几秒。

use std::collections::HashMap;

use ab_glyph::{Font, FontVec, Glyph, PxScale, ScaleFont};

use crate::raster::Canvas;
use crate::theme::Rgb;

/// 文本字体回退表。**必须覆盖 CJK**，否则菜单上的中文全是空白。
pub const TEXT_FAMILIES: &[&str] = &[
    // macOS
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/System/Library/Fonts/STHeiti Light.ttc",
    "/System/Library/Fonts/Supplemental/Songti.ttc",
    // Windows
    "C:/Windows/Fonts/msyh.ttc", // 微软雅黑
    "C:/Windows/Fonts/simhei.ttf",
    "C:/Windows/Fonts/simsun.ttc",
    // Linux
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    "/usr/share/fonts/truetype/arphic/uming.ttc",
    // 兜底：只有拉丁字形，中文会缺字，但界面至少不崩
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "C:/Windows/Fonts/arial.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
];

/// 一行文本的度量。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextMetrics {
    pub width: i32,
    /// 基线到墨迹顶部的距离（正数）。
    pub ascent: i32,
    /// 墨迹底部到基线的距离（正数）。
    pub descent: i32,
}

impl TextMetrics {
    pub fn height(&self) -> i32 {
        self.ascent + self.descent
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphKey {
    ch: char,
    px: u32, // f32 的位模式
}

/// 一个已栅格化的字形。`xmin` / `ymin` 是**相对 `(pen_x, baseline)`** 的位图左上角偏移。
struct CachedGlyph {
    coverage: Vec<u8>,
    w: usize,
    h: usize,
    xmin: i32,
    ymin: i32,
    advance: f32,
}

/// 用来判断某个字体面是否覆盖中文的探针字符。
const CJK_PROBE: char = '游';

/// 界面文本渲染器。持有若干字体面与一张字形缓存。
pub struct TextRenderer {
    faces: Vec<FontVec>,
    cache: HashMap<GlyphKey, CachedGlyph>,
    /// 已加载字体文件的字节数。**必须守住** —— CJK 字体动辄几十 MB，
    /// 多留一份就是几百 MB 的常驻内存。
    bytes: usize,
}

impl TextRenderer {
    /// 按 [`TEXT_FAMILIES`] 顺序加载；加载失败的跳过。
    ///
    /// 两条内存纪律，缺一条就是几百 MB 的常驻内存：
    ///
    /// 1. **每个文件只取集合里的第 0 个面。** 逐个 index 解析要把整个 `.ttc`
    ///    复制多份 —— `Songti.ttc` 有 8 个面，64 MB × 8 = 512 MB。
    /// 2. **拿到能画中文的字体就停。** 后面的回退项是给别的平台准备的，
    ///    在这里没必要再读进来。
    pub fn load(paths: &[&str]) -> TextRenderer {
        let mut faces: Vec<FontVec> = Vec::new();
        let mut bytes = 0usize;
        for p in paths {
            let Ok(data) = std::fs::read(p) else { continue };
            let n = data.len();
            let Ok(f) = FontVec::try_from_vec_and_index(data, 0) else {
                continue;
            };
            let covers_cjk = f.glyph_id(CJK_PROBE).0 != 0;
            faces.push(f);
            bytes += n;
            if covers_cjk {
                break;
            }
        }
        TextRenderer {
            faces,
            cache: HashMap::new(),
            bytes,
        }
    }

    /// 已加载字体文件的字节数（自检用）。
    pub fn loaded_bytes(&self) -> usize {
        self.bytes
    }

    pub fn load_default() -> TextRenderer {
        TextRenderer::load(TEXT_FAMILIES)
    }

    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    /// 找到第一个含该字符的字体面。
    fn face_for(&self, ch: char) -> Option<usize> {
        self.faces.iter().position(|f| f.glyph_id(ch).0 != 0)
    }

    fn glyph(&mut self, ch: char, px: f32) -> Option<&CachedGlyph> {
        let key = GlyphKey {
            ch,
            px: px.to_bits(),
        };
        if !self.cache.contains_key(&key) {
            let fi = self.face_for(ch)?;
            let font = &self.faces[fi];
            let gid = font.glyph_id(ch);
            let advance = font.as_scaled(PxScale::from(px)).h_advance(gid);

            // 在基线原点处取轮廓，`px_bounds` 就是相对 (0, baseline) 的偏移。
            let g: Glyph = gid.with_scale_and_position(px, ab_glyph::point(0.0, 0.0));
            let mut cov = Vec::new();
            let (mut xmin, mut ymin, mut w, mut h) = (0i32, 0i32, 0usize, 0usize);
            if let Some(outlined) = font.outline_glyph(g) {
                let b = outlined.px_bounds();
                xmin = b.min.x.floor() as i32;
                ymin = b.min.y.floor() as i32;
                w = (b.max.x.ceil() - b.min.x.floor()).max(0.0) as usize;
                h = (b.max.y.ceil() - b.min.y.floor()).max(0.0) as usize;
                cov = vec![0u8; w * h];
                outlined.draw(|gx, gy, c| {
                    let (gx, gy) = (gx as usize, gy as usize);
                    if gx < w && gy < h {
                        cov[gy * w + gx] = (c * 255.0).round().clamp(0.0, 255.0) as u8;
                    }
                });
            }
            self.cache.insert(
                key,
                CachedGlyph {
                    coverage: cov,
                    w,
                    h,
                    xmin,
                    ymin,
                    advance,
                },
            );
        }
        self.cache.get(&key)
    }

    /// 测量一行文本的宽度与上下缘（相对基线）。
    ///
    /// **必须与 [`TextRenderer::draw`] 对 `&` 的处理一致** —— 助记符不占宽度，
    /// 否则居中会偏。这是两条路径唯一需要同步的地方。
    pub fn measure(&mut self, text: &str, px: f32) -> TextMetrics {
        let mut pen = 0.0f32;
        let (mut ascent, mut descent) = (0i32, 0i32);
        for ch in text.chars() {
            if ch == '&' {
                continue;
            }
            let Some(g) = self.glyph(ch, px) else {
                pen += px * 0.5; // 缺字也要推进，否则整行会挤在一起
                continue;
            };
            ascent = ascent.max(-g.ymin);
            descent = descent.max(g.ymin + g.h as i32);
            pen += g.advance;
        }
        TextMetrics {
            width: pen.round() as i32,
            ascent: ascent.max(1),
            descent: descent.max(1),
        }
    }

    /// 在 `(x, baseline)` 处画一行文本，返回结束时的 pen x。
    ///
    /// 用覆盖率做 alpha 混合（抗锯齿），而不是硬边二值化 —— 小字号下二值化会把
    /// 中文笔画糊成一团。`&` 是助记符标记，不画也不占宽度。
    pub fn draw(
        &mut self,
        dst: &mut Canvas,
        x: i32,
        baseline: i32,
        text: &str,
        px: f32,
        col: Rgb,
    ) -> i32 {
        let mut pen = 0.0f32;
        for ch in text.chars() {
            if ch == '&' {
                continue;
            }
            let Some(g) = self.glyph(ch, px) else {
                pen += px * 0.5;
                continue;
            };
            let ox = x + (pen + g.xmin as f32).round() as i32;
            let oy = baseline + g.ymin;
            for gy in 0..g.h {
                for gx in 0..g.w {
                    let a = g.coverage[gy * g.w + gx];
                    if a == 0 {
                        continue;
                    }
                    dst.blend_pixel(ox + gx as i32, oy + gy as i32, col, a);
                }
            }
            pen += g.advance;
        }
        x + pen.round() as i32
    }

    /// 在矩形内左侧留 `left_pad` 后**垂直居中**画一行。返回实际文本宽度。
    pub fn draw_in(
        &mut self,
        dst: &mut Canvas,
        r: crate::theme::Rect,
        text: &str,
        px: f32,
        col: Rgb,
        left_pad: i32,
    ) -> i32 {
        let m = self.measure(text, px);
        let baseline = r.y + (r.h - m.height()) / 2 + m.ascent;
        self.draw(dst, r.x + left_pad, baseline, text, px, col);
        m.width
    }

    /// 在矩形内**水平 + 垂直居中**画一行。
    pub fn draw_centered(
        &mut self,
        dst: &mut Canvas,
        r: crate::theme::Rect,
        text: &str,
        px: f32,
        col: Rgb,
    ) -> i32 {
        let m = self.measure(text, px);
        let baseline = r.y + (r.h - m.height()) / 2 + m.ascent;
        self.draw(dst, r.x + (r.w - m.width) / 2, baseline, text, px, col);
        m.width
    }

    /// 报告哪些字符在所有字体面里都找不到（自检用）。
    pub fn missing_chars(&self, text: &str) -> Vec<char> {
        text.chars()
            .filter(|&c| c != '&' && self.face_for(c).is_none())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{Rect, FACE, INK};

    #[test]
    fn loads_some_font() {
        let t = TextRenderer::load_default();
        assert!(!t.is_empty(), "一个字体都没加载到");
    }

    /// 字体常驻内存必须有界。
    ///
    /// 曾经这里是 1.1 GB —— `load` 对 `.ttc` 的每个 index 都 `data.clone()`，
    /// 而 Songti.ttc 有 8 个面、每个 64 MB。这条测试守住它不再回来。
    #[test]
    fn font_memory_is_bounded() {
        let t = TextRenderer::load_default();
        if t.is_empty() {
            return;
        }
        let mb = t.loaded_bytes() as f64 / 1048576.0;
        assert!(
            mb < 80.0,
            "字体常驻 {mb:.1} MB，太多。检查是不是对集合字体的每个 index 都复制了整份文件"
        );
        // 一个 CJK 字体面就够用，不该把整张回退表都读进来。
        assert!(
            t.faces.len() <= 3,
            "加载了 {} 个字体面，应当拿到中文就停",
            t.faces.len()
        );
    }

    /// 加载必须是毫秒级 —— 否则菜单栏会把启动卡住十几秒。
    #[test]
    fn loading_is_fast() {
        let t0 = std::time::Instant::now();
        let t = TextRenderer::load_default();
        let dt = t0.elapsed();
        if t.is_empty() {
            return;
        }
        assert!(
            dt.as_millis() < 1500,
            "字体加载耗时 {dt:?}，太慢（fontdue 那种急切解析会到 3–9 秒）"
        );
    }

    /// 界面用到的全部中文字符都必须能画出来 —— 缺字会让菜单变成空白。
    #[test]
    fn ui_strings_have_no_missing_glyphs() {
        let t = TextRenderer::load_default();
        if t.is_empty() {
            return;
        }
        let samples = [
            "游戏",
            "帮助",
            "开局",
            "初级",
            "中级",
            "高级",
            "自定义",
            "标记",
            "颜色",
            "声音",
            "扫雷英雄榜",
            "退出",
            "目录",
            "查找帮助主题",
            "使用帮助",
            "关于扫雷",
            "确定",
            "取消",
            "重新计分",
            "高度",
            "宽度",
            "雷数",
            "已破高级记录",
            "请留尊姓大名",
            "匿名",
            "秒",
        ];
        let mut missing = Vec::new();
        for s in samples {
            let m = t.missing_chars(s);
            if !m.is_empty() {
                missing.push((s, m));
            }
        }
        assert!(
            missing.is_empty(),
            "以下界面文案缺字（菜单会变空白）：{missing:?}"
        );
    }

    #[test]
    fn measure_grows_with_text_and_size() {
        let mut t = TextRenderer::load_default();
        if t.is_empty() {
            return;
        }
        let a = t.measure("扫雷", 13.0).width;
        let b = t.measure("扫雷扫雷", 13.0).width;
        let c = t.measure("扫雷", 26.0).width;
        assert!(b > a, "更长的文本应更宽");
        assert!(c > a, "更大的字号应更宽");
    }

    /// 助记符 `&` 不占宽度 —— `measure` 与 `draw` 必须一致，否则居中会偏。
    #[test]
    fn ampersand_takes_no_width() {
        let mut t = TextRenderer::load_default();
        if t.is_empty() {
            return;
        }
        assert_eq!(
            t.measure("游戏(&G)", 13.0).width,
            t.measure("游戏(G)", 13.0).width,
            "& 不该占宽度"
        );
    }

    #[test]
    fn draw_puts_ink_on_the_canvas() {
        let mut t = TextRenderer::load_default();
        if t.is_empty() {
            return;
        }
        let mut cv = Canvas::new(120, 24).unwrap();
        cv.clear(FACE);
        let w = t.draw_in(&mut cv, Rect::new(0, 0, 120, 24), "游戏", 13.0, INK, 8);
        assert!(w > 0);
        let (_, _, iw, ih) = cv.ink_bounds();
        assert!(iw > 8 && ih > 4, "文本几乎没画上去：{iw}x{ih}");
    }

    /// 居中绘制时，墨迹左右留白应当大致相等。
    #[test]
    fn centered_text_is_balanced() {
        let mut t = TextRenderer::load_default();
        if t.is_empty() {
            return;
        }
        let mut cv = Canvas::new(200, 24).unwrap();
        cv.clear(FACE);
        t.draw_centered(&mut cv, Rect::new(0, 0, 200, 24), "扫雷", 13.0, INK);
        let (x0, _, w, _) = cv.ink_bounds();
        let left = x0;
        let right = 200 - (x0 + w);
        assert!(
            (left - right).abs() <= 2,
            "居中不平衡：左 {left} 右 {right}"
        );
    }
}
