//! 关键帧动画（Android `KeyframeAnimator` 子集）：多段值插值。
//!
//! # 设计
//! - 关键帧序列：每个 `Keyframe` 携带目标值 + 帧内时间偏移（ms）+ 可选插值器；
//! - 帧驱动：`advance(now_ms)` 定位当前帧段，段内线性/插值器插值；
//! - 与 `ValueAnimator`（单区间）互补：关键帧支持任意多段非线性轨迹
//!   （如"0→50→30→100"的弹性路径）；
//! - 值类型：f32（与 View 属性一致）；颜色/整数经 `evaluator` 模块单独处理。

use crate::anim::interpolator::{Interpolator, Linear};

/// 单个关键帧：目标值 + 时间偏移 + 段插值器。
///
/// # 时间语义
/// `time_ms` 是相对动画起点的绝对偏移（非段内相对）；
/// 帧段 = [本帧 time_ms, 下一帧 time_ms)，段内进度 = (now - 本帧) / (下帧 - 本帧)。
pub struct Keyframe {
    /// 目标值。
    pub value: f32,
    /// 相对起点的绝对时间偏移（ms）。
    pub time_ms: u32,
    /// 本段（从本帧到下一帧）使用的插值器。
    pub interpolator: Box<dyn Interpolator + Send>,
}

impl Keyframe {
    /// 创建关键帧（默认 Linear 段插值）。
    pub fn new(value: f32, time_ms: u32) -> Self {
        Self {
            value,
            time_ms,
            interpolator: Box::new(Linear),
        }
    }

    /// 设置本段插值器。
    pub fn with_interpolator(mut self, i: impl Interpolator + Send + 'static) -> Self {
        self.interpolator = Box::new(i);
        self
    }
}

/// 关键帧动画：多段值插值状态机。
///
/// # 不变量
/// - 关键帧必须按 `time_ms` 升序排列（构造时 debug_assert）；
/// - 至少 2 个关键帧（首帧 + 末帧）；
/// - `advance` 在末帧时间后钳制到末值并置 Ended。
pub struct KeyframeAnimator {
    /// 关键帧序列（升序）。
    frames: Vec<Keyframe>,
    /// 动画起始时间（首帧锚定）。
    start_time: Option<u64>,
    /// 是否已结束。
    ended: bool,
    /// 是否已取消。
    cancelled: bool,
    /// 每帧更新回调。
    on_update: Option<Box<dyn FnMut(f32) + Send>>,
}

impl KeyframeAnimator {
    /// 创建：关键帧序列（必须 ≥2 且 time_ms 升序）。
    ///
    /// # 边界
    /// 少于 2 帧或时间非升序 → debug_assert（release 下取首末帧退化）。
    pub fn new(frames: Vec<Keyframe>) -> Self {
        debug_assert!(
            frames.len() >= 2,
            "KeyframeAnimator requires at least 2 keyframes"
        );
        for pair in frames.windows(2) {
            debug_assert!(
                pair[0].time_ms <= pair[1].time_ms,
                "keyframes must be in ascending time_ms order"
            );
        }
        Self {
            frames,
            start_time: None,
            ended: false,
            cancelled: false,
            on_update: None,
        }
    }

    /// 设置每帧更新回调。
    pub fn set_on_update(&mut self, f: impl FnMut(f32) + Send + 'static) -> &mut Self {
        self.on_update = Some(Box::new(f));
        self
    }

    /// 总时长（ms）= 末帧 time_ms。
    pub fn duration(&self) -> u32 {
        self.frames.last().map(|f| f.time_ms).unwrap_or(0)
    }

    /// 是否仍在活动。
    pub fn is_running(&self) -> bool {
        !self.ended && !self.cancelled
    }

    /// 是否已终止。
    pub fn is_ended(&self) -> bool {
        self.ended || self.cancelled
    }

    /// 帧推进：定位当前帧段并插值。
    ///
    /// # 返回
    /// true = 本帧产生了更新（回调已触发）。
    pub fn advance(&mut self, now_ms: u64) -> bool {
        if self.ended || self.cancelled || self.frames.len() < 2 {
            return false;
        }
        let start = *self.start_time.get_or_insert(now_ms);
        let elapsed = now_ms.saturating_sub(start);
        let total = self.duration() as u64;

        if elapsed >= total {
            // 到达末帧：钳制到末值并结束。
            let end_value = self.frames.last().unwrap().value;
            if let Some(cb) = self.on_update.as_mut() {
                cb(end_value);
            }
            self.ended = true;
            return true;
        }

        // 定位当前帧段：找到 frames[i].time_ms <= elapsed < frames[i+1].time_ms。
        let elapsed_ms = elapsed as u32;
        let mut seg = 0;
        for i in 0..self.frames.len() - 1 {
            if self.frames[i].time_ms <= elapsed_ms && elapsed_ms < self.frames[i + 1].time_ms {
                seg = i;
                break;
            }
            if i == self.frames.len() - 2 {
                seg = i; // 兜底：末段。
            }
        }

        let from = &self.frames[seg];
        let to = &self.frames[seg + 1];
        let seg_start = from.time_ms as u64;
        let seg_end = to.time_ms as u64;
        let seg_dur = (seg_end - seg_start).max(1);
        let frac = ((elapsed - seg_start) as f32 / seg_dur as f32).min(1.0);
        let shaped = from.interpolator.get_interpolation(frac);
        let value = from.value + (to.value - from.value) * shaped;

        if let Some(cb) = self.on_update.as_mut() {
            cb(value);
        }
        true
    }

