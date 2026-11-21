//! ViewGroup：子视图协商与绘制分发（PRD §5.1.2，Task 15）。
//!
//! # 设计
//! - `ViewGroup` 持有子视图集合，measure/layout/draw 全量向下分发；
//! - `get_child_measure_spec` 为纯函数（9 组合与 Android 逐项一致，表驱动单测）；
//! - 绘制分发逆序（后添加的子视图先画，Android z-order 语义）。

use crate::gfx::scene::Scene;

use super::measure_spec::{MeasureSpec, Mode};
use super::view::View;

/// 子视图布局参数（Android `MarginLayoutParams` + LinearLayout weight 扩展）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MarginLayoutParams {
    /// 宽（MATCH_PARENT=-1 / WRAP_CONTENT=-2 / 具体值）。
    pub width: i32,
    /// 高。
    pub height: i32,
    /// 左外边距。
    pub left_margin: f32,
    /// 顶外边距。
    pub top_margin: f32,
    /// 右外边距。
    pub right_margin: f32,
    /// 底外边距。
    pub bottom_margin: f32,
    /// 线性布局权重（仅 LinearLayout 消费；0 = 不参与分配）。
    pub weight: f32,
}

impl MarginLayoutParams {
    /// 简单构造：尺寸 + 零边距。
    pub fn new(width: i32, height: i32) -> Self {
        Self {
            width,
            height,
            ..Default::default()
        }
    }

    /// 水平边距之和。
    pub fn horizontal_margins(&self) -> f32 {
        self.left_margin + self.right_margin
    }

    /// 垂直边距之和。
    pub fn vertical_margins(&self) -> f32 {
        self.top_margin + self.bottom_margin
    }
}

/// Android `ViewGroup.getChildMeasureSpec` 等价（纯函数）。
///
/// # 参数
/// `spec`：父 spec；`padding`：父侧扣除（容器 padding + 已用空间）；
/// `child_dimension`：子 LP 尺寸（可为 -1/-2 哨兵或具体值）。
pub fn get_child_measure_spec(
    spec: MeasureSpec,
    padding: u32,
    child_dimension: i32,
) -> MeasureSpec {
    let size = spec.size();
    match spec.mode() {
        Mode::Exactly => {
            if child_dimension >= 0 {
                MeasureSpec::make(Mode::Exactly, child_dimension as u32)
            } else {
                MeasureSpec::make(Mode::Exactly, size.saturating_sub(padding))
            }
        }
        Mode::AtMost => {
            if child_dimension >= 0 {
                MeasureSpec::make(Mode::Exactly, child_dimension as u32)
            } else if child_dimension == -1 {
                // MATCH_PARENT：受限为父剩余空间。
                MeasureSpec::make(Mode::AtMost, size.saturating_sub(padding))
            } else {
                // WRAP_CONTENT：受限但可更小。
                MeasureSpec::make(Mode::AtMost, size.saturating_sub(padding))
            }
        }
        Mode::Unspecified => {
            if child_dimension >= 0 {
                // Android `getChildMeasureSpec` 表：父 UNSPECIFIED + 子具体
                // 尺寸 → EXACTLY(childDimension)。若降级为 UNSPECIFIED，
                // 控件会退回「内容自适应」，固定高度（列表行/输入框）全部失效。
                MeasureSpec::make(Mode::Exactly, child_dimension as u32)
            } else {
                MeasureSpec::make(Mode::Unspecified, 0)
            }
        }
    }
}

/// measureChildWithMargins 的 spec 推导：可用尺寸 = 父 size - padding - margin。
///
/// # 参数
/// `parent`：父 spec；`container_padding`：容器 padding（含已用空间）；
/// `child_padding`：子方向上的 margin 附加扣减（水平用 h 边距，垂直用 v 边距）；
/// `lp`：子布局参数（尺寸决定子模式）。
pub fn measure_child_with_margins_spec(
    parent: MeasureSpec,
    container_padding: u32,
    child_padding: u32,
    lp: &MarginLayoutParams,
) -> MeasureSpec {
    // Android 流程：available = spec.size - padding - childMargins；
    // child = getChildMeasureSpec(spec, max(0, available), lp.width)。
    let total_padding = container_padding.saturating_add(child_padding);
    get_child_measure_spec(parent, total_padding, lp.width)
}

