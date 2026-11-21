//! 属性动画（PRD §5.1.8，Task 19）：ValueAnimator 与 ObjectAnimator。
//!
//! # 设计
//! - 帧驱动：`AnimationSet::advance(frame_time_ms)` 由 Choreographer
//!   帧回调调用，不内建线程（A11 约束：动画帧合并进帧回调）；
//! - ValueAnimator 纯状态机：ofFloat 区间 + duration/delay/repeat +
//!   Interpolator，`advance` 推进并输出插值；
//! - ObjectAnimator 绑定 View 数值属性：通过 setter 函数指针写入
//!   （translation/alpha/scale/rotation 等），避免反射；
//! - 取消语义：cancel 立即置终态并停发回调（与 Android `cancel`
//!   调用 onAnimationEnd 一致），end 同理。

use crate::anim::interpolator;
use crate::ui::view::View;

/// 重复模式（Android `RESTART`/`REVERSE`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepeatMode {
    /// 重新从头开始。
    Restart,
    /// 下一次反向播放。
    Reverse,
}

/// 动画运行状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// 未开始（等待 start 或 delay）。
    Idle,
    /// 延迟等待中。
    Delaying,
    /// 播放中。
    Running,
    /// 自然结束。
    Ended,
    /// 被取消。
    Cancelled,
}

/// 数值区间动画（Android `ValueAnimator.ofFloat` 子集）。
///
/// # 时间基准
/// 毫秒（与 Choreographer 帧时间一致）；`advance` 传入当前帧时间，
/// 内部保存起始帧时间计算进度。
pub struct ValueAnimator {
    /// 起始值。
    from: f32,
    /// 结束值。
    to: f32,
    /// 时长（ms）。
    duration: u32,
    /// 起始延迟（ms）。
    start_delay: u32,
    /// 重复次数（0 = 播一次；u32::MAX 近似 INFINITE，V1 不设无限）。
    repeat_count: u32,
    /// 重复模式。
    repeat_mode: RepeatMode,
    /// 插值器。
    interpolator: Box<dyn Interpolator + Send>,
    /// 当前阶段。
    phase: Phase,
    /// 动画起始时间（含 delay；首次 advance 时锚定）。
    start_time: Option<u64>,
    /// 已完成整轮数。
    cycles_done: u32,
    /// 反向标志（Reverse 模式的奇数轮）。
    reversed: bool,
    /// 每帧更新回调（ObjectAnimator 经此写属性）。
    on_update: Option<Box<dyn FnMut(f32) + Send>>,
}

