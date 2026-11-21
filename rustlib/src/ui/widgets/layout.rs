//! 内置布局容器：FrameLayout / LinearLayout（PRD §5.1.2/5.1.7，Task 16）。
//!
//! # 设计
//! - 容器持有自身 `View` 状态 + 子控件列表（复用 view_group 协商函数）；
//! - 子项为 `Box<dyn Widget>`（Task 20：内容控件与容器可组成完整树，
//!   measure/layout/draw/触摸全量递归）；裸 `View` 经 `ViewWidget`
//!   适配后挂入；
//! - onMeasure：FrameLayout 取子最大 WRAP 尺寸；LinearLayout 按方向求和
//!   （weight 分配：剩余空间按权重比例二次测量）；
//! - onLayout：Frame 按 gravity 定位；Linear 按方向顺序排布。

use crate::gfx::scene::Scene;
use crate::ui::measure_spec::{MeasureSpec, Mode};
use crate::ui::view::Gravity;
use crate::ui::view::View;
use crate::ui::view_group::{MarginLayoutParams, measure_child_with_margins_spec};
use crate::ui::widgets::widget::Widget;

/// 裸 View 适配器：把静态结构 View 包成可挂树的 `dyn Widget`。
pub struct ViewWidget(pub View);

impl Widget for ViewWidget {
    fn view(&self) -> &View {
        &self.0
    }
    fn view_mut(&mut self) -> &mut View {
        &mut self.0
    }
    fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        self.0.measure(width_spec, height_spec);
    }
    fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.0.layout(left, top, right, bottom);
    }
    fn draw(&self, scene: &mut Scene) {
        self.0.draw(scene);
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// 子控件条目（含 gravity 定位参数）。
pub struct PositionedChild {
    /// 子控件。
    pub child: Box<dyn Widget>,
    /// 布局参数。
    pub lp: MarginLayoutParams,
    /// 容器内对齐。
    pub gravity: Gravity,
}

/// 布局容器公共基座：子列表 + 分发。
pub struct WidgetContainer {
    /// 自身 View 状态。
    pub view: View,
    /// 子控件列表。
    pub children: Vec<PositionedChild>,
}

impl WidgetContainer {
    /// 空容器。
    pub fn new() -> Self {
        Self {
            view: View::new(),
            children: Vec::new(),
        }
    }

    /// 子控件数。
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// 下标访问。
    pub fn child_at(&self, index: usize) -> Option<&PositionedChild> {
        self.children.get(index)
    }

    /// 添加子控件。
    pub fn add_child(&mut self, child: Box<dyn Widget>, lp: MarginLayoutParams, gravity: Gravity) {
        self.children.push(PositionedChild { child, lp, gravity });
    }

    /// 添加裸 View 子项（静态结构块便捷入口）。
    pub fn add_view(&mut self, view: View, lp: MarginLayoutParams, gravity: Gravity) {
        self.add_child(Box::new(ViewWidget(view)), lp, gravity);
    }

    /// 逆序绘制子控件 + 容器背景。
    pub fn draw_all(&self, scene: &mut Scene) {
        self.view.draw(scene);
        for child in self.children.iter().rev() {
            // INVISIBLE 仍占位但不绘制；GONE 占位与绘制都跳过。
            if is_drawn(child) {
                child.child.draw(scene);
            }
        }
    }
}

impl Default for WidgetContainer {
    fn default() -> Self {
        Self::new()
    }
}

/// FrameLayout：全部子视图叠放在自身 bounds 内，按各自 gravity 定位。
pub struct FrameLayout {
    /// 容器基座。
    pub inner: WidgetContainer,
}

impl FrameLayout {
    /// 创建。
    pub fn new() -> Self {
        Self {
            inner: WidgetContainer::new(),
        }
    }

    /// 添加子视图。
    pub fn add_child(&mut self, view: View, lp: MarginLayoutParams, gravity: Gravity) {
        self.inner.add_view(view, lp, gravity);
    }

    /// 添加子控件（Task 20：内容控件直挂容器树）。
    pub fn add_widget(&mut self, child: Box<dyn Widget>, lp: MarginLayoutParams, gravity: Gravity) {
        self.inner.add_child(child, lp, gravity);
    }

    /// 子视图下标访问。
    pub fn child_at(&self, index: usize) -> Option<&PositionedChild> {
        self.inner.child_at(index)
    }

    /// 测量：EXACTLY 取 spec；否则取子视图 WRAP 最大值。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        let pad_x = pad_h(&self.inner.view);
        let pad_y = pad_v(&self.inner.view);
        let mut max_w = 0.0f32;
        let mut max_h = 0.0f32;
        for child in &mut self.inner.children {
            if is_gone(child) {
                // GONE：不测量（Android 语义：尺寸 0 且不占位）。
                child.child.measure(
                    MeasureSpec::make(Mode::Exactly, 0),
                    MeasureSpec::make(Mode::Exactly, 0),
                );
                continue;
            }
            let w = measure_child_with_margins_spec(
                width_spec,
                pad_x as u32,
                child.lp.horizontal_margins().max(0.0) as u32,
                &child.lp,
            );
            // 高度必须走 height 专用推导：width 版内部按 lp.width 取值，
            // 用它会忽略 lp.height（页签条这类固定高子项会被撑成满高）。
            let h = crate::ui::view_group::measure_child_height_with_margins_spec(
                height_spec,
                pad_y as u32,
                child.lp.vertical_margins().max(0.0) as u32,
                &child.lp,
            );
            child.child.measure(w, h);
            max_w = max_w.max(child.child.view().measured_width() + child.lp.horizontal_margins());
            max_h = max_h.max(child.child.view().measured_height() + child.lp.vertical_margins());
        }
        // 容器自身测量尺寸（WRAP 时补 padding）。
        let w = resolve(width_spec, max_w + pad_x);
        let h = resolve(height_spec, max_h + pad_y);
        self.inner.view.set_measured_dimension(w, h);
    }

    /// 布局：按 gravity 把每个子视图放进自身 bounds。
    pub fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.inner.view.layout(left, top, right, bottom);
        // 内容区 = 自身 bounds 去掉 padding（子项在内容区内按 gravity 摆放）。
        let content_left = left + self.inner.view.padding_left();
        let content_top = top + self.inner.view.padding_top();
        let cw = (right - left - pad_h(&self.inner.view)).max(0.0);
        let ch = (bottom - top - pad_v(&self.inner.view)).max(0.0);
        for child in &mut self.inner.children {
            if is_gone(child) {
                // GONE：塌缩到内容区左上角（尺寸 0，命中测试自然不命中）。
                child
                    .child
                    .layout(content_left, content_top, content_left, content_top);
                continue;
            }
            let w = child.child.view().measured_width();
            let h = child.child.view().measured_height();
            let (cx, cy) = gravity_offset(
                child.gravity,
                cw - w - child.lp.horizontal_margins(),
                ch - h - child.lp.vertical_margins(),
            );
            let x = content_left + cx + child.lp.left_margin.max(0.0);
            let y = content_top + cy + child.lp.top_margin.max(0.0);
            child.child.layout(x, y, x + w, y + h);
        }
    }

    /// 绘制。
    pub fn draw(&self, scene: &mut Scene) {
        self.inner.draw_all(scene);
    }
}

