//! 视图过渡动画（Android `Transition` 子集）：View 进入/退出场景的动画。
//!
//! # 设计
//! - `Transition` trait：定义 enter/exit 的初始/终态属性，由框架驱动
//!   `ViewPropertyAnimator` 完成插值；
//! - 内置过渡：`FadeTransition`（透明度）、`SlideTransition`（位移）、
//!   `ScaleTransition`（缩放）；
//! - `TransitionController`：管理 View 上的活跃过渡，帧循环中推进并
//!   在结束后清理；
//! - 与 `ViewPropertyAnimator` 组合：Transition 只描述"从哪到哪"，
//!   时间/插值器/监听器由调用方配置。
//!
//! # 与 AnimatorSet 的关系
//! Transition 是高层语义（"这个 View 要淡入"），内部展开为
//! ViewPropertyAnimator 的具体属性动画；复杂编排（先淡入再上滑）
//! 用 AnimatorSet 组合多个 Transition 产出的动画。

use crate::anim::view_animator::ViewPropertyAnimator;
use crate::ui::view::View;

/// 过渡方向（Slide 用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlideDirection {
    /// 从左侧滑入。
    Left,
    /// 从右侧滑入。
    Right,
    /// 从顶部滑入。
    Top,
    /// 从底部滑入。
    Bottom,
}

/// 视图过渡动画（Android `Transition` 子集）。
///
/// # 语义
/// - `enter`：View 从"不可见态"过渡到"可见态"（如 alpha 0→1）；
/// - `exit`：View 从"可见态"过渡到"不可见态"（如 alpha 1→0）；
/// - 调用方负责在 enter 前设置初始态、exit 结束后移除 View。
pub trait Transition: Send {
    /// 构建进入动画（初始态 → 可见态）。
    ///
    /// # 参数
    /// - `view`：目标视图（当前处于初始态）；
    /// - `animator`：调用方预配置的动画构建器（duration/interpolator 等）。
    fn build_enter(&self, view: &View, animator: &mut ViewPropertyAnimator);

    /// 构建退出动画（可见态 → 终态）。
    fn build_exit(&self, view: &View, animator: &mut ViewPropertyAnimator);
}

/// 淡入淡出过渡（Android `Fade`）。
///
/// # 行为
/// - enter：alpha 0 → 1；
/// - exit：alpha 1 → 0。
#[derive(Clone, Copy, Debug, Default)]
pub struct FadeTransition;

impl Transition for FadeTransition {
    fn build_enter(&self, _view: &View, animator: &mut ViewPropertyAnimator) {
        animator.alpha(1.0);
    }

    fn build_exit(&self, _view: &View, animator: &mut ViewPropertyAnimator) {
        animator.alpha(0.0);
    }
}

/// 滑动过渡（Android `Slide`）。
///
/// # 行为
/// - enter：从指定方向屏幕外滑入到原位（translation 偏移 → 0）；
/// - exit：从原位滑出到指定方向屏幕外（0 → 偏移）。
///
/// # 偏移量
/// 使用 View 自身尺寸作为偏移（滑出/入一个自身宽/高度），
/// 与 Android `Slide` 默认行为一致（不依赖屏幕尺寸）。
#[derive(Clone, Copy, Debug)]
pub struct SlideTransition {
    /// 滑动方向。
    pub direction: SlideDirection,
}

impl Default for SlideTransition {
    fn default() -> Self {
        Self {
            direction: SlideDirection::Right,
        }
    }
}

impl Transition for SlideTransition {
    fn build_enter(&self, _view: &View, animator: &mut ViewPropertyAnimator) {
        // enter 目标 = 原位（translation 0）；初始偏移由调用方在
        // start_enter 前写入 View（见 TransitionController 文档）。
        animator.translation_x(0.0).translation_y(0.0);
    }

    fn build_exit(&self, view: &View, animator: &mut ViewPropertyAnimator) {
        // exit 目标 = 方向外侧偏移（一个自身宽/高度）。
        let (dx, dy) = self.offset(view);
        animator.translation_x(dx).translation_y(dy);
    }
}

impl SlideTransition {
    /// 计算方向对应的偏移量（一个自身宽/高度）。
    pub fn offset(&self, view: &View) -> (f32, f32) {
        let w = view.width();
        let h = view.height();
        match self.direction {
            SlideDirection::Left => (-w, 0.0),
            SlideDirection::Right => (w, 0.0),
            SlideDirection::Top => (0.0, -h),
            SlideDirection::Bottom => (0.0, h),
        }
    }
}

