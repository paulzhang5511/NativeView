//! M3 色彩角色（Material 3 Expressive 静态基线）。
//!
//! # 设计
//! - 角色集对齐 M3 color roles：accent 三系 × 四角色 + surface 五级容器
//!   （Expressive 标志）+ outline/error；
//! - 不做动态取色（Material You）：亮暗两套静态基线紫调板；
//! - 固定色（`*_fixed`）V1 仅保留字段不参与暗色映射，便于未来扩展。
//!
//! # 边界
//! 全部 sRGB 不透明色（M3 基线无透明色板）；状态层透明度由
//! [`super::state::STATE_LAYER_ALPHA`] 承担，不烘焙进角色色。

use crate::gfx::color::Color;

/// M3 色彩角色全集（一个主题的完整令牌集）。
///
/// 字段按 M3 规范命名（snake_case）；`Theme` 持有一份，组件经
/// [`crate::ui::theme::theme()`] 取当前主题后按需读取角色。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorRoles {
    // ── 强调三系 ──
    pub primary: Color,
    pub on_primary: Color,
    pub primary_container: Color,
    pub on_primary_container: Color,
    pub secondary: Color,
    pub on_secondary: Color,
    pub secondary_container: Color,
    pub on_secondary_container: Color,
    pub tertiary: Color,
    pub on_tertiary: Color,
    pub tertiary_container: Color,
    pub on_tertiary_container: Color,

    // ── 表面（Expressive 五级容器）──
    pub surface: Color,
    pub on_surface: Color,
    pub on_surface_variant: Color,
    pub surface_dim: Color,
    pub surface_bright: Color,
    pub surface_container_lowest: Color,
    pub surface_container_low: Color,
    pub surface_container: Color,
    pub surface_container_high: Color,
    pub surface_container_highest: Color,

    // ── 边界与错误 ──
    pub outline: Color,
    pub outline_variant: Color,
    pub error: Color,
    pub on_error: Color,
    pub error_container: Color,
    pub on_error_container: Color,

    // ── 反色（Snackbar/Tooltip 底）──
    pub inverse_surface: Color,
    pub inverse_on_surface: Color,
    pub inverse_primary: Color,

    // ── 固定色（V1 仅保留字段，不参与暗色映射）──
    pub primary_fixed: Color,
    pub primary_fixed_dim: Color,
    pub on_primary_fixed: Color,
    pub on_primary_fixed_variant: Color,
    pub secondary_fixed: Color,
    pub on_secondary_fixed: Color,
    pub tertiary_fixed: Color,
    pub on_tertiary_fixed: Color,
}

/// M3 baseline 亮色调板（紫基调，静态近似）。
///
/// 数值取自 M3 baseline scheme 的常用近似值；不做 tonal palette
/// 生成（HCT 算法超出 V1 范围），直接固化查表。
pub const LIGHT: ColorRoles = ColorRoles {
    primary: Color::rgb(0x67, 0x50, 0xA4),
    on_primary: Color::WHITE,
    primary_container: Color::rgb(0xE9, 0xDD, 0xFF),
    on_primary_container: Color::rgb(0x21, 0x00, 0x5C),
    secondary: Color::rgb(0x62, 0x5B, 0x71),
    on_secondary: Color::WHITE,
    secondary_container: Color::rgb(0xE8, 0xDF, 0xF8),
    on_secondary_container: Color::rgb(0x1E, 0x1B, 0x29),
    tertiary: Color::rgb(0x7D, 0x53, 0x63),
    on_tertiary: Color::WHITE,
    tertiary_container: Color::rgb(0xFF, 0xD9, 0xE4),
    on_tertiary_container: Color::rgb(0x3A, 0x11, 0x1C),
    surface: Color::rgb(0xFC, 0xF8, 0xFD),
    on_surface: Color::rgb(0x1C, 0x1B, 0x1F),
    on_surface_variant: Color::rgb(0x49, 0x45, 0x4F),
    surface_dim: Color::rgb(0xDD, 0xDA, 0xE0),
    surface_bright: Color::rgb(0xFC, 0xF8, 0xFD),
    surface_container_lowest: Color::WHITE,
    surface_container_low: Color::rgb(0xF7, 0xF2, 0xFA),
    surface_container: Color::rgb(0xF1, 0xEC, 0xF4),
    surface_container_high: Color::rgb(0xEB, 0xE7, 0xEE),
    surface_container_highest: Color::rgb(0xE5, 0xE1, 0xE9),
    outline: Color::rgb(0x7A, 0x75, 0x7F),
    outline_variant: Color::rgb(0xC9, 0xC5, 0xD0),
    error: Color::rgb(0xB3, 0x26, 0x1E),
    on_error: Color::WHITE,
    error_container: Color::rgb(0xF9, 0xDE, 0xDC),
    on_error_container: Color::rgb(0x41, 0x0E, 0x0B),
    inverse_surface: Color::rgb(0x31, 0x30, 0x36),
    inverse_on_surface: Color::rgb(0xF4, 0xEF, 0xF7),
    inverse_primary: Color::rgb(0xCF, 0xBC, 0xFF),
    primary_fixed: Color::rgb(0xE9, 0xDD, 0xFF),
    primary_fixed_dim: Color::rgb(0xCF, 0xBC, 0xFF),
    on_primary_fixed: Color::rgb(0x21, 0x00, 0x5C),
    on_primary_fixed_variant: Color::rgb(0x4A, 0x30, 0xB9),
    secondary_fixed: Color::rgb(0xE8, 0xDF, 0xF8),
    on_secondary_fixed: Color::rgb(0x1E, 0x1B, 0x29),
    tertiary_fixed: Color::rgb(0xFF, 0xD9, 0xE4),
    on_tertiary_fixed: Color::rgb(0x3A, 0x11, 0x1C),
};

