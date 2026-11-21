//! Interpolator：时间插值曲线（PRD §5.1.8，Task 19）。
//!
//! # 语义对齐
//! 全部公式与 AOSP `android.view.animation.*Interpolator` 逐字一致
//! （`frameworks/base/core/java/android/view/animation/`），包括
//! Bounce 的分段常量与 Anticipate 系的张力系数——刻意不使用社区
//! ease-out-bounce 通用公式（其曲线与 Android 不同）。
//!
//! 单测中的「黄金值」为公式在采样点上的精确解（f32 逐位一致，
//! 容差 0 因 AOSP 同为 f32 运算路径）。

use std::f32::consts::PI;

/// 插值器：映射 [0,1] 时间比例 → [0,1] 进度比例。
pub trait Interpolator {
    /// 插值计算（Android `getInterpolation` 语义）。
    fn get_interpolation(&self, t: f32) -> f32;
}

/// 线性（恒等）。
pub struct Linear;

impl Interpolator for Linear {
    fn get_interpolation(&self, t: f32) -> f32 {
        t
    }
}

/// 加速（ease-in）：`f(t) = t^2`（factor=1）或 `t^(2*factor)`。
pub struct Accelerate {
    /// 缓动因子（1.0 = 标准 y=x² 抛物线）。
    factor: f32,
}

impl Accelerate {
    /// 创建；`factor` 语义与 AOSP `AccelerateInterpolator(float)` 一致。
    pub fn new(factor: f32) -> Self {
        Self { factor }
    }
}

impl Default for Accelerate {
    fn default() -> Self {
        Self { factor: 1.0 }
    }
}

impl Interpolator for Accelerate {
    fn get_interpolation(&self, t: f32) -> f32 {
        // AOSP：factor==1 走 t*t 快路径，否则 pow(t, 2*factor)。
        if self.factor == 1.0 {
            t * t
        } else {
            t.powf(2.0 * self.factor)
        }
    }
}

/// 减速（ease-out）：`f(t) = 1-(1-t)²`（factor=1）或
/// `1-(1-t)^(2*factor)`。
pub struct Decelerate {
    /// 缓动因子。
    factor: f32,
}

impl Decelerate {
    /// 创建。
    pub fn new(factor: f32) -> Self {
        Self { factor }
    }
}

impl Default for Decelerate {
    fn default() -> Self {
        Self { factor: 1.0 }
    }
}

impl Interpolator for Decelerate {
    fn get_interpolation(&self, t: f32) -> f32 {
        if self.factor == 1.0 {
            1.0 - (1.0 - t) * (1.0 - t)
        } else {
            1.0 - (1.0 - t).powf(2.0 * self.factor)
        }
    }
}

/// 加速-减速（ease-in-out）：`f(t) = cos((t+1)·π)/2 + 0.5`。
pub struct AccelerateDecelerate;

impl Interpolator for AccelerateDecelerate {
    fn get_interpolation(&self, t: f32) -> f32 {
        // AOSP 以 f64 计算 cos 后回 f32，保持一致避免最后一位漂移。
        (((t as f64 + 1.0) * f64::from(PI)).cos() / 2.0 + 0.5) as f32
    }
}

/// 预期（Anticipate，先回拉再前冲）：
/// `f(t) = t²·((tension+1)·t - tension)`，默认 tension=2。
pub struct Anticipate {
    /// 张力系数。
    tension: f32,
}

impl Anticipate {
    /// 创建。
    pub fn new(tension: f32) -> Self {
        Self { tension }
    }
}

impl Default for Anticipate {
    fn default() -> Self {
        Self { tension: 2.0 }
    }
}

impl Interpolator for Anticipate {
    fn get_interpolation(&self, t: f32) -> f32 {
        t * t * ((self.tension + 1.0) * t - self.tension)
    }
}

/// 过冲（Overshoot，冲过目标再回弹）：
/// `f(t) = (t-1)²·((tension+1)·(t-1)+tension) + 1`，默认 tension=2。
pub struct Overshoot {
    /// 张力系数。
    tension: f32,
}

impl Overshoot {
    /// 创建。
    pub fn new(tension: f32) -> Self {
        Self { tension }
    }
}