/// 缩放过渡（Android `Scale`）。
///
/// # 行为
/// - enter：scale 0 → 1（从中心放大）；
/// - exit：scale 1 → 0（缩小消失）。
#[derive(Clone, Copy, Debug, Default)]
pub struct ScaleTransition;

impl Transition for ScaleTransition {
    fn build_enter(&self, _view: &View, animator: &mut ViewPropertyAnimator) {
        animator.scale_x(1.0).scale_y(1.0);
    }

    fn build_exit(&self, _view: &View, animator: &mut ViewPropertyAnimator) {
        animator.scale_x(0.0).scale_y(0.0);
    }
}

/// 过渡控制器：管理 View 上的活跃过渡动画。
///
/// # 用法
/// ```ignore
/// let mut controller = TransitionController::new();
///
/// // 启动淡入：
/// controller.start_enter(&mut view, &FadeTransition, 300);
///
/// // 帧循环中：
/// if controller.advance(&mut view, now_ms) {
///     // invalidate
/// }
/// ```
pub struct TransitionController {
    /// 当前活跃的进入/退出动画（None = 无活跃过渡）。
    active: Option<ViewPropertyAnimator>,
    /// 是否为退出过渡（exit 结束后调用方应移除 View）。
    is_exit: bool,
}

impl TransitionController {
    /// 创建空控制器。
    pub fn new() -> Self {
        Self {
            active: None,
            is_exit: false,
        }
    }

    /// 启动进入过渡。
    ///
    /// # 前置条件
    /// 调用方须先将 View 设为初始态（如 alpha=0），本方法只负责
    /// 构建并启动"初始态 → 可见态"的动画。
    pub fn start_enter(&mut self, view: &mut View, transition: &dyn Transition, duration_ms: u32) {
        let mut anim = ViewPropertyAnimator::new();
        anim.set_duration(duration_ms);
        transition.build_enter(view, &mut anim);
        self.active = Some(anim);
        self.is_exit = false;
    }

    /// 启动退出过渡。
    ///
    /// # 后置条件
    /// 动画结束后（`is_ended()` 为 true），调用方应移除/隐藏 View。
    pub fn start_exit(&mut self, view: &mut View, transition: &dyn Transition, duration_ms: u32) {
        let mut anim = ViewPropertyAnimator::new();
        anim.set_duration(duration_ms);
        transition.build_exit(view, &mut anim);
        self.active = Some(anim);
        self.is_exit = true;
    }

    /// 帧推进：推进活跃过渡动画。
    ///
    /// # 返回
    /// true = 本帧有更新（调用方应 invalidate）。
    pub fn advance(&mut self, view: &mut View, now_ms: u64) -> bool {
        match self.active.as_mut() {
            Some(anim) => anim.advance(view, now_ms),
            None => false,
        }
    }

    /// 是否有活跃过渡。
    pub fn is_active(&self) -> bool {
        self.active.as_ref().is_some_and(|a| a.is_running())
    }

    /// 过渡是否已结束（自然完成或取消）。
    pub fn is_ended(&self) -> bool {
        self.active.as_ref().is_some_and(|a| a.is_ended())
    }

    /// 是否为退出过渡（结束后调用方应移除 View）。
    pub fn was_exit(&self) -> bool {
        self.is_exit
    }

    /// 取消当前过渡。
    pub fn cancel(&mut self) {
        if let Some(anim) = self.active.as_mut() {
            anim.cancel();
        }
    }

    /// 结束当前过渡（跳到终值）。
    pub fn end(&mut self, view: &mut View) {
        if let Some(anim) = self.active.as_mut() {
            anim.end(view);
        }
    }

    /// 清除已完成过渡（释放资源，允许启动新过渡）。
    pub fn clear(&mut self) {
        self.active = None;
        self.is_exit = false;
    }
}

impl Default for TransitionController {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::interpolator::Linear;

    /// 淡入：alpha 0→1。
    #[test]
    fn fade_enter() {
        let mut v = View::new();
        v.set_alpha(0.0);
        let mut controller = TransitionController::new();
        controller.start_enter(&mut v, &FadeTransition, 100);

        // 手动设插值器为 Linear（controller 内部用默认 AccelerateDecelerate）。
        // 这里直接验证 advance 推进后 alpha 趋向 1。
        assert!(controller.advance(&mut v, 0));
        assert!(controller.advance(&mut v, 100));
        assert!(controller.is_ended());
        assert!(
            (v.alpha() - 1.0).abs() < f32::EPSILON,
            "alpha should be 1.0, got {}",
            v.alpha()
        );
        assert!(!controller.was_exit());
    }