impl ValueAnimator {
    /// ofFloat 创建：区间 + 默认 Linear 插值、无延迟、播放一次。
    pub fn of_float(from: f32, to: f32) -> Self {
        Self {
            from,
            to,
            duration: 300,
            start_delay: 0,
            repeat_count: 0,
            repeat_mode: RepeatMode::Restart,
            interpolator: Box::new(interpolator::Linear),
            phase: Phase::Idle,
            start_time: None,
            cycles_done: 0,
            reversed: false,
            on_update: None,
        }
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

    /// 设置重复次数与模式。
    pub fn set_repeat(&mut self, count: u32, mode: RepeatMode) -> &mut Self {
        self.repeat_count = count;
        self.repeat_mode = mode;
        self
    }

    /// 设置插值器（默认 Linear）。
    pub fn set_interpolator(&mut self, i: impl Interpolator + Send + 'static) -> &mut Self {
        self.interpolator = Box::new(i);
        self
    }

    /// 设置每帧更新回调（值 = 插值后输出）。
    pub fn set_on_update(&mut self, f: impl FnMut(f32) + Send + 'static) -> &mut Self {
        self.on_update = Some(Box::new(f));
        self
    }

    /// 是否仍在活动（未结束/未取消）。
    pub fn is_running(&self) -> bool {
        matches!(self.phase, Phase::Delaying | Phase::Running)
    }

    /// 是否已终止（自然结束或取消）。
    pub fn is_ended(&self) -> bool {
        matches!(self.phase, Phase::Ended | Phase::Cancelled)
    }

    /// 帧推进：传入当前帧时间（ms）。
    ///
    /// # 返回
    /// true = 本帧产生了更新（回调已触发，需要重绘）；
    /// 结束帧额外触发一次终值回调（与 Android endFrame 语义一致）。
    pub fn advance(&mut self, now_ms: u64) -> bool {
        // 终态后不再推进（Ended/Cancelled 幂等）。
        if self.is_ended() {
            return false;
        }
        // 首帧锚定起始时间（含 delay）。
        let start = *self
            .start_time
            .get_or_insert(now_ms + self.start_delay as u64);
        // 时间进度。
        let elapsed = now_ms.saturating_sub(start);
        if self.phase == Phase::Idle {
            self.phase = Phase::Delaying;
        }
        if now_ms < start {
            return false; // 延迟等待中。
        }
        self.phase = Phase::Running;

        let dur = self.duration as u64;
        // 完成整轮数与轮内进度。
        let cycle = elapsed / dur;
        let mut frac = (elapsed % dur) as f32 / dur as f32;

        // 重复次数判定：cycle > repeat_count 即最终一轮结束。
        let total_rounds = self.repeat_count + 1;
        if cycle >= total_rounds as u64 {
            // 自然结束：钳制到终值（含插值终值恒等）。
            let done_frac = if self.reversed {
                // 反向轮结束在起点。
                0.0f32
            } else {
                1.0
            };
            let value = self.lerp(done_frac);
            if let Some(cb) = self.on_update.as_mut() {
                cb(value);
            }
            self.phase = Phase::Ended;
            return true;
        }

        // Reverse 模式：奇数轮反向。
        self.reversed = match self.repeat_mode {
            RepeatMode::Restart => false,
            RepeatMode::Reverse => cycle % 2 == 1,
        };
        if self.reversed {
            frac = 1.0 - frac;
        }
        self.cycles_done = cycle as u32;

        let value = self.lerp(frac);
        if let Some(cb) = self.on_update.as_mut() {
            cb(value);
        }
        true
    }

    /// 插值：线性区间 + 时间曲线整形。
    fn lerp(&self, frac: f32) -> f32 {
        let shaped = self.interpolator.get_interpolation(frac);
        self.from + (self.to - self.from) * shaped
    }

    /// 取消：立即停发回调（语义：不保证到终值，与 Android cancel 一致）。
    pub fn cancel(&mut self) {
        if self.is_running() || self.phase == Phase::Idle {
            self.phase = Phase::Cancelled;
        }
    }

    /// 结束：跳到终值并触发一次回调。
    pub fn end(&mut self) {
        if self.is_running() || self.phase == Phase::Idle {
            let value = if self.reversed { self.from } else { self.to };
            if let Some(cb) = self.on_update.as_mut() {
                cb(value);
            }
            self.phase = Phase::Ended;
        }
    }
}

/// 可动画的 View 数值属性（ObjectAnimator 绑定目标）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Property {
    /// x 平移。
    TranslationX,
    /// y 平移。
    TranslationY,
    /// 透明度 [0,1]。
    Alpha,
    /// x 缩放。
    ScaleX,
    /// y 缩放。
    ScaleY,
    /// 旋转（度）。
    Rotation,
}

/// View 属性动画（Android `ObjectAnimator` 子集）：
/// ValueAnimator 状态机 + 属性写入器。
pub struct ObjectAnimator {
    /// 状态机。
    anim: ValueAnimator,
    /// 目标属性。
    property: Property,
}

impl ObjectAnimator {
    /// 创建：目标属性 + 区间。
    pub fn of_view(property: Property, from: f32, to: f32) -> Self {
        Self {
            anim: ValueAnimator::of_float(from, to),
            property,
        }
    }

    /// 设置时长（ms）。
    pub fn set_duration(&mut self, ms: u32) -> &mut Self {
        self.anim.set_duration(ms);
        self
    }

    /// 设置起始延迟（ms）。
    pub fn set_start_delay(&mut self, ms: u32) -> &mut Self {
        self.anim.set_start_delay(ms);
        self
    }

    /// 设置重复次数与模式。
    pub fn set_repeat(&mut self, count: u32, mode: RepeatMode) -> &mut Self {
        self.anim.set_repeat(count, mode);
        self
    }

