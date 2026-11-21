//! 颜色类型（PRD §5.1.3）。
//!
//! # 设计
//! - RGBA8888 为唯一存储格式（后端一致性的基石：所有后端以相同字节序消费）；
//! - `Color` 为纯值类型（Copy），无任何平台依赖；
//! - Android `Color` 类常量以关联常量对齐，便于从 Android 代码直译。

/// RGBA 颜色（每通道 u8，非预乘 alpha）。
///
/// # 不变量
/// 存储顺序固定为 RGBA；与 `ANativeWindow` 缓冲（RGBA_8888）字节序一致，
/// 后端搬运时无需通道交换。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// 构造不透明颜色。
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 0xFF }
    }

    /// 构造带 alpha 的颜色。
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// 从 0xAARRGGBB 打包值解包（Android `Color` 类的 int 形态）。
    pub const fn from_argb(argb: u32) -> Self {
        Self {
            a: ((argb >> 24) & 0xFF) as u8,
            r: ((argb >> 16) & 0xFF) as u8,
            g: ((argb >> 8) & 0xFF) as u8,
            b: (argb & 0xFF) as u8,
        }
    }

    /// 打包为 0xAARRGGBB（与 Android 对齐，供日志/调试）。
    pub const fn to_argb(self) -> u32 {
        ((self.a as u32) << 24) | ((self.r as u32) << 16) | ((self.g as u32) << 8) | self.b as u32
    }

    // ---- Android 常用颜色常量（android.graphics.Color 对齐）----

    pub const BLACK: Color = Color::rgb(0x00, 0x00, 0x00);
    pub const WHITE: Color = Color::rgb(0xFF, 0xFF, 0xFF);
    pub const RED: Color = Color::rgb(0xFF, 0x00, 0x00);
    pub const GREEN: Color = Color::rgb(0x00, 0xFF, 0x00);
    pub const BLUE: Color = Color::rgb(0x00, 0x00, 0xFF);
    pub const TRANSPARENT: Color = Color::rgba(0x00, 0x00, 0x00, 0x00);
    pub const GRAY: Color = Color::rgb(0x88, 0x88, 0x88);
    pub const YELLOW: Color = Color::rgb(0xFF, 0xFF, 0x00);
    pub const CYAN: Color = Color::rgb(0x00, 0xFF, 0xFF);
    pub const MAGENTA: Color = Color::rgb(0xFF, 0x00, 0xFF);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ARGB 打包/解包往返一致（Android int 颜色互转）。
    #[test]
    fn argb_roundtrip() {
        for &v in &[0xFF000000u32, 0xFFFFFFFF, 0x2082F8FF, 0x00FF00FF] {
            assert_eq!(Color::from_argb(v).to_argb(), v);
        }
    }

    /// 构造函数的 alpha 默认值。
    #[test]
    fn alpha_defaults() {
        assert_eq!(Color::rgb(1, 2, 3).a, 0xFF);
        assert_eq!(Color::rgba(1, 2, 3, 4).a, 4);
    }

    /// 常量与 Android 值对齐。
    #[test]
    fn android_constants() {
        assert_eq!(Color::BLACK.to_argb(), 0xFF000000);
        assert_eq!(Color::WHITE.to_argb(), 0xFFFFFFFF);
        assert_eq!(Color::TRANSPARENT.to_argb(), 0x00000000);
        assert_eq!(Color::BLUE.to_argb(), 0xFF0000FF);
        assert_eq!(Color::YELLOW.to_argb(), 0xFFFFFF00);
    }
}