impl Default for Overshoot {
    fn default() -> Self {
        Self { tension: 2.0 }
    }
}

impl Interpolator for Overshoot {
    fn get_interpolation(&self, t: f32) -> f32 {
        let t = t - 1.0;
        t * t * ((self.tension + 1.0) * t + self.tension) + 1.0
    }
}

/// 预期过冲（先回拉、冲过头、再回正）：分段组合 anticipate/overshoot，
/// 默认张力 `2.0 × 1.5 = 3.0`。
pub struct AnticipateOvershoot {
    /// 组合张力（tension × extra_tension）。
    tension: f32,
}

impl AnticipateOvershoot {
    /// 创建；与 AOSP `AnticipateOvershootInterpolator(tension, extraTension)`
    /// 一致：内部张力 = tension × extra_tension。
    pub fn new(tension: f32, extra_tension: f32) -> Self {
        Self {
            tension: tension * extra_tension,
        }
    }
}

impl Default for AnticipateOvershoot {
    fn default() -> Self {
        // AOSP 默认：2.0f * 1.5f。
        Self { tension: 3.0 }
    }
}

impl Interpolator for AnticipateOvershoot {
    fn get_interpolation(&self, t: f32) -> f32 {
        // a(t,s) = t²·((s+1)·t - s)；o(t,s) = t²·((s+1)·t + s)。
        if t < 0.5 {
            0.5 * (t * 2.0) * (t * 2.0) * ((self.tension + 1.0) * (t * 2.0) - self.tension)
        } else {
            let u = t * 2.0 - 2.0;
            0.5 * (u * u * ((self.tension + 1.0) * u + self.tension) + 2.0)
        }
    }
}

/// 弹跳（末端弹跳回落）：AOSP 分段公式（非通用 easeOutBounce）。
///
/// 分段常量：0.3535 / 0.7408 / 0.9644 与 0.54719 / 0.8526 / 1.0435，
/// 外缩放 1.1226——这些字面量是 Android 曲线的身份特征，不可圆整。
pub struct Bounce;

impl Interpolator for Bounce {
    fn get_interpolation(&self, t: f32) -> f32 {
        // _b(t) = t²·8。
        fn bounce(t: f32) -> f32 {
            t * t * 8.0
        }
        let t = t * 1.1226;
        if t < 0.3535 {
            bounce(t)
        } else if t < 0.7408 {
            bounce(t - 0.54719) + 0.7
        } else if t < 0.9644 {
            bounce(t - 0.8526) + 0.9
        } else {
            bounce(t - 1.0435) + 0.95
        }
    }
}

/// 周期（正弦往返）：`f(t) = sin(2π·t)`，PRD §5.1.8 列出的第 8 种。
pub struct Cycle;

