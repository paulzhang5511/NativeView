//! 动画模块（PRD §5.1.8，Task 19）。
//!
//! - [`interpolator`]：时间插值曲线（AOSP 公式逐字对齐）；
//! - [`animator`]：ValueAnimator/ObjectAnimator 状态机；
//! - [`animator_set`]：AnimatorSet 顺序/并行组合；
//! - [`view_animator`]：ViewPropertyAnimator 多属性流式动画 + 监听器；
//! - [`evaluator`]：类型求值器（Float/Int/Argb）；
//! - [`keyframe`]：关键帧动画（多段插值）。
//!
//! 帧驱动约定：`advance(frame_time_ms)` 由 Choreographer 帧回调
//! 调用（A11：动画帧合并进帧回调，不额外开线程）。

pub mod animator;
pub mod animator_set;
pub mod evaluator;
pub mod interpolator;
pub mod keyframe;
pub mod transition;
pub mod view_animator;

pub use animator::{ObjectAnimator, Property, RepeatMode, ValueAnimator};
pub use animator_set::{Animatable, AnimatorSet, Order};
pub use evaluator::{ArgbEvaluator, FloatEvaluator, IntEvaluator, TypeEvaluator};
pub use interpolator::{
    Accelerate, AccelerateDecelerate, Anticipate, AnticipateOvershoot, Bounce, Cycle, Decelerate,
    Interpolator, Linear, Overshoot,
};
pub use keyframe::{Keyframe, KeyframeAnimator};
pub use transition::{
    FadeTransition, ScaleTransition, SlideDirection, SlideTransition, Transition,
    TransitionController,
};
pub use view_animator::{AnimatorListener, ViewPropertyAnimator};
