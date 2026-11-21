//! 显示密度注入（dp/sp 换算基准）。
//!
//! # 为什么需要
//! Android 的布局尺寸语义是 dp（密度无关像素），文字尺寸语义是 sp
//!（受用户字体缩放影响）。Rust 侧所有几何量都以 px 表达，因此必须
//! 拿到设备的 `density`（px/dp）与 `scaledDensity`（px/sp）才能把
//! 设计稿的 dp/sp 值转成物理像素。
//!
//! # 数据来源
//! 路线 B 下由 Kotlin 空壳 `MyNativeActivity` 在 `onCreate` 读取
//! `resources.displayMetrics` 后经 JNI 注入（与 insets 同构的最小桥）。
//!
//! # 兜底语义
//! 未注入时 `density` 与 `scaled_density` 均为 **1.0**——保证主机侧
//! 单测/黄金图在无 Android 环境时行为可预测（1dp = 1px），也让注入
//! 前的极早期调用不会崩。
//!
//! # 配置变化
//! 用户改系统字体大小、切换显示大小、分屏密度变化时，Activity 会
//! 重建或收到 `onConfigurationChanged`，Kotlin 侧重新注入最新值。

use std::sync::atomic::{AtomicU32, Ordering};

/// 密度基数（px/dp），以 f32 位模式存放（原子读写避免锁）。
/// 100 = 1.00（未注入时的兜底值）。
static DENSITY_BITS: AtomicU32 = AtomicU32::new(1.0f32.to_bits());

/// 字体缩放基数（px/sp）。100 = 1.00。
static SCALED_DENSITY_BITS: AtomicU32 = AtomicU32::new(1.0f32.to_bits());

/// Kotlin 侧注入密度参数（JNI 入口调用；重复调用以最新为准）。
///
/// # 参数
/// `density`：`DisplayMetrics.density`（px/dp，典型 1.0~4.0）；
/// `scaled_density`：`DisplayMetrics.scaledDensity`（px/sp，含用户字体缩放）。
///
/// # 防御
/// 非有限值或非正值一律忽略（保留上一次有效值），避免除零与 NaN 传播。
pub fn set_density(density: f32, scaled_density: f32) {
    if density.is_finite() && density > 0.0 {
        DENSITY_BITS.store(density.to_bits(), Ordering::Release);
    }
    if scaled_density.is_finite() && scaled_density > 0.0 {
        SCALED_DENSITY_BITS.store(scaled_density.to_bits(), Ordering::Release);
    }
}

/// 当前密度（px/dp；未注入为 1.0）。
pub fn density() -> f32 {
    f32::from_bits(DENSITY_BITS.load(Ordering::Acquire))
}

/// 当前字体缩放密度（px/sp；未注入为 1.0）。
pub fn scaled_density() -> f32 {
    f32::from_bits(SCALED_DENSITY_BITS.load(Ordering::Acquire))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试串行化（全局静态状态）。
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn reset() {
        DENSITY_BITS.store(1.0f32.to_bits(), Ordering::Release);
        SCALED_DENSITY_BITS.store(1.0f32.to_bits(), Ordering::Release);
    }

    /// 默认 1.0（主机兜底）。
    #[test]
    fn defaults_to_one() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        assert_eq!(density(), 1.0);
        assert_eq!(scaled_density(), 1.0);
    }

    /// 注入生效。
    #[test]
    fn injects_values() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        set_density(4.0, 4.5);
        assert_eq!(density(), 4.0);
        assert_eq!(scaled_density(), 4.5);
        reset();
    }

    /// 非法值被忽略（保留上次有效值）。
    #[test]
    fn rejects_invalid() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        set_density(3.0, 3.0);
        set_density(0.0, -1.0);
        assert_eq!(density(), 3.0, "zero density rejected");
        assert_eq!(scaled_density(), 3.0, "negative rejected");
        set_density(f32::NAN, f32::INFINITY);
        assert_eq!(density(), 3.0, "NaN rejected");
        assert_eq!(scaled_density(), 3.0, "infinity rejected");
        reset();
    }
}