impl Default for FrameLayout {
    fn default() -> Self {
        Self::new()
    }
}

/// 排列方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    /// 水平。
    Horizontal,
    /// 垂直。
    Vertical,
}

/// LinearLayout：子视图按方向顺序排布，支持 weight 分配剩余空间。
pub struct LinearLayout {
    /// 容器基座。
    pub inner: WidgetContainer,
    /// 排列方向。
    pub orientation: Orientation,
}

impl LinearLayout {
    /// 垂直方向。
    pub fn vertical() -> Self {
        Self {
            inner: WidgetContainer::new(),
            orientation: Orientation::Vertical,
        }
    }

    /// 水平方向。
    pub fn horizontal() -> Self {
        Self {
            inner: WidgetContainer::new(),
            orientation: Orientation::Horizontal,
        }
    }

    /// 添加子视图。
    pub fn add_child(&mut self, view: View, lp: MarginLayoutParams, gravity: Gravity) {
        self.inner.add_view(view, lp, gravity);
    }

    /// 添加子控件（Task 20：内容控件直挂容器树）。
    pub fn add_widget(&mut self, child: Box<dyn Widget>, lp: MarginLayoutParams, gravity: Gravity) {
        self.inner.add_child(child, lp, gravity);
    }

    /// 子视图下标访问。
    pub fn child_at(&self, index: usize) -> Option<&PositionedChild> {
        self.inner.child_at(index)
    }

