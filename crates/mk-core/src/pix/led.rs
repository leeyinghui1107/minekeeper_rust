//! 七段数码管。
//!
//! 字形盒 13×23（16px 基准逻辑像素），段厚 3，两端 45° 斜切 —— 比例按截图实测。
//! 段位序（低位起）：`0 top`、`1 upper-left`、`2 upper-right`、`3 middle`、
//! `4 lower-left`、`5 lower-right`、`6 bottom`。

use crate::raster::Canvas;
use crate::theme::{self, Metrics, Rect};

const LED_PATTERNS: [u8; 10] = [
    0b1110111, // 0
    0b0100100, // 1
    0b1011101, // 2
    0b1101101, // 3
    0b0101110, // 4
    0b1101011, // 5
    0b1111011, // 6
    0b0100101, // 7
    0b1111111, // 8
    0b1101111, // 9
];

const MINUS_PATTERN: u8 = 0b0001000;
const BLANK_PATTERN: u8 = 0b0000000;

/// 格式化成三个字符：`'0'..'9'`、`'-'`、`' '`。原版始终显示三位并带前导零。
pub fn led_format(value: i32) -> [char; 3] {
    if value < 0 {
        let v = (-value).min(99);
        return [
            '-',
            char::from_digit((v / 10) as u32, 10).unwrap_or('0'),
            char::from_digit((v % 10) as u32, 10).unwrap_or('0'),
        ];
    }
    let n = value.min(999);
    [
        char::from_digit((n / 100) as u32, 10).unwrap_or('0'),
        char::from_digit(((n / 10) % 10) as u32, 10).unwrap_or('0'),
        char::from_digit((n % 10) as u32, 10).unwrap_or('0'),
    ]
}

/// 画数码管：外框（含凹槽边）+ 黑底 + 三位数字。
///
/// 外框尺寸取自 `m.led_w()/led_h()`（**按部件求值**），不是 `m.px(LED_W)`，
/// 否则中档（24px）下三个数字塞不进黑底。
pub fn draw_led(dst: &mut Canvas, outer: Rect, value: i32, m: &Metrics) {
    let f = m.frame_px();
    super::bevels::draw_sunken_rect(dst, outer, f);
    dst.fill_rect(outer.inset(f), theme::LED_BACK);

    let digits = led_format(value);
    let dw = m.px(theme::LED_DIGIT_W);
    for (i, ch) in digits.iter().enumerate() {
        let box_ = Rect::new(
            outer.x + f + i as i32 * dw,
            outer.y + f,
            dw,
            m.px(theme::LED_DIGIT_H),
        );
        draw_led_digit(dst, box_, *ch, m);
    }
}

fn draw_led_digit(dst: &mut Canvas, box_: Rect, c: char, m: &Metrics) {
    let on = match c {
        '0'..='9' => LED_PATTERNS[(c as u8 - b'0') as usize],
        '-' => MINUS_PATTERN,
        _ => BLANK_PATTERN,
    };
    for seg in 0..7 {
        let col = if on & (1 << seg) != 0 {
            theme::LED_ON
        } else {
            theme::LED_OFF
        };
        dst.fill_polygon(&segment_points(box_, seg, m), col);
    }
}

/// 段的多边形顶点：先按 16px 基准逻辑坐标定义，再经 `Metrics` 落到设备像素。
///
/// **不要**用 `box.w / LED_DIGIT_W` 反推档位 —— 那是「档位比例必为整数」的假设，
/// 中档下 `20/13 = 1`，整只数码管会缩回小档大小。
fn segment_points(box_: Rect, seg: usize, m: &Metrics) -> Vec<(i32, i32)> {
    let logical: &[i32] = match seg {
        0 => &[3, 1, 9, 1, 10, 2, 9, 3, 3, 3, 2, 2],       // top
        1 => &[2, 2, 3, 3, 3, 9, 2, 10, 1, 9, 1, 3],       // upper-left
        2 => &[10, 2, 11, 3, 11, 9, 10, 10, 9, 9, 9, 3],   // upper-right
        3 => &[3, 10, 9, 10, 10, 11, 9, 12, 3, 12, 2, 11], // middle
        4 => &[2, 12, 3, 13, 3, 19, 2, 20, 1, 19, 1, 13],  // lower-left
        5 => &[10, 12, 11, 13, 11, 19, 10, 20, 9, 19, 9, 13], // lower-right
        _ => &[3, 19, 9, 19, 10, 20, 9, 21, 3, 21, 2, 20], // bottom
    };
    logical
        .chunks_exact(2)
        .map(|p| (box_.x + m.px(p[0]), box_.y + m.px(p[1])))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_is_always_three_digits() {
        assert_eq!(led_format(0), ['0', '0', '0']);
        assert_eq!(led_format(7), ['0', '0', '7']);
        assert_eq!(led_format(42), ['0', '4', '2']);
        assert_eq!(led_format(999), ['9', '9', '9']);
        // 上限 999，溢出部分被夹住而不是回绕。
        assert_eq!(led_format(1234), ['9', '9', '9']);
        // 负数：剩余雷数可以为负，原版显示 `-NN`（最多两位）。
        assert_eq!(led_format(-1), ['-', '0', '1']);
        assert_eq!(led_format(-120), ['-', '9', '9']);
    }

    /// 三档下每段的顶点都必须落在数码字盒内，否则会溢出黑底。
    #[test]
    fn segments_stay_inside_the_digit_box() {
        for size in crate::theme::Size::ALL {
            let m = Metrics::new(size);
            let box_ = Rect::new(0, 0, m.px(theme::LED_DIGIT_W), m.px(theme::LED_DIGIT_H));
            for seg in 0..7 {
                for (x, y) in segment_points(box_, seg, &m) {
                    assert!(
                        x >= 0 && y >= 0 && x <= box_.w && y <= box_.h,
                        "{size:?} 段 {seg} 顶点越界 ({x},{y})"
                    );
                }
            }
        }
    }
}
