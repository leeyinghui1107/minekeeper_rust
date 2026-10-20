//! 几何与配色常量。数值一律是 16px 基准逻辑像素 —— 即从 `winmine.exe` 截图逐像素
//! 量出来的原始尺寸；渲染时由 [`Metrics`] 按小/中/大三档换算，代码里不出现位图拉伸。
//!
//! **取整规则只有 [`Metrics::px`] 一处**，
//! 所有总尺寸一律由「换算后的基本量相加」得到，绝不对逻辑总量整体缩放 —— 否则 24px 档下
//! 会出现「边框厚度之和 ≠ 凹槽宽度」这类缝隙（见设计文档 §5.2）。

use std::fmt;

// ── 16px 基准常量 ───────────────────────────────────────────────────────────

pub const CELL: i32 = 16;
pub const BEVEL: i32 = 2;
pub const SUNKEN_LINE: i32 = 1;
pub const FIELD_BORDER: i32 = 3;
pub const STATUS_BORDER: i32 = 2;
pub const MARGIN_TL: i32 = 6;
pub const MARGIN_BR: i32 = 5;
pub const STATUS_HEIGHT: i32 = 37;
pub const STATUS_GAP: i32 = 6;

pub const LED_W: i32 = 41;
pub const LED_H: i32 = 25;
pub const LED_INSET: i32 = 13;
pub const LED_TOP: i32 = 12;
pub const LED_DIGIT_W: i32 = 13;
pub const LED_DIGIT_H: i32 = 23;
pub const LED_SEG: i32 = 3;

pub const SMILEY_BTN: i32 = 26;
pub const SMILEY_FACE: i32 = 24;

pub const GLYPH_BOX: i32 = 10;
pub const GLYPH_OFFSET: i32 = 3;

/// 逐字形的 16px 基准实测宽度：原版数字 2–8 撑满 10 宽，只有「1」是 7 宽。
///
/// 通用粗体字的数字宽高比只有约 0.65，只按字高等比画会偏窄，所以按实测逐字定宽，
/// 做水平/垂直各自独立的整数目标框适配，而不是统一缩放。
pub fn glyph_width(digit: u8) -> i32 {
    if digit == 1 {
        7
    } else {
        GLYPH_BOX
    }
}

// ── 尺寸档位 ────────────────────────────────────────────────────────────────

/// 方格边长只有三档：小 16 / 中 24 / 大 32 设备像素。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Size {
    Small,
    #[default]
    Medium,
    Large,
}

impl Size {
    pub const ALL: [Size; 3] = [Size::Small, Size::Medium, Size::Large];

    pub fn cell_px(self) -> i32 {
        match self {
            Size::Small => 16,
            Size::Medium => 24,
            Size::Large => 32,
        }
    }

    /// 解析 `small|medium|large`、`s|m|l`、`16|24|32`、`小|中|大`。
    pub fn parse(text: &str) -> Result<Size, String> {
        match text.trim().to_ascii_lowercase().as_str() {
            "small" | "s" | "16" | "小" => Ok(Size::Small),
            "medium" | "m" | "24" | "中" => Ok(Size::Medium),
            "large" | "l" | "32" | "大" => Ok(Size::Large),
            other => Err(format!("{other:?} 无效（small/medium/large 或 16/24/32）")),
        }
    }
}

impl fmt::Display for Size {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Size::Small => "小",
            Size::Medium => "中",
            Size::Large => "大",
        })
    }
}

// ── 配色 ────────────────────────────────────────────────────────────────────

/// 直通 sRGB 的 8 位颜色。光栅层用它，避免在绘制代码里引入 `tiny_skia::Color`。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rgb(pub u8, pub u8, pub u8);

pub const FACE: Rgb = Rgb(0xC0, 0xC0, 0xC0);
pub const HILITE: Rgb = Rgb(0xFF, 0xFF, 0xFF);
pub const SHADOW: Rgb = Rgb(0x80, 0x80, 0x80);
pub const INK: Rgb = Rgb(0x00, 0x00, 0x00);

pub const LED_BACK: Rgb = Rgb(0x00, 0x00, 0x00);
pub const LED_ON: Rgb = Rgb(0xFF, 0x00, 0x00);
pub const LED_OFF: Rgb = Rgb(0x80, 0x00, 0x00);

