//! 动画模块（PRD §5.1.8，Task 19）。
//!
//! - [`interpolator`]：时间插值曲线（AOSP 公式逐字对齐）；
//! - [`animator`]：ValueAnimator/ObjectAnimator 状态机。
//!
//! 帧驱动约定：`advance(frame_time_ms)` 由 Choreographer 帧回调
//! 调用（A11：动画帧合并进帧回调，不额外开线程）。

pub mod animator;
pub mod interpolator;

pub use animator::{ObjectAnimator, Property, RepeatMode, ValueAnimator};
pub use interpolator::{
    Accelerate, AccelerateDecelerate, Anticipate, AnticipateOvershoot, Bounce, Cycle, Decelerate,
    Interpolator, Linear, Overshoot,
};
