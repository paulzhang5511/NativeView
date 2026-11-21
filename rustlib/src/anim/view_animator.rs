//! View 属性动画构建器（Android `ViewPropertyAnimator` 子集）：
//! 流式 API 同时动画多个 View 属性。
//!
//! # 设计
//! - 流式构建：`ViewPropertyAnimator::new().alpha(0.0).translation_x(100.0).set_duration(300)`；
//! - 帧驱动：`advance(&mut view, now_ms)` 由主循环调用，写入 View 属性；
//! - 多属性共享同一时间轴（duration/interpolator/delay），与 Android
//!   `ViewPropertyAnimator` 语义一致（所有属性同步起止）；
//! - 监听器：`set_listener` 注册 onStart/onEnd/onCancel 回调。

use crate::anim::animator::ValueAnimator;
use crate::anim::interpolator::Interpolator;
use crate::ui::view::View;

/// 动画生命周期监听器（Android `AnimatorListener` 子集）。
///
/// # 回调时机
/// - `on_start`：动画从 Idle/Delaying 进入 Running 的首帧；
/// - `on_end`：动画自然结束（到达终值）；
/// - `on_cancel`：动画被取消（不保证到终值）。
pub struct AnimatorListener {
    on_start: Option<Box<dyn FnMut() + Send>>,
    on_end: Option<Box<dyn FnMut() + Send>>,
    on_cancel: Option<Box<dyn FnMut() + Send>>,
}

impl AnimatorListener {
    /// 创建空监听器（无回调）。
    pub fn new() -> Self {
        Self {
            on_start: None,
            on_end: None,
            on_cancel: None,
        }
    }

    /// 注册 onStart 回调。
    pub fn on_start(&mut self, f: impl FnMut() + Send + 'static) -> &mut Self {
        self.on_start = Some(Box::new(f));
        self
    }

    /// 注册 onEnd 回调。
    pub fn on_end(&mut self, f: impl FnMut() + Send + 'static) -> &mut Self {
        self.on_end = Some(Box::new(f));
        self
    }

    /// 注册 onCancel 回调。
    pub fn on_cancel(&mut self, f: impl FnMut() + Send + 'static) -> &mut Self {
        self.on_cancel = Some(Box::new(f));
        self
    }

    /// 触发 onStart。
    pub fn fire_start(&mut self) {
        if let Some(f) = self.on_start.as_mut() {
            f();
        }
    }

    /// 触发 onEnd。
    pub fn fire_end(&mut self) {
        if let Some(f) = self.on_end.as_mut() {
            f();
        }
    }

    /// 触发 onCancel。
    pub fn fire_cancel(&mut self) {
        if let Some(f) = self.on_cancel.as_mut() {
            f();
        }
    }
}

impl Default for AnimatorListener {
    fn default() -> Self {
        Self::new()
    }
}

/// 可动画属性槽（内部存储目标值）。
#[derive(Clone, Copy, Debug, Default)]
struct PropSlot {
    enabled: bool,
    target: f32,
}

/// View 属性动画构建器（Android `ViewPropertyAnimator` 子集）。
///
/// # 用法
/// ```ignore
/// let mut anim = ViewPropertyAnimator::new()
///     .alpha(0.0)
///     .translation_x(200.0)
///     .set_duration(300)
///     .set_interpolator(AccelerateDecelerate::default());
///
/// // 帧循环中：
/// if anim.advance(&mut view, now_ms) {
///     // invalidate
/// }
/// ```
pub struct ViewPropertyAnimator {
    /// 各属性目标值（None = 不动画该属性）。
    alpha: PropSlot,
    translation_x: PropSlot,
    translation_y: PropSlot,
    scale_x: PropSlot,
    scale_y: PropSlot,
    rotation: PropSlot,
    /// 时长（ms）。
    duration: u32,
    /// 起始延迟（ms）。
    start_delay: u32,
    /// 插值器。
    interpolator: Box<dyn Interpolator + Send>,
    /// 监听器。
    listener: AnimatorListener,
    /// 动画起始时间（首帧锚定）。
    start_time: Option<u64>,
    /// 是否已触发 onStart。
    started: bool,
    /// 是否已结束。
    ended: bool,
    /// 是否已取消。
    cancelled: bool,
}

impl ViewPropertyAnimator {
    /// 创建空构建器（未设置任何属性 = 无操作）。
    pub fn new() -> Self {
        Self {
            alpha: PropSlot::default(),
            translation_x: PropSlot::default(),
            translation_y: PropSlot::default(),
            scale_x: PropSlot::default(),
            scale_y: PropSlot::default(),
            rotation: PropSlot::default(),
            duration: 300,
            start_delay: 0,
            interpolator: Box::new(crate::anim::interpolator::AccelerateDecelerate),
            listener: AnimatorListener::new(),
            start_time: None,
            started: false,
            ended: false,
            cancelled: false,
        }
    }