pub const FLAG_RED: Rgb = Rgb(0xFF, 0x00, 0x00);
pub const QUESTION_BLUE: Rgb = Rgb(0x00, 0x00, 0xFF);

/// 按住中键 / 左右键时源格的描边色。
pub const CHORD_SOURCE: Rgb = Rgb(0xFF, 0x7F, 0x27);
/// 被单格约束判定为「必是雷」的格子的描边色。
pub const CHORD_MINE: Rgb = Rgb(0xED, 0x1C, 0x24);

pub const SMILE_YELLOW: Rgb = Rgb(0xFF, 0xFF, 0x00);

/// 数字 1..8 的取色；索引 0 不使用。
pub const DIGITS: [Rgb; 9] = [
    FACE,
    Rgb(0x00, 0x00, 0xFF), // 1 蓝
    Rgb(0x00, 0x80, 0x00), // 2 绿
    Rgb(0xFF, 0x00, 0x00), // 3 红
    Rgb(0x80, 0x00, 0x00), // 4 栗
    Rgb(0x00, 0x00, 0x80), // 5 深蓝
    Rgb(0x00, 0x80, 0x80), // 6 青
    Rgb(0x00, 0x00, 0x00), // 7 黑
    Rgb(0x80, 0x80, 0x80), // 8 灰
];

/// 按「颜色 / 单色」开关取数字色。
pub fn digit_color(n: usize, colored: bool) -> Rgb {
    if colored && n < DIGITS.len() {
        DIGITS[n]
    } else {
        INK
    }
}

// ── 换算器 ──────────────────────────────────────────────────────────────────

/// 某一档方格尺寸下的设备像素换算器。所有绘制只经它取坐标。
///
/// 其余尺寸按 16px 基准比例推导：`Px(v) = (v*cellPx + 8) / 16`，`.5` 一律向上。
/// 16/32 是严格线性，24 的奇数逻辑值落在半像素上时向上取整。
#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    cell_px: i32,
}

impl Metrics {
    pub fn new(size: Size) -> Self {
        Metrics {
            cell_px: size.cell_px(),
        }
    }

    /// 按任意方格边长建换算器。用于图标这类「不落在三档里、但要保持同一套比例」的场景：
    /// `Px` 与 `DivPx` 都是纯比例式，所以 512px 的图标与 32px 的图标在比例上完全一致
    /// （斜边都是边长的 1/8，雷体半径都是 0.28 倍）。
    pub fn for_cell(cell_px: i32) -> Self {
        Metrics {
            cell_px: cell_px.max(1),
        }
    }

    pub fn cell_px(&self) -> i32 {
        self.cell_px
    }

    /// 把 16px 基准下的逻辑长度换算成设备像素，`.5` 一律向上。
    ///
    /// 整数除法对负数向零取整，而逻辑值恒为非负，所以这里无需额外处理。
    #[inline]
    pub fn px(&self, v: i32) -> i32 {
        (v * self.cell_px + 8) / 16
    }

    /// 换算「num/den 个逻辑像素」这种半整数长度（例如雷体半径 9/2 = 4.5）。
    #[inline]
    pub fn div_px(&self, num: i32, den: i32) -> i32 {
        (num * self.cell_px + 8 * den) / (16 * den)
    }

    // ── 基本量的设备尺寸 ────────────────────────────────────────────────────

    pub fn bevel_px(&self) -> i32 {
        self.px(BEVEL)
    }
    pub fn sunken_line_px(&self) -> i32 {
        self.px(SUNKEN_LINE).max(1)
    }
    pub fn field_border_px(&self) -> i32 {
        self.px(FIELD_BORDER)
    }
    pub fn status_border_px(&self) -> i32 {
        self.px(STATUS_BORDER)
    }
    pub fn margin_px(&self) -> i32 {
        self.px(MARGIN_TL)
    }
    pub fn margin_br_px(&self) -> i32 {
        self.px(MARGIN_BR)
    }
    pub fn status_height_px(&self) -> i32 {
        self.px(STATUS_HEIGHT)
    }
    pub fn status_gap_px(&self) -> i32 {
        self.px(STATUS_GAP)
    }

    /// 1 逻辑像素的边框厚度（数码管外框、笑脸按钮外框共用），至少 1 设备像素。
    pub fn frame_px(&self) -> i32 {
        self.px(1).max(1)
    }

