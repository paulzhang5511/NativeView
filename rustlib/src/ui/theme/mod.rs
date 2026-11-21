//! M3 主题模块（Material 3 Expressive token 系统）。
//!
//! # 设计
//! - `Theme` 聚合色彩角色 + 字阶 + 形状，作为组件视觉的唯一取值点；
//! - 进程级默认主题（`OnceLock`，与字体缓存同构）：组件不传主题时
//!   取默认，宿主可整体替换；
//! - 状态层透明度常量：hover 8% / pressed 10% / focused 10% /
//!   dragged 16%（M3 规范），叠加色由调用方指定（通常 on_*）。
//!
//! # 边界
//! V1 不做动态取色（Material You）；亮暗切换由宿主重建主题完成，
//! 不做监听机制。

pub mod color;
pub mod motion;
pub mod shape;
pub mod typography;

use crate::gfx::color::Color;
use crate::gfx::paint::Paint;
use crate::gfx::path::Path;
use crate::gfx::scene::Command;
use crate::gfx::scene::Rect;
use std::sync::OnceLock;

/// 状态层透明度（M3 规范：相对 on_* 色叠加）。
pub mod state {
    pub const HOVER_ALPHA: f32 = 0.08;
    pub const PRESSED_ALPHA: f32 = 0.10;
    pub const FOCUSED_ALPHA: f32 = 0.10;
    pub const DRAGGED_ALPHA: f32 = 0.16;
}

/// 完整主题（色彩 + 字阶入口 + 默认形状）。
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    /// 色彩角色全集。
    pub roles: color::ColorRoles,
    /// 是否暗色模式（影响 scrim/阴影近似）。
    pub dark: bool,
}

impl Theme {
    /// M3 亮色基线主题。
    pub fn light() -> Self {
        Self {
            roles: color::LIGHT,
            dark: false,
        }
    }

    /// M3 暗色基线主题。
    pub fn dark() -> Self {
        Self {
            roles: color::DARK,
            dark: true,
        }
    }
}

/// 进程级默认主题。
static DEFAULT_THEME: OnceLock<Theme> = OnceLock::new();

/// 取默认主题（未设置时回落亮色基线）。
pub fn theme() -> Theme {
    *DEFAULT_THEME.get().unwrap_or(&BASE)
}

static BASE: Theme = Theme {
    roles: color::LIGHT,
    dark: false,
};

/// 替换进程级默认主题（首次设置生效，后续调用静默忽略）。
///
/// # 约束
/// 与字体注入同构：只允许调用一次，主题切换不在 V1 范围。
pub fn set_default_theme(theme: Theme) {
    let _ = DEFAULT_THEME.set(theme);
}

/// 状态层颜色合成：底色 + on_* 色按透明度叠加。
///
/// M3 状态层语义：组件背景色之上叠一层半透明的 on_* 色
/// （primary 背景叠 on_primary，surface 背景叠 on_surface）。
///
/// # 参数
/// `alpha` 取 [`state`] 模块常量（0.0-1.0）。
#[inline]
pub fn state_layer(base: Color, overlay: Color, alpha: f32) -> Color {
    let mix = |b: u8, o: u8| -> u8 { (o as f32 * alpha + b as f32 * (1.0 - alpha)).round() as u8 };
    Color {
        r: mix(base.r, overlay.r),
        g: mix(base.g, overlay.g),
        b: mix(base.b, overlay.b),
        a: base.a,
    }
}

/// 绘制 M3 状态层（圆角矩形整面叠加）。
///
/// 可交互组件在背景之上调用；`pressed`/`focused` 至少一项为真才产生
/// 命令，否则零开销（普通绘制路径不受影响）。
///
/// # 参数
/// - `bounds`：状态层覆盖区域（通常 = 控件背景矩形）；
/// - `bg`：组件背景色（合成底）；
/// - `overlay`：状态层色（通常 = 主题 on_* 角色）；
/// - `corner_radius`：与背景一致的圆角（px）。
pub fn draw_state_layer(
    scene: &mut crate::gfx::scene::Scene,
    bounds: Rect,
    bg: Color,
    overlay: Color,
    pressed: bool,
    focused: bool,
    corner_radius: f32,
) {
    // 按压优先于聚焦（M3 状态层叠加顺序：pressed 盖 focused）。
    let alpha = if pressed {
        state::PRESSED_ALPHA
    } else if focused {
        state::FOCUSED_ALPHA
    } else {
        return;
    };
    let color = state_layer(bg, overlay, alpha);
    scene.commands.push(Command::FillPath {
        path: Path::rounded_rect(
            bounds.x,
            bounds.y,
            bounds.w,
            bounds.h,
            f64::from(corner_radius),
        ),
        paint: Paint::default().color(color),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 未设置默认主题时回落亮色基线。
    #[test]
    fn default_theme_is_light_baseline() {
        let t = theme();
        assert!(!t.dark);
        assert_eq!(t.roles.primary, color::LIGHT.primary);
    }

    /// 状态层合成：白底叠 10% 黑 ≈ 90% 亮度白。
    #[test]
    fn state_layer_blends_by_alpha() {
        let out = state_layer(Color::WHITE, Color::BLACK, 0.10);
        assert_eq!(out.r, 230);
        assert_eq!(out.g, 230);
        assert_eq!(out.b, 230);
        assert_eq!(out.a, 0xFF);
    }

    /// 状态层透明度与 M3 规范一致。
    #[test]
    fn state_layer_alphas_match_m3() {
        assert_eq!(state::HOVER_ALPHA, 0.08);
        assert_eq!(state::PRESSED_ALPHA, 0.10);
        assert_eq!(state::FOCUSED_ALPHA, 0.10);
        assert_eq!(state::DRAGGED_ALPHA, 0.16);
    }
}
