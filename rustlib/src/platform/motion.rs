//! 系统动画时长倍率（`Settings.Global.ANIMATOR_DURATION_SCALE`）。
//!
//! # 为什么需要
//! 1. **无障碍合规**：用户开启「移除动画」时该值变为 0，自绘动画必须
//!    直接落到终态，而不是继续播放；
//! 2. **开发/验证**：把倍率调大即可得到慢镜头，逐帧检查动效细节。
//!
//! # 约定
//! - 默认 1.0（正常速度）；Kotlin 侧在 `onCreate`/`onResume` 读取后经 JNI 注入；
//! - 值为 0 表示「关闭动画」，由动画层解释为「一帧内推进到终态」；
//! - 仅存一个原子 f32 位模式，读取无锁、跨线程安全。

use std::sync::atomic::{AtomicU32, Ordering};

/// 当前倍率（f32 位模式存储，避免额外的锁）。
static SCALE: AtomicU32 = AtomicU32::new(0x3F80_0000); // 1.0f32

/// 注入系统动画倍率（非有限值或负值按 0 处理）。
pub fn set_animator_scale(scale: f32) {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        0.0
    };
    SCALE.store(scale.to_bits(), Ordering::Relaxed);
}

/// 读取当前倍率（默认 1.0）。
pub fn animator_scale() -> f32 {
    f32::from_bits(SCALE.load(Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_roundtrips_and_rejects_garbage() {
        // 注意：全局原子，测试结束必须复位，避免影响其它用例。
        set_animator_scale(2.5);
        assert_eq!(animator_scale(), 2.5);
        set_animator_scale(f32::NAN);
        assert_eq!(animator_scale(), 0.0);
        set_animator_scale(-1.0);
        assert_eq!(animator_scale(), 0.0);
        set_animator_scale(1.0);
        assert_eq!(animator_scale(), 1.0);
    }
}