    /// 数码管外框**不能**直接 `px(41)`：`41 = 1 边框 + 13×3 数字 + 1 边框`，各自取整后
    /// 加起来才对得上。中档下 3 个数字是 `3×20 = 60`，直接 `px(41) = 62` 扣掉两条边框
    /// 只剩 58，塞不进去。按部件求值在 41/64/82 三档都与实测一致。
    pub fn led_w(&self) -> i32 {
        2 * self.frame_px() + 3 * self.px(LED_DIGIT_W)
    }
    pub fn led_h(&self) -> i32 {
        2 * self.frame_px() + self.px(LED_DIGIT_H)
    }

    // ── 客户区与锚点 ────────────────────────────────────────────────────────
    //
    // 全部写成「换算后的基本量相加」。三档下各部分之和恰好等于整体，
    // 不会出现边框与总宽互相矛盾。

    pub fn client_width(&self, cols: i32) -> i32 {
        self.margin_px()
            + self.field_border_px()
            + cols * self.cell_px()
            + self.field_border_px()
            + self.margin_br_px()
    }

    pub fn client_height(&self, rows: i32) -> i32 {
        self.margin_px()
            + self.status_height_px()
            + self.status_gap_px()
            + 2 * self.field_border_px()
            + rows * self.cell_px()
            + self.margin_br_px()
    }

    pub fn status_top_px(&self) -> i32 {
        self.margin_px()
    }
    pub fn field_rect_top_px(&self) -> i32 {
        self.margin_px() + self.status_height_px() + self.status_gap_px()
    }
    pub fn field_left_px(&self) -> i32 {
        self.margin_px() + self.field_border_px()
    }
    pub fn field_top_px(&self) -> i32 {
        self.field_rect_top_px() + self.field_border_px()
    }

    // ── 矩形 ────────────────────────────────────────────────────────────────

    /// 第 `(col,row)` 格的设备矩形。
    ///
    /// 格距用**换算后的单元格边长**逐格累加，而不是「(原点 + 格号×16) × 档位比例」——
    /// 后者在 24px 下会让格宽在 23/24/25 之间抖动。
    pub fn cell_rect(&self, col: i32, row: i32) -> Rect {
        let x = self.field_left_px() + col * self.cell_px();
        let y = self.field_top_px() + row * self.cell_px();
        Rect::new(x, y, self.cell_px(), self.cell_px())
    }

    pub fn status_rect(&self, cols: i32) -> Rect {
        Rect::new(
            self.margin_px(),
            self.status_top_px(),
            self.client_width(cols) - self.margin_px() - self.margin_br_px(),
            self.status_height_px(),
        )
    }

    pub fn field_rect(&self, cols: i32, rows: i32) -> Rect {
        Rect::new(
            self.margin_px(),
            self.field_rect_top_px(),
            self.field_border_px() + cols * self.cell_px() + self.field_border_px(),
            self.field_border_px() + rows * self.cell_px() + self.field_border_px(),
        )
    }

    /// 笑脸按钮：取整发生在**逻辑空间**（`(ClientW - 26)/2` 再换算），
    /// 所以按钮可能比设备像素正中偏左 1px —— 这是 XP 原版的行为，必须照抄。
    pub fn smiley_rect(&self, cols: i32) -> Rect {
        let logical = (logical_client_width(cols) - SMILEY_BTN) / 2;
        Rect::new(
            self.px(logical),
            self.px(LED_TOP),
            self.px(SMILEY_BTN),
            self.px(SMILEY_BTN),
        )
    }

    pub fn led_left_rect(&self) -> Rect {
        Rect::new(
            self.px(LED_INSET),
            self.px(LED_TOP),
            self.led_w(),
            self.led_h(),
        )
    }

    pub fn led_right_rect(&self, cols: i32) -> Rect {
        let right = self.client_width(cols) - self.px(LED_INSET);
        Rect::new(
            right - self.led_w(),
            self.px(LED_TOP),
            self.led_w(),
            self.led_h(),
        )
    }

    // ── 反算 ────────────────────────────────────────────────────────────────

    /// 把设备坐标反算成格号；落在雷区左上角之外返回 `None`。
    ///
    /// 命中测试与脏矩形裁剪都走这里 —— 直接用换算后的锚点与格距反算，
    /// 不能拿「设备坐标 ÷ 档位比例」，24px 下会把格子边界算错。
    pub fn col_at(&self, x: i32) -> Option<i32> {
        if x < self.field_left_px() {
            return None;
        }
        Some((x - self.field_left_px()) / self.cell_px())
    }