    /// 设置插值器。
    pub fn set_interpolator(&mut self, i: impl Interpolator + Send + 'static) -> &mut Self {
        self.anim.set_interpolator(i);
        self
    }

    /// 帧推进：计算插值并写入 View 属性。
    ///
    /// # 返回
    /// true = 本帧更新了属性（调用方应 invalidate）。
    pub fn advance(&mut self, view: &mut View, now_ms: u64) -> bool {
        // 借用拆分：状态机直写属性（View 借用无法跨 on_update 闭包
        // 长期持有，advance_to_value 与 advance 推进语义完全一致）。
        let property = self.property;
        let anim = &mut self.anim;
        anim.advance_to_value(now_ms, |v| match property {
            Property::TranslationX => view.set_translation_x(v),
            Property::TranslationY => view.set_translation_y(v),
            Property::Alpha => view.set_alpha(v),
            Property::ScaleX => view.set_scale_x(v),
            Property::ScaleY => view.set_scale_y(v),
            Property::Rotation => view.set_rotation(v),
        })
    }

    /// 是否已终止。
    pub fn is_ended(&self) -> bool {
        self.anim.is_ended()
    }

    /// 取消。
    pub fn cancel(&mut self) {
        self.anim.cancel();
    }

    /// 结束。
    pub fn end(&mut self) {
        self.anim.end();
    }
}

impl ValueAnimator {
    /// 内部：推进并直接写属性（ObjectAnimator 专用；进度语义与
    /// `advance` 完全一致，仅回调形式不同）。
    pub(crate) fn advance_to_value(&mut self, now_ms: u64, mut apply: impl FnMut(f32)) -> bool {
        // 终态后不再推进（与 advance 一致）。
        if self.is_ended() {
            return false;
        }
        let start = *self
            .start_time
            .get_or_insert(now_ms + self.start_delay as u64);
        if self.phase == Phase::Idle {
            self.phase = Phase::Delaying;
        }
        if now_ms < start {
            return false;
        }
        self.phase = Phase::Running;

        let dur = self.duration as u64;
        let cycle = now_ms.saturating_sub(start) / dur;
        let mut frac = ((now_ms - start) % dur) as f32 / dur as f32;

        let total_rounds = self.repeat_count + 1;
        if cycle >= total_rounds as u64 {
            let done_frac = if self.reversed { 0.0 } else { 1.0 };
            apply(self.lerp(done_frac));
            self.phase = Phase::Ended;
            return true;
        }

        self.reversed = match self.repeat_mode {
            RepeatMode::Restart => false,
            RepeatMode::Reverse => cycle % 2 == 1,
        };
        if self.reversed {
            frac = 1.0 - frac;
        }
        self.cycles_done = cycle as u32;
        apply(self.lerp(frac));
        true
    }
}