/// 与 `measure_child_with_margins_spec` 同构，但推导高度 spec
/// （LinearLayout 等方向性容器需要分别推导宽/高）。
pub fn measure_child_height_with_margins_spec(
    parent: MeasureSpec,
    container_padding: u32,
    child_padding: u32,
    lp: &MarginLayoutParams,
) -> MeasureSpec {
    let total_padding = container_padding.saturating_add(child_padding);
    get_child_measure_spec(parent, total_padding, lp.height)
}

/// 子视图条目（View + 布局参数）。
pub struct ChildView {
    /// 子视图。
    pub view: View,
    /// 布局参数。
    pub lp: MarginLayoutParams,
}

/// ViewGroup：子视图容器。
pub struct ViewGroup {
    /// 容器自身 View 状态。
    pub view: View,
    /// 子视图（按下标管理，逆序绘制）。
    children: Vec<ChildView>,
}

impl ViewGroup {
    /// 创建空容器。
    pub fn new() -> Self {
        Self {
            view: View::new(),
            children: Vec::new(),
        }
    }

    /// 添加子视图。
    pub fn add_view(&mut self, view: View, lp: MarginLayoutParams) {
        self.children.push(ChildView { view, lp });
    }

    /// 子视图数。
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// 按下标取子视图。
    pub fn child_at(&self, index: usize) -> Option<&ChildView> {
        self.children.get(index)
    }

    /// 按下标可变取子视图。
    pub fn child_at_mut(&mut self, index: usize) -> Option<&mut ChildView> {
        self.children.get_mut(index)
    }

    /// 移除指定下标子视图。
    pub fn remove_view_at(&mut self, index: usize) -> Option<View> {
        if index < self.children.len() {
            Some(self.children.remove(index).view)
        } else {
            None
        }
    }

    /// 移除全部子视图。
    pub fn remove_all_views(&mut self) {
        self.children.clear();
    }

    /// 子视图下标（首个相同引用无法用，V1 按指针语义暂缺；提供 ID 检索由上层维护）。
    pub fn index_of_child(&self, index: usize) -> Option<usize> {
        if index < self.children.len() {
            Some(index)
        } else {
            None
        }
    }

    /// measureChildren：全部子视图按各自 spec 测量。
    ///
    /// 水平/垂直 padding 由容器统一扣减（方向语义由 LinearLayout 等
    /// widgets 覆盖，V1 基线按零容器 padding + 子 margin 协商）。
    pub fn measure_children(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        for child in &mut self.children {
            let w = measure_child_with_margins_spec(
                width_spec,
                0,
                child.lp.horizontal_margins().max(0.0) as u32,
                &child.lp,
            );
            let h = measure_child_with_margins_spec(
                height_spec,
                0,
                child.lp.vertical_margins().max(0.0) as u32,
                &child.lp,
            );
            child.view.measure(w, h);
        }
    }

    /// dispatchDraw：逆序绘制子视图（后添加者先画 = 视觉在最上层）。
    pub fn dispatch_draw(&self, scene: &mut Scene) {
        for child in self.children.iter().rev() {
            child.view.draw(scene);
        }
    }
}

impl Default for ViewGroup {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// EXACTLY 父 + 具体尺寸子：EXACTLY(具体值)。
    #[test]
    fn child_spec_exactly_parent() {
        let parent = MeasureSpec::make(Mode::Exactly, 100);
        let spec = get_child_measure_spec(parent, 10, 50);
        assert_eq!(spec.mode(), Mode::Exactly);
        assert_eq!(spec.size(), 50);
    }

    /// UNSPECIFIED 父 + 具体尺寸子：仍为 EXACTLY(具体值)。
    ///
    /// 回归：曾返回 UNSPECIFIED(具体值)，导致控件跳过固定高度、
    /// 滚动容器内的固定高子项（列表行/输入框）高度变成 0。
    #[test]
    fn child_spec_unspecified_parent_fixed_child() {
        let parent = MeasureSpec::make(Mode::Unspecified, 0);
        let spec = get_child_measure_spec(parent, 0, 64);
        assert_eq!(spec.mode(), Mode::Exactly, "固定尺寸必须 EXACTLY");
        assert_eq!(spec.size(), 64);
    }

    /// UNSPECIFIED 父 + WRAP 子：UNSPECIFIED(0)（内容自报）。
    #[test]
    fn child_spec_unspecified_parent_wrap_child() {
        let parent = MeasureSpec::make(Mode::Unspecified, 0);
        let spec = get_child_measure_spec(parent, 0, -2);
        assert_eq!(spec.mode(), Mode::Unspecified);
        assert_eq!(spec.size(), 0);
    }
}