    /// 测量：按方向求和；weight 子视图平分剩余空间。
    ///
    /// # weight 语义（Android 对齐）
    /// 首遍主轴按 EXACTLY(0) 测量、交叉轴仍按正常 spec 测量——若连交叉轴
    /// 一起跳过测量，子项尺寸恒为 0，命中测试与绘制全部失效。
    /// 次遍按分得的主轴尺寸**重测**（文本需按新宽度重新折行）。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        // 第一遍：非 weight 子项按 LP 测量；weight 子项主轴清零。
        let mut used = 0.0f32;
        let mut total_weight = 0.0f32;
        let mut children = std::mem::take(&mut self.inner.children);
        for child in &mut children {
            let (w, h) = self.child_specs(width_spec, height_spec, child);
            if is_gone(child) {
                child.child.measure(
                    MeasureSpec::make(Mode::Exactly, 0),
                    MeasureSpec::make(Mode::Exactly, 0),
                );
                continue;
            }
            if child.lp.weight > 0.0 {
                total_weight += child.lp.weight;
                match self.orientation {
                    Orientation::Horizontal => {
                        child.child.measure(MeasureSpec::make(Mode::Exactly, 0), h);
                    }
                    Orientation::Vertical => {
                        child.child.measure(w, MeasureSpec::make(Mode::Exactly, 0));
                    }
                }
                continue;
            }
            child.child.measure(w, h);
            // 主轴占用含外边距（Android：margin 参与剩余空间扣减）。
            match self.orientation {
                Orientation::Horizontal => {
                    used += child.child.view().measured_width() + child.lp.horizontal_margins()
                }
                Orientation::Vertical => {
                    used += child.child.view().measured_height() + child.lp.vertical_margins()
                }
            }
        }
        // 容器可用主轴尺寸（自身尺寸减 padding）与剩余空间。
        let available = match self.orientation {
            Orientation::Horizontal => resolve(width_spec, used + pad_h(&self.inner.view)),
            Orientation::Vertical => resolve(height_spec, used + pad_v(&self.inner.view)),
        };
        let content_available = (available
            - match self.orientation {
                Orientation::Horizontal => pad_h(&self.inner.view),
                Orientation::Vertical => pad_v(&self.inner.view),
            })
        .max(0.0);
        let extra = (content_available - used).max(0.0);
        // 第二遍：weight 子项按占比份额重测（尺寸取整避免浮点尾差）。
        if total_weight > 0.0 {
            for child in &mut children {
                if child.lp.weight <= 0.0 {
                    continue;
                }
                let share = (extra * child.lp.weight / total_weight).floor() as u32;
                let (w, h) = self.child_specs(width_spec, height_spec, child);
                match self.orientation {
                    Orientation::Horizontal => {
                        child
                            .child
                            .measure(MeasureSpec::make(Mode::Exactly, share), h);
                    }
                    Orientation::Vertical => {
                        child
                            .child
                            .measure(w, MeasureSpec::make(Mode::Exactly, share));
                    }
                }
            }
        }
        self.inner.children = children;
        // 容器尺寸：方向求和（weight 后重算）。
        let mut total = 0.0f32;
        let mut cross = 0.0f32;
        for child in &self.inner.children {
            if is_gone(child) {
                continue;
            }
            match self.orientation {
                Orientation::Horizontal => {
                    total += child.child.view().measured_width() + child.lp.horizontal_margins();
                    cross = cross
                        .max(child.child.view().measured_height() + child.lp.vertical_margins());
                }
                Orientation::Vertical => {
                    total += child.child.view().measured_height() + child.lp.vertical_margins();
                    cross = cross
                        .max(child.child.view().measured_width() + child.lp.horizontal_margins());
                }
            }
        }
        match self.orientation {
            Orientation::Horizontal => {
                let h = resolve(height_spec, cross + pad_v(&self.inner.view));
                let w = resolve(width_spec, total + pad_h(&self.inner.view));
                self.inner.view.set_measured_dimension(w, h);
            }
            Orientation::Vertical => {
                let w = resolve(width_spec, cross + pad_h(&self.inner.view));
                let h = resolve(height_spec, total + pad_v(&self.inner.view));
                self.inner.view.set_measured_dimension(w, h);
            }
        }
    }

    /// 单个子视图的双向 spec（不含 weight 感知）。
    fn child_specs(
        &self,
        width_spec: MeasureSpec,
        height_spec: MeasureSpec,
        child: &PositionedChild,
    ) -> (MeasureSpec, MeasureSpec) {
        let w = measure_child_with_margins_spec(
            width_spec,
            pad_h(&self.inner.view) as u32,
            child.lp.horizontal_margins().max(0.0) as u32,
            &child.lp,
        );
        let h = crate::ui::view_group::measure_child_height_with_margins_spec(
            height_spec,
            pad_v(&self.inner.view) as u32,
            child.lp.vertical_margins().max(0.0) as u32,
            &child.lp,
        );
        (w, h)
    }

    /// 布局：按方向顺序排布子视图。
    pub fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.inner.view.layout(left, top, right, bottom);
        let content_left = left + self.inner.view.padding_left();
        let content_top = top + self.inner.view.padding_top();
        let content_w = (right - left - pad_h(&self.inner.view)).max(0.0);
        let content_h = (bottom - top - pad_v(&self.inner.view)).max(0.0);
        let mut cursor = match self.orientation {
            Orientation::Horizontal => content_left,
            Orientation::Vertical => content_top,
        };
        for child in &mut self.inner.children {
            if is_gone(child) {
                child.child.layout(cursor, content_top, cursor, content_top);
                continue;
            }
            let w = child.child.view().measured_width();
            let h = child.child.view().measured_height();
            match self.orientation {
                Orientation::Horizontal => {
                    let (.., cy) = gravity_offset(
                        child.gravity,
                        0.0,
                        content_h - h - child.lp.vertical_margins(),
                    );
                    let x = cursor + child.lp.left_margin.max(0.0);
                    let y = content_top + cy + child.lp.top_margin.max(0.0);
                    child.child.layout(x, y, x + w, y + h);
                    cursor = x + w + child.lp.right_margin.max(0.0);
                }
                Orientation::Vertical => {
                    let (cx, ..) = gravity_offset(
                        child.gravity,
                        content_w - w - child.lp.horizontal_margins(),
                        0.0,
                    );
                    let x = content_left + cx + child.lp.left_margin.max(0.0);
                    let y = cursor + child.lp.top_margin.max(0.0);
                    child.child.layout(x, y, x + w, y + h);
                    cursor = y + h + child.lp.bottom_margin.max(0.0);
                }
            }
        }
    }

    /// 绘制。
    pub fn draw(&self, scene: &mut Scene) {
        self.inner.draw_all(scene);
    }
}

