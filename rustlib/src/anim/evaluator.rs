//! 类型求值器（Android `TypeEvaluator` 子集）：动画值插值的类型化实现。
//!
//! # 设计
//! - 与 `ValueAnimator`（f32 专用）互补：求值器支持任意可插值类型
//!   （颜色/整数/自定义），供 `KeyframeAnimator` 与多值动画使用；
//! - `frac` 为插值器整形后的时间进度 [0,1]（与 Android `evaluate` 语义一致）；
//! - 纯函数无状态：同一 (from, to, frac) 恒等输出，便于测试与缓存。

/// 类型求值器：在 `from` 与 `to` 之间按 `frac` 插值。
///
/// # 不变量
/// `frac=0` 必须返回 `from`，`frac=1` 必须返回 `to`（端点恒等）。
pub trait TypeEvaluator<T>: Send {
    /// 计算插值结果。
    fn evaluate(&self, fraction: f32, start_value: &T, end_value: &T) -> T;
}

/// 浮点求值器（线性插值；`ValueAnimator` 内部等价逻辑的 trait 化）。
#[derive(Clone, Copy, Debug, Default)]
pub struct FloatEvaluator;

impl TypeEvaluator<f32> for FloatEvaluator {
    fn evaluate(&self, fraction: f32, start_value: &f32, end_value: &f32) -> f32 {
        start_value + (end_value - start_value) * fraction
    }
}

/// 整数求值器（Android `IntEvaluator`：线性插值后四舍五入）。
#[derive(Clone, Copy, Debug, Default)]
pub struct IntEvaluator;

impl TypeEvaluator<i32> for IntEvaluator {
    fn evaluate(&self, fraction: f32, start_value: &i32, end_value: &i32) -> i32 {
        let start = *start_value as f32;
        let end = *end_value as f32;
        (start + (end - start) * fraction).round() as i32
    }
}

/// ARGB 颜色求值器（Android `ArgbEvaluator`：四通道独立线性插值）。
///
/// # 边界
/// 颜色为 `u32`（0xAARRGGBB）；alpha 通道同样线性插值（不做 gamma 校正，
/// 与 Android 默认行为一致）。
#[derive(Clone, Copy, Debug, Default)]
pub struct ArgbEvaluator;

impl TypeEvaluator<u32> for ArgbEvaluator {
    fn evaluate(&self, fraction: f32, start_value: &u32, end_value: &u32) -> u32 {
        let start = *start_value;
        let end = *end_value;
        let sa = ((start >> 24) & 0xFF) as u8;
        let sb = ((start >> 16) & 0xFF) as u8;
        let sg = ((start >> 8) & 0xFF) as u8;
        let sr = (start & 0xFF) as u8;
        let ea = ((end >> 24) & 0xFF) as u8;
        let eb = ((end >> 16) & 0xFF) as u8;
        let eg = ((end >> 8) & 0xFF) as u8;
        let er = (end & 0xFF) as u8;
        let a = lerp_u8(sa, ea, fraction);
        let b = lerp_u8(sb, eb, fraction);
        let g = lerp_u8(sg, eg, fraction);
        let r = lerp_u8(sr, er, fraction);
        ((a as u32) << 24) | ((b as u32) << 16) | ((g as u32) << 8) | (r as u32)
    }
}

/// 单通道线性插值（u8 精度，四舍五入）。
#[inline]
fn lerp_u8(start: u8, end: u8, fraction: f32) -> u8 {
    (start as f32 + (end as f32 - start as f32) * fraction).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 浮点端点恒等。
    #[test]
    fn float_endpoints() {
        let e = FloatEvaluator;
        assert_eq!(e.evaluate(0.0, &10.0, &20.0), 10.0);
        assert_eq!(e.evaluate(1.0, &10.0, &20.0), 20.0);
        assert_eq!(e.evaluate(0.5, &10.0, &20.0), 15.0);
    }

    /// 整数四舍五入：0.5 进位。
    #[test]
    fn int_rounding() {
        let e = IntEvaluator;
        assert_eq!(e.evaluate(0.0, &0, &10), 0);
        assert_eq!(e.evaluate(1.0, &0, &10), 10);
        assert_eq!(e.evaluate(0.5, &0, &1), 1, "0.5 rounds up");
        assert_eq!(e.evaluate(0.25, &0, &1), 0, "0.25 rounds down");
    }

    /// 颜色端点恒等 + 中点通道独立插值。
    #[test]
    fn argb_endpoints_and_midpoint() {
        let e = ArgbEvaluator;
        let red = 0xFFFF0000u32;
        let blue = 0xFF0000FFu32;
        assert_eq!(e.evaluate(0.0, &red, &blue), red);
        assert_eq!(e.evaluate(1.0, &red, &blue), blue);
        // 中点：A=255, R=128, G=0, B=128。
        let mid = e.evaluate(0.5, &red, &blue);
        assert_eq!((mid >> 24) & 0xFF, 255, "A");
        assert_eq!((mid >> 16) & 0xFF, 128, "R");
        assert_eq!((mid >> 8) & 0xFF, 0, "G");
        assert_eq!(mid & 0xFF, 128, "B");
    }

    /// 颜色 alpha 通道插值（半透明过渡）。
    #[test]
    fn argb_alpha_interpolation() {
        let e = ArgbEvaluator;
        let opaque = 0xFFFF0000u32;
        let transparent = 0x00FF0000u32;
        let mid = e.evaluate(0.5, &opaque, &transparent);
        assert_eq!((mid >> 24) & 0xFF, 128, "alpha should be 128 at midpoint");
    }
}