    /// 淡出：alpha 1→0，结束后 was_exit=true。
    #[test]
    fn fade_exit() {
        let mut v = View::new();
        v.set_alpha(1.0);
        let mut controller = TransitionController::new();
        controller.start_exit(&mut v, &FadeTransition, 100);

        assert!(controller.advance(&mut v, 0));
        assert!(controller.advance(&mut v, 100));
        assert!(controller.is_ended());
        assert!((v.alpha() - 0.0).abs() < f32::EPSILON);
        assert!(controller.was_exit());
    }

    /// 滑动进入（从右侧）：translationX 从 w → 0。
    #[test]
    fn slide_enter_from_right() {
        let mut v = View::new();
        // 模拟已布局的 View（宽 200）。
        v.layout(0.0, 0.0, 200.0, 100.0);
        // 初始态：从右侧偏移。
        v.set_translation_x(200.0);

        let mut controller = TransitionController::new();
        let slide = SlideTransition {
            direction: SlideDirection::Right,
        };
        controller.start_enter(&mut v, &slide, 100);

        assert!(controller.advance(&mut v, 0));
        assert!(controller.advance(&mut v, 100));
        assert!(controller.is_ended());
        assert!(
            (v.translation_x() - 0.0).abs() < f32::EPSILON,
            "should slide to 0, got {}",
            v.translation_x()
        );
    }

    /// 滑动退出（向左）：translationX 从 0 → -w。
    #[test]
    fn slide_exit_to_left() {
        let mut v = View::new();
        v.layout(0.0, 0.0, 200.0, 100.0);
        v.set_translation_x(0.0);

        let mut controller = TransitionController::new();
        let slide = SlideTransition {
            direction: SlideDirection::Left,
        };
        controller.start_exit(&mut v, &slide, 100);

        assert!(controller.advance(&mut v, 0));
        assert!(controller.advance(&mut v, 100));
        assert!(controller.is_ended());
        assert!(
            (v.translation_x() - (-200.0)).abs() < f32::EPSILON,
            "should slide to -200, got {}",
            v.translation_x()
        );
    }

    /// 缩放进入：scale 0→1。
    #[test]
    fn scale_enter() {
        let mut v = View::new();
        v.set_scale_x(0.0);
        v.set_scale_y(0.0);

        let mut controller = TransitionController::new();
        controller.start_enter(&mut v, &ScaleTransition, 100);

        assert!(controller.advance(&mut v, 0));
        assert!(controller.advance(&mut v, 100));
        assert!(controller.is_ended());
        assert!((v.scale_x() - 1.0).abs() < f32::EPSILON);
        assert!((v.scale_y() - 1.0).abs() < f32::EPSILON);
    }

    /// 无活跃过渡：advance 返回 false。
    #[test]
    fn no_active_transition() {
        let mut v = View::new();
        let mut controller = TransitionController::new();
        assert!(!controller.is_active());
        assert!(!controller.advance(&mut v, 0));
    }

    /// 取消：过渡停止，View 属性不再变化。
    #[test]
    fn cancel_stops_transition() {
        let mut v = View::new();
        v.set_alpha(0.0);
        let mut controller = TransitionController::new();
        controller.start_enter(&mut v, &FadeTransition, 1000);

        assert!(controller.advance(&mut v, 0));
        let alpha_at_cancel = v.alpha();
        controller.cancel();
        assert!(controller.is_ended());
        // 取消后不再推进。
        assert!(!controller.advance(&mut v, 500));
        assert_eq!(v.alpha(), alpha_at_cancel);
    }

    /// clear 后允许启动新过渡。
    #[test]
    fn clear_allows_new_transition() {
        let mut v = View::new();
        v.set_alpha(0.0);
        let mut controller = TransitionController::new();
        controller.start_enter(&mut v, &FadeTransition, 100);
        assert!(controller.advance(&mut v, 0));
        controller.clear();
        assert!(!controller.is_active());
        // 可以启动新过渡。
        controller.start_exit(&mut v, &FadeTransition, 100);
        assert!(controller.is_active());
    }
}
