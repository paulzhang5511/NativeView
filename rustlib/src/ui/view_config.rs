//! 视图交互常量（对齐 Android `android.view.ViewConfiguration`）。
//!
//! # 为什么必须是运行时函数而非常量
//! Android 的 `ViewConfiguration.get(context).getScaledTouchSlop()` 返回
//! **按设备密度缩放**后的像素值（基准 8dp）。若框架写成 `const 8.0` px，
//! 在 640dpi（density=4.0）设备上实际只有 2dp——手指轻微抖动就会被判成
//! 拖拽，点击识别大面积失效。这是物理尺寸语义，必须经 [`dp`] 换算。
//!
//! # 唯一真源
//! 本模块是全框架触摸阈值的**唯一**定义处：手势识别（`input::dispatch`）
//! 与滚动容器（`widgets::scroll_view`）都必须引用这里，禁止各自定义常量
//!（历史上曾两处各写 `8.0`，语义漂移难以发现）。
//!
//! # 时间常量
//! 长按、双击等**时间**阈值与密度无关，保持毫秒常量即可。

use crate::ui::dimen::dp;

/// 触摸滑动阈值（px）：移动超过此距离判定为拖拽而非点击。
///
/// Android 基准为 8dp（`ViewConfiguration.TOUCH_SLOP`），
/// 640dpi 设备上换算后为 32px。
#[inline]
pub fn touch_slop() -> f32 {
    dp(8.0)
}

/// 最小可触摸目标尺寸（px，Android `ViewConfiguration.TOUCH_TARGET_SIZE`）。
///
/// 无障碍要求：任何可点击元素的实际命中区不应小于 48dp×48dp。
#[inline]
pub fn touch_target_size() -> f32 {
    dp(48.0)
}

/// 长按触发时长（ms；与密度无关，Android 默认 400ms）。
pub const LONG_PRESS_TIMEOUT_MS: i64 = 400;

#[cfg(test)]
mod tests {
    use super::*;

    /// 主机侧（density=1.0）：slop 为 8px 基准值。
    #[test]
    fn touch_slop_host_baseline() {
        #[cfg(not(target_os = "android"))]
        assert_eq!(touch_slop(), 8.0);
    }

    /// 主机侧：可触摸目标 48px 基准。
    #[test]
    fn touch_target_host_baseline() {
        #[cfg(not(target_os = "android"))]
        assert_eq!(touch_target_size(), 48.0);
    }

    /// 时间常量与密度无关。
    #[test]
    fn long_press_timeout_constant() {
        assert_eq!(LONG_PRESS_TIMEOUT_MS, 400);
    }
}