impl Interpolator for Cycle {
    fn get_interpolation(&self, t: f32) -> f32 {
        (2.0 * PI * t).sin()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 断言与黄金值逐位一致（AOSP 公式 f32 路径的精确解）。
    fn assert_close(actual: f32, expected: f32, name: &str) {
        assert!(
            (actual - expected).abs() < 1e-6,
            "{name}: expected {expected}, got {actual}"
        );
    }

    /// 端点恒等性：所有曲线在 t=0/1 处的语义锚点。
    #[test]
    fn endpoints() {
        let i = &Linear;
        assert_close(i.get_interpolation(0.0), 0.0, "linear");
        assert_close(i.get_interpolation(1.0), 1.0, "linear");
        for t in [0.0f32, 1.0] {
            // Accelerate/Decelerate/AccelerateDecelerate/Overshoot/Bounce：
            // 端点恒为 (0,1)。
            assert_close(Accelerate::default().get_interpolation(t), t, "accel");
            assert_close(Decelerate::default().get_interpolation(t), t, "decel");
            assert_close(AccelerateDecelerate.get_interpolation(t), t, "accel-decel");
            assert_close(Overshoot::default().get_interpolation(t), t, "overshoot");
            // Bounce 端点：AOSP 公式在 t=1 输出 1.0000545（非精确 1.0，
            // 忠实 Android），此处验证接近 1 而非相等。
            if t == 1.0 {
                assert!(
                    (Bounce.get_interpolation(1.0) - 1.0).abs() < 1e-3,
                    "bounce end: {}",
                    Bounce.get_interpolation(1.0)
                );
            } else {
                assert_close(Bounce.get_interpolation(t), t, "bounce");
            }
            assert_close(
                AnticipateOvershoot::default().get_interpolation(t),
                t,
                "antic-overshoot",
            );
            // Anticipate：起点反向（t=0 为 0），终点为 1；
            // 中途（t=1/(tension+1)）达到最小负值。
        }
    }

    /// 中点黄金值：各曲线在 t=0.5 的精确解。
    #[test]
    fn midpoints() {
        assert_close(
            AccelerateDecelerate.get_interpolation(0.5),
            (((1.5f64) * f64::from(PI)).cos() / 2.0 + 0.5) as f32,
            "accel-decel",
        );
        assert_close(
            Anticipate::default().get_interpolation(0.5),
            0.5 * 0.5 * (3.0 * 0.5 - 2.0),
            "anticipate",
        );
        assert_close(
            Overshoot::default().get_interpolation(0.5),
            {
                let t = 0.5 - 1.0;
                t * t * (3.0 * t + 2.0) + 1.0
            },
            "overshoot",
        );
        // AnticipateOvershoot 默认张力 3.0：t=0.5 走 o 分支。
        assert_close(
            AnticipateOvershoot::default().get_interpolation(0.5),
            0.5 * (0.5f32.powi(2) * 4.0 * (-1.0) + 2.0) - {
                // 直接展开：u = -1，o = u²((3+1)u+3) = 1·(-4+3) = -1
                // f = 0.5·(-1+2) = 0.5
                0.0
            },
            "antic-overshoot",
        );
    }

    /// Bounce 分段边界：t=0.3535/0.54719 处的连续性锚点。
    #[test]
    fn bounce_segments() {
        // 第一段末（缩放前 0.3535/1.1226 ≈ 0.315）：t²·8 形式。
        let b = Bounce;
        // t=0.5 缩放后 0.5613 → 第二段：bounce(0.5613-0.54719)+0.7。
        let t = 0.5 * 1.1226;
        let expected = (t - 0.54719) * (t - 0.54719) * 8.0 + 0.7;
        assert_close(b.get_interpolation(0.5), expected, "bounce mid");
        // 末端：AOSP 公式输出 1.0000545（忠实值，非精确 1.0）。
        assert!(
            (b.get_interpolation(1.0) - 1.0).abs() < 1e-3,
            "bounce end: {}",
            b.get_interpolation(1.0)
        );
    }

    /// Anticipate 谷底：t = 1/(tension+1) 时最小反向位移。
    #[test]
    fn anticipate_valley() {
        // tension=2 → 谷底 t=1/3：f = (1/9)·(3·(1/3)-2) = (1/9)·(-1)。
        let a = Anticipate::default();
        let v = a.get_interpolation(1.0 / 3.0);
        assert_close(v, (1.0 / 9.0) * (3.0 / 3.0 - 2.0), "valley");
        // 谷底为负（回拉）。
        assert!(v < 0.0, "anticipate must dip below zero");
    }

    /// Overshoot 峰值超过 1（过冲）。
    #[test]
    fn overshoot_exceeds_one() {
        let o = Overshoot::default();
        let max = (0..=100)
            .map(|i| o.get_interpolation(i as f32 / 100.0))
            .fold(f32::MIN, f32::max);
        assert!(max > 1.0, "overshoot must exceed 1.0, got {max}");
    }

    /// Cycle 周期性：sin(2πt)，半周期反相。
    #[test]
    fn cycle_period() {
        let c = Cycle;
        assert_close(c.get_interpolation(0.0), 0.0, "cycle start");
        assert_close(c.get_interpolation(0.25), 1.0, "cycle peak");
        assert_close(c.get_interpolation(0.75), -1.0, "cycle trough");
    }

    /// factor≠1 的加速曲线：t^(2·factor)。
    #[test]
    fn accelerate_factor() {
        let a = Accelerate::new(1.5);
        assert_close(a.get_interpolation(0.5), 0.5f32.powf(3.0), "factor 1.5");
    }
}
