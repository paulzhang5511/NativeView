//! 控件 trait：measure/layout/draw 统一接口（Task 20）。
//!
//! # 设计动机
//! Task 16 的 widgets 是「平行结构」：各控件自有 measure/layout/draw
//! 方法但无共同接口，容器只持裸 `View`——内容控件（TextView 等）
//! 无法挂入容器树，也无法递归分发触摸。本 trait 收敛「一个可挂进
//! 容器的控件」所需的最小接口，容器持 `Box<dyn Widget>` 即可递归
//! measure/layout/draw/触摸分发。
//!
//! # 与 dispatch::Node 的关系
//! `Node` 是分发期的借用视图（零拷贝）；`Widget` 是所有权期的树节点。
//! 二者职责分离：装配用 `Widget`，分发用 `Node`（借 `view()` 的地址
//! 做手势注册键）。

use crate::gfx::scene::Scene;
use crate::ui::measure_spec::MeasureSpec;
use crate::ui::view::View;

/// 可挂入容器的控件接口。
pub trait Widget {
    /// 自身 View 几何（命中测试/布局产出）。
    fn view(&self) -> &View;

    /// 自身 View 几何可变访问（容器 measure 阶段改写测量尺寸用）。
    fn view_mut(&mut self) -> &mut View;

    /// 测量（对应 View.onMeasure）。
    fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec);

    /// 布局（对应 View.onLayout）。
    fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32);

    /// 绘制（背景 + 内容，对应 View.onDraw + dispatchDraw）。
    fn draw(&self, scene: &mut Scene);

    /// 触摸分发（对应 dispatchTouchEvent）。
    ///
    /// # 语义
    /// 容器先向子树递归（子项消费则整体消费），叶子再走
    /// 「可点击 → 手势状态机」；容器（拥有 `for_each_child_mut`）与
    /// 普通 View 因此共用同一默认实现。
    fn dispatch_touch(&mut self, event: &crate::ui::input::MotionEvent) -> bool {
        let mut consumed = false;
        self.for_each_child_mut(event, &mut |child, ev| {
            if child.dispatch_touch(ev) {
                consumed = true;
                true
            } else {
                false
            }
        });
        if consumed {
            return true;
        }
        if !self.view().is_clickable() {
            return false;
        }
        crate::ui::input::dispatch::gesture_registry::advance(self.view().unique_id(), event)
    }

    /// 容器语义：逆序回调遍历子节点（对应 ViewGroup 的子分发；
    /// 非容器返回 false）。
    ///
    /// # 回调约定
    /// `f(child, event)` 返回 true 时提前终止（已消费）。
    /// 滚动坐标补偿由 ScrollView 覆写（子内容绘制平移的逆变换）。
    fn for_each_child_mut(
        &mut self,
        event: &crate::ui::input::MotionEvent,
        f: &mut dyn FnMut(&mut dyn Widget, &crate::ui::input::MotionEvent) -> bool,
    ) -> bool {
        let _ = (event, f);
        false
    }

    /// 推进内部动画/惯性一帧。
    ///
    /// # 返回
    /// true = 产生了视觉变化，需要重绘。
    fn advance(&mut self, _now_ms: i64) -> bool {
        false
    }

    /// 是否存在需要持续帧驱动的动画（惯性/回弹）。
    fn is_animating(&self) -> bool {
        false
    }

    /// Any 转换（Demo 侧按具体类型读回容器状态，如 ScrollView 滚动偏移）。
    fn as_any(&self) -> &dyn std::any::Any;
}
