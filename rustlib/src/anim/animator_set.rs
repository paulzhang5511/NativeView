//! 动画组合（Android `AnimatorSet` 子集）：顺序 / 并行编排多个动画。
//!
//! # 设计
//! - 帧驱动：`advance(now_ms)` 由主循环帧回调调用（与 `ValueAnimator` 一致）；
//! - 两种编排模式：
//!   - `Sequential`：前一个结束后下一个才开始（Android `playSequentially`）；
//!   - `Parallel`：所有动画同时开始（Android `playTogether`）；
//! - 子动画为 `Box<dyn Animatable>`（trait 对象），支持嵌套组合
//!   （AnimatorSet 内嵌 AnimatorSet）；
//! - 取消/结束语义：cancel 传播到所有子动画；end 跳到各子动画终值。

use crate::anim::animator::ValueAnimator;

/// 可推进动画的统一接口（AnimatorSet 子项抽象）。
///
/// # 设计
/// 与 `ValueAnimator`/`ObjectAnimator` 的 `advance` 语义对齐：
/// 传入帧时间，返回本帧是否产生更新（调用方据此决定是否 invalidate）。
pub trait Animatable: Send {
    /// 帧推进。
    ///
    /// # 返回
    /// true = 本帧产生了更新（需要重绘）。
    fn advance(&mut self, now_ms: u64) -> bool;

    /// 是否仍在活动。
    fn is_running(&self) -> bool;

    /// 是否已终止。
    fn is_ended(&self) -> bool;

    /// 取消（传播到子项）。
    fn cancel(&mut self);

    /// 结束（跳到终值）。
    fn end(&mut self);
}

impl Animatable for ValueAnimator {
    fn advance(&mut self, now_ms: u64) -> bool {
        ValueAnimator::advance(self, now_ms)
    }
    fn is_running(&self) -> bool {
        ValueAnimator::is_running(self)
    }
    fn is_ended(&self) -> bool {
        ValueAnimator::is_ended(self)
    }
    fn cancel(&mut self) {
        ValueAnimator::cancel(self);
    }
    fn end(&mut self) {
        ValueAnimator::end(self);
    }
}

/// 编排模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    /// 顺序播放（前一个结束后下一个开始）。
    Sequential,
    /// 并行播放（同时开始）。
    Parallel,
}

/// 动画组合器（Android `AnimatorSet` 子集）。
///
/// # 时间基准
/// 毫秒；`advance` 传入当前帧时间。Sequential 模式内部维护"当前激活索引"，
/// 前一个子动画 `is_ended()` 后切换到下一个。
pub struct AnimatorSet {
    /// 子动画列表。
    children: Vec<Box<dyn Animatable>>,
    /// 编排模式。
    order: Order,
    /// Sequential 模式：当前激活的子动画索引。
    active_index: usize,
    /// 整体是否已取消。
    cancelled: bool,
    /// 整体是否已结束。
    ended: bool,
}

impl AnimatorSet {
    /// 创建空组合器（后续 `add` 子动画）。
    pub fn new(order: Order) -> Self {
        Self {
            children: Vec::new(),
            order,
            active_index: 0,
            cancelled: false,
            ended: false,
        }
    }

    /// 顺序播放构造器（Android `playSequentially` 语义）。
    pub fn sequential() -> Self {
        Self::new(Order::Sequential)
    }

    /// 并行播放构造器（Android `playTogether` 语义）。
    pub fn parallel() -> Self {
        Self::new(Order::Parallel)
    }

    /// 添加子动画。
    pub fn add(&mut self, anim: impl Animatable + 'static) -> &mut Self {
        self.children.push(Box::new(anim));
        self
    }