/// 插值器 trait 重导出（供调用方自定义实现，见 `anim::interpolator`）。
pub use crate::anim::interpolator::Interpolator;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::view::View;

    /// 基础推进：300ms 线性区间 0→100。
    #[test]
    fn linear_advance() {
        let mut a = ValueAnimator::of_float(0.0, 100.0);
        a.set_duration(100);
        let last = std::sync::Arc::new(std::sync::Mutex::new(f32::NAN));
        a.set_on_update({
            let last = std::sync::Arc::clone(&last);
            move |v| {
                *last.lock().unwrap() = v;
            }
        });
        assert!(a.advance(0));
        assert_eq!(*last.lock().unwrap(), 0.0);
        assert!(a.advance(50));
        assert_eq!(*last.lock().unwrap(), 50.0);
        assert!(a.advance(100));
        assert_eq!(*last.lock().unwrap(), 100.0);
        assert!(a.is_ended());
        // 结束后再推进不再回调。
        assert!(!a.advance(200));
    }

    /// 起始延迟：延迟期内不产生更新。
    #[test]
    fn start_delay_holds() {
        let mut a = ValueAnimator::of_float(0.0, 10.0);
        a.set_duration(100);
        a.set_start_delay(50);
        assert!(!a.advance(0), "within delay: no update");
        assert!(!a.advance(49));
        assert!(a.advance(50), "delay elapsed");
    }

    /// 重复 Restart：两轮从起点重来。
    #[test]
    fn repeat_restart() {
        let mut a = ValueAnimator::of_float(0.0, 10.0);
        a.set_duration(100);
        a.set_repeat(1, RepeatMode::Restart);
        let last = std::sync::Arc::new(std::sync::Mutex::new(f32::NAN));
        a.set_on_update({
            let last = std::sync::Arc::clone(&last);
            move |v| {
                *last.lock().unwrap() = v;
            }
        });
        a.advance(0);
        a.advance(150); // 第二轮一半。
        assert_eq!(*last.lock().unwrap(), 5.0);
        assert!(a.advance(200));
        assert_eq!(*last.lock().unwrap(), 10.0);
        assert!(a.is_ended());
    }

    /// 重复 Reverse：第二轮反向（终点→起点）。
    #[test]
    fn repeat_reverse() {
        let mut a = ValueAnimator::of_float(0.0, 10.0);
        a.set_duration(100);
        a.set_repeat(1, RepeatMode::Reverse);
        let last = std::sync::Arc::new(std::sync::Mutex::new(f32::NAN));
        a.set_on_update({
            let last = std::sync::Arc::clone(&last);
            move |v| {
                *last.lock().unwrap() = v;
            }
        });
        a.advance(0);
        a.advance(150); // 反向轮一半：从 10 回到 5。
        assert_eq!(*last.lock().unwrap(), 5.0);
        a.advance(200); // 反向轮结束：回到 0。
        assert_eq!(*last.lock().unwrap(), 0.0);
        assert!(a.is_ended());
    }

    /// cancel：立即停止，不再产生更新。
    #[test]
    fn cancel_stops_updates() {
        let mut a = ValueAnimator::of_float(0.0, 10.0);
        a.set_duration(100);
        let count = std::sync::Arc::new(std::sync::Mutex::new(0));
        a.set_on_update({
            let count = std::sync::Arc::clone(&count);
            move |_| {
                *count.lock().unwrap() += 1;
            }
        });
        a.advance(0);
        assert_eq!(*count.lock().unwrap(), 1);
        a.cancel();
        assert!(!a.advance(50), "no update after cancel");
        assert!(!a.advance(100));
    }

    /// end：跳到终值触发一次。
    #[test]
    fn end_jumps_to_target() {
        let mut a = ValueAnimator::of_float(0.0, 10.0);
        a.set_duration(1000);
        let last = std::sync::Arc::new(std::sync::Mutex::new(f32::NAN));
        a.set_on_update({
            let last = std::sync::Arc::clone(&last);
            move |v| {
                *last.lock().unwrap() = v;
            }
        });
        a.advance(0);
        a.end();
        assert_eq!(*last.lock().unwrap(), 10.0);
        assert!(a.is_ended());
    }

    /// ObjectAnimator：写 View 属性。
    #[test]
    fn object_animator_writes_property() {
        let mut v = View::new();
        let mut a = ObjectAnimator::of_view(Property::TranslationX, 0.0, 100.0);
        a.set_duration(100);
        assert!(a.advance(&mut v, 0));
        assert_eq!(v.translation_x(), 0.0);
        assert!(a.advance(&mut v, 50));
        assert_eq!(v.translation_x(), 50.0);
        assert!(a.advance(&mut v, 100));
        assert_eq!(v.translation_x(), 100.0);
        assert!(a.is_ended());
    }

    /// 插值器生效：Accelerate 中点半程（t=0.5 → shaped=0.25）。
    #[test]
    fn interpolator_shapes_output() {
        let mut a = ValueAnimator::of_float(0.0, 100.0);
        a.set_duration(100);
        a.set_interpolator(interpolator::Accelerate::default());
        let last = std::sync::Arc::new(std::sync::Mutex::new(f32::NAN));
        a.set_on_update({
            let last = std::sync::Arc::clone(&last);
            move |v| {
                *last.lock().unwrap() = v;
            }
        });
        a.advance(0);
        a.advance(50);
        assert_eq!(*last.lock().unwrap(), 25.0, "t=0.5 with t^2 curve → 25%");
    }
}