/// gravity 偏移：给定可用富余空间计算子视图起点偏移。
fn gravity_offset(gravity: Gravity, free_w: f32, free_h: f32) -> (f32, f32) {
    let mut dx = 0.0;
    let mut dy = 0.0;
    if gravity.0 & 0x00000007 == 0x00000001 {
        // CENTER_HORIZONTAL
        dx = free_w / 2.0;
    } else if gravity.0 & 0x00000007 == 0x00000005 {
        // RIGHT
        dx = free_w;
    }
    if gravity.0 & 0x00000070 == 0x00000010 {
        // CENTER_VERTICAL
        dy = free_h / 2.0;
    } else if gravity.0 & 0x00000070 == 0x00000050 {
        // BOTTOM
        dy = free_h;
    }
    // CENTER = H|V
    if gravity.0 == 0x00000011 {
        dx = free_w / 2.0;
        dy = free_h / 2.0;
    }
    (dx, dy)
}

/// FrameLayout 的 Widget 实现：触摸沿子树递归分发。
impl Widget for FrameLayout {
    fn view(&self) -> &View {
        &self.inner.view
    }
    fn view_mut(&mut self) -> &mut View {
        &mut self.inner.view
    }

    fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        FrameLayout::measure(self, width_spec, height_spec)
    }

    fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        FrameLayout::layout(self, left, top, right, bottom)
    }

    fn draw(&self, scene: &mut Scene) {
        FrameLayout::draw(self, scene)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn for_each_child_mut(
        &mut self,
        event: &crate::ui::input::MotionEvent,
        f: &mut dyn FnMut(&mut dyn Widget, &crate::ui::input::MotionEvent) -> bool,
    ) -> bool {
        for child in self.inner.children.iter_mut().rev() {
            if f(&mut *child.child, event) {
                return true;
            }
        }
        false
    }
}