    /// 子动画数量。
    pub fn len(&self) -> usize {
        self.children.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    /// 帧推进。
    ///
    /// # 返回
    /// true = 本帧有子动画产生更新（调用方应 invalidate）。
    pub fn advance(&mut self, now_ms: u64) -> bool {
        if self.cancelled || self.ended || self.children.is_empty() {
            return false;
        }
        let mut updated = false;
        match self.order {
            Order::Parallel => {
                // 所有子动画同时推进。
                for child in self.children.iter_mut() {
                    if child.advance(now_ms) {
                        updated = true;
                    }
                }
                // 全部结束 → 整体结束。
                if self.children.iter().all(|c| c.is_ended()) {
                    self.ended = true;
                }
            }
            Order::Sequential => {
                // 仅推进当前激活的子动画。
                while self.active_index < self.children.len() {
                    let child = &mut self.children[self.active_index];
                    if child.advance(now_ms) {
                        updated = true;
                    }
                    if child.is_ended() {
                        self.active_index += 1;
                    } else {
                        break; // 当前子动画未结束，等待下一帧。
                    }
                }
                if self.active_index >= self.children.len() {
                    self.ended = true;
                }
            }
        }
        updated
    }

    /// 是否仍在活动。
    pub fn is_running(&self) -> bool {
        !self.cancelled && !self.ended && !self.children.is_empty()
    }

    /// 是否已终止。
    pub fn is_ended(&self) -> bool {
        self.ended || self.cancelled
    }

    /// 取消：传播到所有子动画。
    pub fn cancel(&mut self) {
        if self.cancelled {
            return;
        }
        self.cancelled = true;
        for child in self.children.iter_mut() {
            child.cancel();
        }
    }

    /// 结束：所有子动画跳到终值。
    pub fn end(&mut self) {
        if self.ended {
            return;
        }
        self.ended = true;
        for child in self.children.iter_mut() {
            child.end();
        }
    }
}

impl Animatable for AnimatorSet {
    fn advance(&mut self, now_ms: u64) -> bool {
        AnimatorSet::advance(self, now_ms)
    }
    fn is_running(&self) -> bool {
        AnimatorSet::is_running(self)
    }
    fn is_ended(&self) -> bool {
        AnimatorSet::is_ended(self)
    }
    fn cancel(&mut self) {
        AnimatorSet::cancel(self);
    }
    fn end(&mut self) {
        AnimatorSet::end(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::animator::ValueAnimator;
    use crate::anim::interpolator::Linear;

    /// 并行：两个动画同时推进，全部结束后整体结束。
    #[test]
    fn parallel_both_advance() {
        let mut a1 = ValueAnimator::of_float(0.0, 100.0);
        a1.set_duration(100);
        let mut a2 = ValueAnimator::of_float(0.0, 50.0);
        a2.set_duration(200);

        let mut set = AnimatorSet::parallel();
        set.add(a1);
        set.add(a2);

        assert!(set.advance(0));
        assert!(!set.is_ended(), "a2 still running at t=0");
        assert!(set.advance(100));
        assert!(!set.is_ended(), "a2 still running at t=100");
        assert!(set.advance(200));
        assert!(set.is_ended(), "both done at t=200");
    }

    /// 顺序：第一个结束后第二个才开始。
    #[test]
    fn sequential_second_waits() {
        let mut a1 = ValueAnimator::of_float(0.0, 100.0);
        a1.set_duration(100);
        let mut a2 = ValueAnimator::of_float(0.0, 50.0);
        a2.set_duration(100);

        let mut set = AnimatorSet::sequential();
        set.add(a1);
        set.add(a2);

        // t=0..100: 只有 a1 在跑。
        assert!(set.advance(0));
        assert!(set.advance(50));
        assert!(set.advance(100));
        // t=100: a1 结束，a2 开始。
        assert!(!set.is_ended(), "a2 just started");
        // t=150: a2 一半。
        assert!(set.advance(150));
        // t=200: a2 结束。
        assert!(set.advance(200));
        assert!(set.is_ended());
    }

    /// 取消传播：cancel 后所有子动画停止。
    #[test]
    fn cancel_propagates() {
        let mut a1 = ValueAnimator::of_float(0.0, 100.0);
        a1.set_duration(1000);
        let mut a2 = ValueAnimator::of_float(0.0, 50.0);
        a2.set_duration(1000);

        let mut set = AnimatorSet::parallel();
        set.add(a1);
        set.add(a2);

        assert!(set.advance(0));
        set.cancel();
        assert!(set.is_ended());
        assert!(!set.advance(100), "no update after cancel");
    }

    /// 嵌套：Sequential 内嵌 Parallel。
    #[test]
    fn nested_sequential_parallel() {
        // 内层：两个 100ms 动画并行。
        let mut inner_a = ValueAnimator::of_float(0.0, 10.0);
        inner_a.set_duration(100);
        let mut inner_b = ValueAnimator::of_float(0.0, 20.0);
        inner_b.set_duration(100);
        let mut inner = AnimatorSet::parallel();
        inner.add(inner_a);
        inner.add(inner_b);

        // 外层：内层 + 一个 100ms 动画顺序。
        let mut outer = ValueAnimator::of_float(0.0, 30.0);
        outer.set_duration(100);

        let mut set = AnimatorSet::sequential();
        set.add(inner);
        set.add(outer);

        // t=0..100: 内层并行。
        assert!(set.advance(0));
        assert!(set.advance(100));
        assert!(!set.is_ended(), "outer just started");
        // t=100..200: 外层。
        assert!(set.advance(200));
        assert!(set.is_ended());
    }

    /// 空组合器：advance 返回 false。
    #[test]
    fn empty_set_noop() {
        let mut set = AnimatorSet::parallel();
        assert!(!set.advance(0));
        assert!(set.is_empty());
    }
}