    pub fn row_at(&self, y: i32) -> Option<i32> {
        if y < self.field_top_px() {
            return None;
        }
        Some((y - self.field_top_px()) / self.cell_px())
    }
}

/// 16px 基准下的客户区宽度（`smiley_rect` 的取整必须在逻辑空间做）。
fn logical_client_width(cols: i32) -> i32 {
    cols * CELL + MARGIN_TL + 2 * FIELD_BORDER + MARGIN_BR
}

// ── 矩形 ────────────────────────────────────────────────────────────────────

/// 整数像素矩形，左上含、右下不含 —— 与 GDI/GDI+ 口径一致。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Rect { x, y, w, h }
    }
    pub const fn right(&self) -> i32 {
        self.x + self.w
    }
    pub const fn bottom(&self) -> i32 {
        self.y + self.h
    }
    pub const fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }
    pub fn overlaps(&self, o: &Rect) -> bool {
        self.x < o.right() && o.x < self.right() && self.y < o.bottom() && o.y < self.bottom()
    }
    pub fn inset(&self, t: i32) -> Rect {
        Rect::new(self.x + t, self.y + t, self.w - 2 * t, self.h - 2 * t)
    }
    pub fn intersect(&self, o: &Rect) -> Rect {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        let r = self.right().min(o.right());
        let b = self.bottom().min(o.bottom());
        Rect::new(x, y, r - x, b - y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 三档客户区尺寸必须与 README 的实测表逐项相符。
    #[test]
    fn client_size_table() {
        let expect = [
            (Size::Small, 497, 444),
            (Size::Medium, 747, 668),
            (Size::Large, 994, 888),
        ];
        for (size, w, h) in expect {
            let m = Metrics::new(size);
            assert_eq!(m.client_width(30), w, "{size:?} 宽");
            assert_eq!(m.client_height(24), h, "{size:?} 高");
        }
    }

    /// 数码管外框必须是「按部件求值」，三档得 41 / 64 / 82。
    #[test]
    fn led_frame_by_parts() {
        let got: Vec<i32> = Size::ALL.iter().map(|s| Metrics::new(*s).led_w()).collect();
        assert_eq!(got, vec![41, 64, 82]);
    }

    /// 中档（24px）下每个逻辑单位都可能落在半像素上；这里守住 `.5` 一律向上。
    #[test]
    fn medium_rounding_is_half_up() {
        let m = Metrics::new(Size::Medium);
        assert_eq!(m.px(1), 2); // 1.5 → 2
        assert_eq!(m.px(3), 5); // 4.5 → 5
        assert_eq!(m.px(5), 8); // 7.5 → 8
        assert_eq!(m.div_px(9, 2), 7); // 6.75 → 7
    }

    /// 状态栏与雷区不得重叠，且雷区下沿不超出客户区。
    #[test]
    fn status_and_field_do_not_overlap() {
        for size in Size::ALL {
            for (rows, cols) in [(16, 16), (16, 30), (24, 30), (99, 99)] {
                let m = Metrics::new(size);
                let status = m.status_rect(cols);
                let field = m.field_rect(cols, rows);
                assert!(
                    status.bottom() <= field.y,
                    "{size:?} {rows}x{cols}：状态栏与雷区重叠"
                );
                assert!(
                    field.bottom() <= m.client_height(rows),
                    "{size:?} {rows}x{cols}：雷区超出客户区"
                );
                assert!(field.right() <= m.client_width(cols));
            }
        }
    }

    /// 反算与正算必须互逆 —— 这是命中测试的正确性前提。
    #[test]
    fn hit_test_round_trips() {
        for size in Size::ALL {
            let m = Metrics::new(size);
            for row in 0..24 {
                for col in 0..30 {
                    let r = m.cell_rect(col, row);
                    assert_eq!(m.col_at(r.x), Some(col));
                    assert_eq!(m.row_at(r.y), Some(row));
                    assert_eq!(m.col_at(r.right() - 1), Some(col));
                    assert_eq!(m.row_at(r.bottom() - 1), Some(row));
                }
            }
            assert_eq!(m.col_at(m.field_left_px() - 1), None);
            assert_eq!(m.row_at(m.field_top_px() - 1), None);
        }
    }
}