/// LinearLayout 的 Widget 实现：触摸沿子树递归分发。
impl Widget for LinearLayout {
    fn view(&self) -> &View {
        &self.inner.view
    }
    fn view_mut(&mut self) -> &mut View {
        &mut self.inner.view
    }

    fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        LinearLayout::measure(self, width_spec, height_spec)
    }

    fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        LinearLayout::layout(self, left, top, right, bottom)
    }

    fn draw(&self, scene: &mut Scene) {
        LinearLayout::draw(self, scene)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn for_each_child_mut(
        &mut self,
        event: &crate::ui::input::MotionEvent,
        f: &mut dyn FnMut(&mut dyn Widget, &crate::ui::input::MotionEvent) -> bool,
    ) -> bool {
        for child in self.inner.children.iter_mut().rev() {
            if f(&mut *child.child, event) {
                return true;
            }
        }
        false
    }
}

/// EXACTLY 取 spec，其余取内容值。
fn resolve(spec: MeasureSpec, content: f32) -> f32 {
    match spec.mode() {
        Mode::Exactly => spec.size() as f32,
        _ => content,
    }
}

/// 水平 padding（左右之和，px）。
fn pad_h(view: &View) -> f32 {
    (view.padding_left() + view.padding_right()).max(0.0)
}

/// 垂直 padding（上下之和，px）。
fn pad_v(view: &View) -> f32 {
    (view.padding_top() + view.padding_bottom()).max(0.0)
}

/// 子项是否参与布局（GONE 不占位、不绘制、不参与命中）。
fn is_gone(child: &PositionedChild) -> bool {
    child.child.view().visibility() == crate::ui::view::Visibility::Gone
}

