//! 绘制图元层。全部是整数坐标的矩形 / 多边形 / 椭圆 / 折线，
//! 坐标一律经 [`crate::theme::Metrics`] 取整后落到设备像素。
//!
//! 这一层的代码**与平台无关**：它只调 [`crate::raster::Canvas`] 的方法，
//! 因此换光栅器不用改这里一行。

pub mod bevels;
pub mod glyph;
pub mod icon;
pub mod led;
pub mod sprites;

pub use bevels::*;
pub use glyph::*;
pub use icon::*;
pub use led::*;
pub use sprites::*;