/// M3 baseline 暗色调板（与 [`LIGHT`] 同源的暗色映射）。
pub const DARK: ColorRoles = ColorRoles {
    primary: Color::rgb(0xCF, 0xBC, 0xFF),
    on_primary: Color::rgb(0x38, 0x1E, 0x8C),
    primary_container: Color::rgb(0x4F, 0x37, 0xA5),
    on_primary_container: Color::rgb(0xE9, 0xDD, 0xFF),
    secondary: Color::rgb(0xCC, 0xBF, 0xDB),
    on_secondary: Color::rgb(0x33, 0x2F, 0x41),
    secondary_container: Color::rgb(0x4A, 0x46, 0x57),
    on_secondary_container: Color::rgb(0xE8, 0xDF, 0xF8),
    tertiary: Color::rgb(0xEF, 0xB8, 0xC8),
    on_tertiary: Color::rgb(0x4A, 0x25, 0x33),
    tertiary_container: Color::rgb(0x63, 0x3B, 0x4B),
    on_tertiary_container: Color::rgb(0xFF, 0xD9, 0xE4),
    surface: Color::rgb(0x1C, 0x1B, 0x1F),
    on_surface: Color::rgb(0xE5, 0xE1, 0xE9),
    on_surface_variant: Color::rgb(0xC9, 0xC5, 0xD0),
    surface_dim: Color::rgb(0x1C, 0x1B, 0x1F),
    surface_bright: Color::rgb(0x3C, 0x3A, 0x41),
    surface_container_lowest: Color::rgb(0x16, 0x15, 0x1A),
    surface_container_low: Color::rgb(0x24, 0x23, 0x27),
    surface_container: Color::rgb(0x28, 0x27, 0x2B),
    surface_container_high: Color::rgb(0x33, 0x31, 0x36),
    surface_container_highest: Color::rgb(0x3D, 0x3B, 0x41),
    outline: Color::rgb(0x94, 0x8F, 0x99),
    outline_variant: Color::rgb(0x49, 0x45, 0x4F),
    error: Color::rgb(0xF2, 0xB8, 0xB5),
    on_error: Color::rgb(0x60, 0x1B, 0x14),
    error_container: Color::rgb(0x8C, 0x1D, 0x18),
    on_error_container: Color::rgb(0xF9, 0xDE, 0xDC),
    inverse_surface: Color::rgb(0xE5, 0xE1, 0xE9),
    inverse_on_surface: Color::rgb(0x31, 0x30, 0x36),
    inverse_primary: Color::rgb(0x67, 0x50, 0xA4),
    primary_fixed: Color::rgb(0xE9, 0xDD, 0xFF),
    primary_fixed_dim: Color::rgb(0xCF, 0xBC, 0xFF),
    on_primary_fixed: Color::rgb(0x21, 0x00, 0x5C),
    on_primary_fixed_variant: Color::rgb(0x4A, 0x30, 0xB9),
    secondary_fixed: Color::rgb(0xE8, 0xDF, 0xF8),
    on_secondary_fixed: Color::rgb(0x1E, 0x1B, 0x29),
    tertiary_fixed: Color::rgb(0xFF, 0xD9, 0xE4),
    on_tertiary_fixed: Color::rgb(0x3A, 0x11, 0x1C),
};

#[cfg(test)]
mod tests {
    use super::*;

    /// 亮暗调板必须可区分：primary 在两套主题中不同。
    #[test]
    fn light_dark_palettes_differ() {
        assert_ne!(LIGHT.primary, DARK.primary);
        assert_ne!(LIGHT.surface, DARK.surface);
    }

    /// on_primary 与 primary 必须有足够对比（亮主题白字紫底）。
    #[test]
    fn on_primary_contrasts_primary() {
        assert_eq!(LIGHT.on_primary, Color::WHITE);
        assert_ne!(LIGHT.on_primary, LIGHT.primary);
    }

    /// 角色色不允许 alpha 透明（状态层透明度单独叠加）。
    #[test]
    fn roles_are_opaque() {
        let check = |c: Color| assert_eq!(c.a, 0xFF);
        check(LIGHT.primary);
        check(DARK.primary);
        check(LIGHT.inverse_surface);
        check(DARK.error_container);
    }
}