/// 子项是否绘制（GONE / INVISIBLE 都不绘制）。
fn is_drawn(child: &PositionedChild) -> bool {
    !matches!(
        child.child.view().visibility(),
        crate::ui::view::Visibility::Gone | crate::ui::view::Visibility::Invisible
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(w: u32, h: u32) -> (MeasureSpec, MeasureSpec) {
        (
            MeasureSpec::make(Mode::Exactly, w),
            MeasureSpec::make(Mode::Exactly, h),
        )
    }

    /// FrameLayout 子项固定高必须被尊重（高度不能按 lp.width 推导）。
    ///
    /// 回归：曾用 width 版推导高度 spec，页签条(lp=-1×96)被撑成满高，
    /// 覆盖内容区导致点击命中错位。
    #[test]
    fn frame_layout_respects_fixed_child_height() {
        let mut frame = FrameLayout::new();
        let mut bar = View::new();
        bar.set_background_color(None);
        frame.add_child(bar, MarginLayoutParams::new(-1, 96), Gravity::TOP);

        let (ws, hs) = spec(1440, 2848);
        frame.measure(ws, hs);
        frame.layout(0.0, 152.0, 1440.0, 3000.0);

        let child = frame.child_at(0).expect("child present");
        let v = child.child.view();
        assert_eq!(v.top(), 152.0);
        assert_eq!(v.bottom(), 248.0, "fixed 96px height must be respected");
        assert_eq!(v.measured_height(), 96.0);
    }

    /// LinearLayout weight 子项：主轴平分，交叉轴仍按 LP 测量。
    ///
    /// 回归：曾对 weight 子项整对清零，交叉轴高度变 0，导致不可命中。
    #[test]
    fn linear_weight_keeps_cross_axis() {
        let mut bar = LinearLayout::horizontal();
        for _ in 0..3 {
            let mut tab = View::new();
            tab.set_clickable(true);
            let mut lp = MarginLayoutParams::new(0, 96);
            lp.weight = 1.0;
            bar.add_child(tab, lp, Gravity::TOP);
        }
        let (ws, hs) = spec(1440, 96);
        bar.measure(ws, hs);
        bar.layout(0.0, 0.0, 1440.0, 96.0);

        for i in 0..3 {
            let v = bar.child_at(i).unwrap().child.view();
            assert_eq!(v.measured_width(), 480.0, "tab {i} width");
            assert_eq!(v.measured_height(), 96.0, "tab {i} cross-axis height");
        }
        // 三等分不重叠：第二格从 480 起。
        assert_eq!(bar.child_at(1).unwrap().child.view().left(), 480.0);
        assert_eq!(bar.child_at(2).unwrap().child.view().left(), 960.0);
    }

    /// 容器嵌套分发链：事件需递归到深层可点击子项。
    /// 回归：曾在 dispatch_child 直接调 Widget 默认 dispatch_touch，
    /// 不递归子树，深层 tab 永远收不到事件。
    #[test]
    fn nested_dispatch_reaches_deep_clickable() {
        use crate::ui::input::dispatch::Node;
        use crate::ui::input::motion_event::{Action, MotionEvent, PointerCoords};
        use std::sync::atomic::{AtomicBool, Ordering};
        static HIT: AtomicBool = AtomicBool::new(false);
        HIT.store(false, Ordering::Relaxed);

        let mut root = FrameLayout::new();
        let mut bar = LinearLayout::horizontal();
        let mut tab = View::new();
        tab.set_clickable(true);
        let mut lp = MarginLayoutParams::new(0, 96);
        lp.weight = 1.0;
        bar.add_child(tab, lp, Gravity::TOP);
        // 注册必须用装箱后的最终堆地址（栈地址在移动后失效）。
        crate::ui::input::dispatch::gesture_registry::on(
            bar.child_at(0).unwrap().child.view().unique_id(),
            crate::ui::input::dispatch::gesture_registry::Gesture::Click,
            || HIT.store(true, Ordering::Relaxed),
        );
        root.add_widget(Box::new(bar), MarginLayoutParams::new(-1, 96), Gravity::TOP);

        let (ws, hs) = spec(1440, 2848);
        root.measure(ws, hs);
        root.layout(0.0, 0.0, 1440.0, 2848.0);

        let event = |action: i32| MotionEvent {
            packed_action: action,
            down_time: 0,
            event_time: 0,
            pointers: vec![PointerCoords {
                pointer_id: 0,
                x: 700.0,
                y: 40.0,
                pressure: 0.5,
            }],
            historical: vec![],
        };
        assert!(Node::Frame(&mut root).dispatch_touch(&event(Action::DOWN)));
        assert!(Node::Frame(&mut root).dispatch_touch(&event(Action::UP)));
        assert!(HIT.load(Ordering::Relaxed), "deep tab click must fire");
    }
}

#[cfg(test)]
mod padding_tests {
    use super::*;
    use crate::ui::view::Visibility;

    /// 容器 padding 必须同时进入测量（可用空间）与布局（子项起点）。
    #[test]
    fn frame_padding_offsets_child_and_shrinks_content() {
        let mut root = FrameLayout::new();
        root.inner.view.set_padding_dp(20.0, 10.0, 20.0, 10.0);
        let mut child = View::new();
        child.set_background_color(Some(crate::gfx::color::Color::WHITE));
        root.add_child(child, MarginLayoutParams::new(-1, -1), Gravity::TOP);

        let ws = MeasureSpec::make(Mode::Exactly, 400);
        let hs = MeasureSpec::make(Mode::Exactly, 800);
        root.measure(ws, hs);
        root.layout(0.0, 0.0, 400.0, 800.0);

        let c = root.child_at(0).unwrap().child.view();
        assert_eq!(c.left(), 20.0);
        assert_eq!(c.top(), 10.0);
        assert_eq!(c.width(), 360.0);
        assert_eq!(c.height(), 780.0);
    }

    /// GONE 子项不占位（LinearLayout 主轴不累加其尺寸）。
    #[test]
    fn gone_child_takes_no_space() {
        let mut col = LinearLayout::vertical();
        let mut a = View::new();
        a.set_measured_dimension(100.0, 100.0);
        col.add_child(a, MarginLayoutParams::new(-1, 100), Gravity::TOP);
        let mut gone = View::new();
        gone.set_visibility(Visibility::Gone);
        col.add_child(gone, MarginLayoutParams::new(-1, 100), Gravity::TOP);
        let mut b = View::new();
        col.add_child(b, MarginLayoutParams::new(-1, 100), Gravity::TOP);

        col.measure(
            MeasureSpec::make(Mode::Exactly, 200),
            MeasureSpec::make(Mode::AtMost, 1000),
        );
        assert_eq!(col.inner.view.measured_height(), 200.0);
        assert_eq!(col.child_at(1).unwrap().child.view().measured_height(), 0.0);
    }
}