    /// 设置目标 alpha [0,1]。
    pub fn alpha(&mut self, target: f32) -> &mut Self {
        self.alpha = PropSlot {
            enabled: true,
            target,
        };
        self
    }

    /// 设置目标 translationX。
    pub fn translation_x(&mut self, target: f32) -> &mut Self {
        self.translation_x = PropSlot {
            enabled: true,
            target,
        };
        self
    }

    /// 设置目标 translationY。
    pub fn translation_y(&mut self, target: f32) -> &mut Self {
        self.translation_y = PropSlot {
            enabled: true,
            target,
        };
        self
    }

    /// 设置目标 scaleX。
    pub fn scale_x(&mut self, target: f32) -> &mut Self {
        self.scale_x = PropSlot {
            enabled: true,
            target,
        };
        self
    }

    /// 设置目标 scaleY。
    pub fn scale_y(&mut self, target: f32) -> &mut Self {
        self.scale_y = PropSlot {
            enabled: true,
            target,
        };
        self
    }

    /// 设置目标 rotation（度）。
    pub fn rotation(&mut self, target: f32) -> &mut Self {
        self.rotation = PropSlot {
            enabled: true,
            target,
        };
        self
    }

    /// 设置时长（ms）。
    pub fn set_duration(&mut self, ms: u32) -> &mut Self {
        self.duration = ms.max(1);
        self
    }

    /// 设置起始延迟（ms）。
    pub fn set_start_delay(&mut self, ms: u32) -> &mut Self {
        self.start_delay = ms;
        self
    }

    /// 设置插值器。
    pub fn set_interpolator(&mut self, i: impl Interpolator + Send + 'static) -> &mut Self {
        self.interpolator = Box::new(i);
        self
    }

    /// 设置监听器。
    pub fn set_listener(&mut self, l: AnimatorListener) -> &mut Self {
        self.listener = l;
        self
    }

    /// 是否有属性被设置（空构建器 advance 恒返回 false）。
    pub fn is_empty(&self) -> bool {
        !self.alpha.enabled
            && !self.translation_x.enabled
            && !self.translation_y.enabled
            && !self.scale_x.enabled
            && !self.scale_y.enabled
            && !self.rotation.enabled
    }

    /// 是否仍在活动。
    pub fn is_running(&self) -> bool {
        !self.ended && !self.cancelled && !self.is_empty()
    }

    /// 是否已终止。
    pub fn is_ended(&self) -> bool {
        self.ended || self.cancelled
    }

    /// 帧推进：计算插值并写入 View 属性。
    ///
    /// # 返回
    /// true = 本帧更新了属性（调用方应 invalidate）。
    pub fn advance(&mut self, view: &mut View, now_ms: u64) -> bool {
        if self.ended || self.cancelled || self.is_empty() {
            return false;
        }
        let start = *self
            .start_time
            .get_or_insert(now_ms + self.start_delay as u64);
        if now_ms < start {
            return false; // 延迟等待中。
        }
        // 首帧触发 onStart。
        if !self.started {
            self.started = true;
            self.listener.fire_start();
        }

        let elapsed = now_ms - start;
        let dur = self.duration as u64;
        if elapsed >= dur {
            // 到达终值：写入目标值并结束。
            self.apply(view, 1.0);
            self.ended = true;
            self.listener.fire_end();
            return true;
        }

        let frac = (elapsed as f32 / dur as f32).min(1.0);
        let shaped = self.interpolator.get_interpolation(frac);
        self.apply(view, shaped);
        true
    }

    /// 写入所有已启用属性（frac = 插值器整形后的进度 [0,1]）。
    fn apply(&self, view: &mut View, frac: f32) {
        if self.alpha.enabled {
            let from = view.alpha();
            view.set_alpha(from + (self.alpha.target - from) * frac);
        }
        if self.translation_x.enabled {
            let from = view.translation_x();
            view.set_translation_x(from + (self.translation_x.target - from) * frac);
        }
        if self.translation_y.enabled {
            let from = view.translation_y();
            view.set_translation_y(from + (self.translation_y.target - from) * frac);
        }
        if self.scale_x.enabled {
            let from = view.scale_x();
            view.set_scale_x(from + (self.scale_x.target - from) * frac);
        }
        if self.scale_y.enabled {
            let from = view.scale_y();
            view.set_scale_y(from + (self.scale_y.target - from) * frac);
        }
        if self.rotation.enabled {
            let from = view.rotation();
            view.set_rotation(from + (self.rotation.target - from) * frac);
        }
    }