    /// 取消。
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    /// 结束：跳到末值。
    pub fn end(&mut self) {
        if self.ended || self.cancelled {
            return;
        }
        let end_value = self.frames.last().map(|f| f.value).unwrap_or(0.0);
        if let Some(cb) = self.on_update.as_mut() {
            cb(end_value);
        }
        self.ended = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::interpolator::Accelerate;

    /// 两段线性：0→50 (0-100ms), 50→100 (100-200ms)。
    #[test]
    fn two_segment_linear() {
        let frames = vec![
            Keyframe::new(0.0, 0),
            Keyframe::new(50.0, 100),
            Keyframe::new(100.0, 200),
        ];
        let mut anim = KeyframeAnimator::new(frames);
        let last = std::sync::Arc::new(std::sync::Mutex::new(f32::NAN));
        anim.set_on_update({
            let last = std::sync::Arc::clone(&last);
            move |v| {
                *last.lock().unwrap() = v;
            }
        });

        assert!(anim.advance(0));
        assert_eq!(*last.lock().unwrap(), 0.0);
        assert!(anim.advance(50));
        assert_eq!(*last.lock().unwrap(), 25.0, "midpoint of seg1");
        assert!(anim.advance(100));
        assert_eq!(*last.lock().unwrap(), 50.0, "seg1 end = seg2 start");
        assert!(anim.advance(150));
        assert_eq!(*last.lock().unwrap(), 75.0, "midpoint of seg2");
        assert!(anim.advance(200));
        assert_eq!(*last.lock().unwrap(), 100.0);
        assert!(anim.is_ended());
    }

    /// 非单调轨迹：0→80→30→100（先升后降再升）。
    #[test]
    fn non_monotonic_path() {
        let frames = vec![
            Keyframe::new(0.0, 0),
            Keyframe::new(80.0, 100),
            Keyframe::new(30.0, 200),
            Keyframe::new(100.0, 300),
        ];
        let mut anim = KeyframeAnimator::new(frames);
        let last = std::sync::Arc::new(std::sync::Mutex::new(f32::NAN));
        anim.set_on_update({
            let last = std::sync::Arc::clone(&last);
            move |v| {
                *last.lock().unwrap() = v;
            }
        });

        assert!(anim.advance(0)); // 锚定起始
        assert!(anim.advance(100));
        assert_eq!(*last.lock().unwrap(), 80.0);
        assert!(anim.advance(200));
        assert_eq!(*last.lock().unwrap(), 30.0, "dips down");
        assert!(anim.advance(300));
        assert_eq!(*last.lock().unwrap(), 100.0);
        assert!(anim.is_ended());
    }

    /// 段内插值器生效：Accelerate 段中点 = 25%。
    #[test]
    fn segment_interpolator() {
        let frames = vec![
            Keyframe::new(0.0, 0).with_interpolator(Accelerate::default()),
            Keyframe::new(100.0, 100),
        ];
        let mut anim = KeyframeAnimator::new(frames);
        let last = std::sync::Arc::new(std::sync::Mutex::new(f32::NAN));
        anim.set_on_update({
            let last = std::sync::Arc::clone(&last);
            move |v| {
                *last.lock().unwrap() = v;
            }
        });

        assert!(anim.advance(0)); // 锚定起始
        assert!(anim.advance(50));
        assert_eq!(*last.lock().unwrap(), 25.0, "t^2 at 0.5 = 0.25");
    }

    /// 超出总时长：钳制到末值。
    #[test]
    fn clamps_past_end() {
        let frames = vec![Keyframe::new(0.0, 0), Keyframe::new(10.0, 100)];
        let mut anim = KeyframeAnimator::new(frames);
        let last = std::sync::Arc::new(std::sync::Mutex::new(f32::NAN));
        anim.set_on_update({
            let last = std::sync::Arc::clone(&last);
            move |v| {
                *last.lock().unwrap() = v;
            }
        });

        assert!(anim.advance(0)); // 锚定起始
        assert!(anim.advance(500));
        assert_eq!(*last.lock().unwrap(), 10.0);
        assert!(anim.is_ended());
        assert!(!anim.advance(600), "no update after end");
    }

    /// 取消后不再更新。
    #[test]
    fn cancel_stops() {
        let frames = vec![Keyframe::new(0.0, 0), Keyframe::new(10.0, 100)];
        let mut anim = KeyframeAnimator::new(frames);
        let count = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        anim.set_on_update({
            let count = std::sync::Arc::clone(&count);
            move |_| {
                count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        });

        assert!(anim.advance(0));
        anim.cancel();
        assert!(!anim.advance(50));
        assert_eq!(count.load(std::sync::atomic::Ordering::Relaxed), 1);
    }
}