    /// 取消：立即停止，触发 onCancel。
    pub fn cancel(&mut self) {
        if self.ended || self.cancelled {
            return;
        }
        self.cancelled = true;
        self.listener.fire_cancel();
    }

    /// 结束：跳到终值，触发 onEnd。
    pub fn end(&mut self, view: &mut View) {
        if self.ended || self.cancelled {
            return;
        }
        self.apply(view, 1.0);
        self.ended = true;
        self.listener.fire_end();
    }
}

impl Default for ViewPropertyAnimator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::interpolator::Linear;

    /// 单属性动画：alpha 1→0。
    #[test]
    fn alpha_fade_out() {
        let mut v = View::new();
        v.set_alpha(1.0);
        let mut anim = ViewPropertyAnimator::new();
        anim.alpha(0.0).set_duration(100).set_interpolator(Linear);

        assert!(anim.advance(&mut v, 0));
        assert!((v.alpha() - 1.0).abs() < f32::EPSILON);
        assert!(anim.advance(&mut v, 50));
        assert!(
            (v.alpha() - 0.5).abs() < 0.01,
            "midpoint alpha ≈ 0.5, got {}",
            v.alpha()
        );
        assert!(anim.advance(&mut v, 100));
        assert!((v.alpha() - 0.0).abs() < f32::EPSILON);
        assert!(anim.is_ended());
    }

    /// 多属性同步：alpha + translationX 同时动画。
    #[test]
    fn multi_property_sync() {
        let mut v = View::new();
        v.set_alpha(1.0);
        v.set_translation_x(0.0);
        let mut anim = ViewPropertyAnimator::new();
        anim.alpha(0.0)
            .translation_x(200.0)
            .set_duration(100)
            .set_interpolator(Linear);

        assert!(anim.advance(&mut v, 0)); // 锚定起始
        assert!(anim.advance(&mut v, 50));
        assert!((v.alpha() - 0.5).abs() < 0.01);
        assert!((v.translation_x() - 100.0).abs() < 1.0);
        assert!(anim.advance(&mut v, 100));
        assert!((v.alpha() - 0.0).abs() < f32::EPSILON);
        assert!((v.translation_x() - 200.0).abs() < f32::EPSILON);
    }

    /// 监听器：onStart 首帧触发，onEnd 终帧触发。
    #[test]
    fn listener_callbacks() {
        let mut v = View::new();
        let start_count = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let end_count = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let sc = std::sync::Arc::clone(&start_count);
        let ec = std::sync::Arc::clone(&end_count);

        let mut listener = AnimatorListener::new();
        listener.on_start(move || {
            sc.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });
        listener.on_end(move || {
            ec.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });

        let mut anim = ViewPropertyAnimator::new();
        anim.alpha(0.0)
            .set_duration(100)
            .set_interpolator(Linear)
            .set_listener(listener);

        assert!(anim.advance(&mut v, 0));
        assert_eq!(start_count.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert_eq!(end_count.load(std::sync::atomic::Ordering::Relaxed), 0);
        assert!(anim.advance(&mut v, 100));
        assert_eq!(end_count.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    /// 取消：onCancel 触发，后续 advance 无更新。
    #[test]
    fn cancel_fires_listener() {
        let mut v = View::new();
        let cancel_count = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let cc = std::sync::Arc::clone(&cancel_count);
        let mut listener = AnimatorListener::new();
        listener.on_cancel(move || {
            cc.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });

        let mut anim = ViewPropertyAnimator::new();
        anim.alpha(0.0).set_duration(1000).set_listener(listener);

        assert!(anim.advance(&mut v, 0));
        anim.cancel();
        assert_eq!(cancel_count.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert!(!anim.advance(&mut v, 100));
    }

    /// 空构建器：advance 恒 false。
    #[test]
    fn empty_animator_noop() {
        let mut v = View::new();
        let mut anim = ViewPropertyAnimator::new();
        assert!(anim.is_empty());
        assert!(!anim.advance(&mut v, 0));
    }

    /// 延迟：延迟期内无更新。
    #[test]
    fn delay_holds() {
        let mut v = View::new();
        v.set_alpha(1.0);
        let mut anim = ViewPropertyAnimator::new();
        anim.alpha(0.0)
            .set_duration(100)
            .set_start_delay(50)
            .set_interpolator(Linear);

        assert!(!anim.advance(&mut v, 0), "within delay");
        assert!(!anim.advance(&mut v, 49));
        assert!(anim.advance(&mut v, 50), "delay elapsed");
    }
}
