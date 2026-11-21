//! 滚动容器：ScrollView 结构（PRD §5.1.7，Task 16）。
//!
//! # 范围
//! V1 结构：measure（撑满子内容）+ layout（应用 scroll 偏移）+
//! 裁剪绘制 + 拖拽滚动（Task 18：Down 命中→Move 超 slop 拦截接管，
//! 偏移随 MOVE 差量累加，Up/CANCEL 结束手势）。

use crate::gfx::scene::{Command, Scene};
use crate::ui::input::motion_event::{Action, MotionEvent};
use crate::ui::measure_spec::{MeasureSpec, Mode};
use crate::ui::view::View;
use crate::ui::view_group::MarginLayoutParams;
use crate::ui::widgets::widget::Widget;

/// 触摸滑动阈值（px）：统一取框架真源（随设备密度缩放，基准 8dp）。
use crate::ui::view_config::touch_slop;

/// 垂直滚动容器。
pub struct ScrollView {
    /// 自身 View 状态。
    pub view: View,
    /// 唯一子内容（ScrollView 语义：单子，通常一个竖排 LinearLayout）。
    child: Option<Box<dyn Widget>>,
    /// 子内容布局参数。
    child_lp: Option<MarginLayoutParams>,
    /// 当前纵向滚动偏移（px，>= 0，内容高 - 容器高 封顶）。
    scroll_y: f32,
    /// 拖拽手势中（已拦截）。
    dragging: bool,
    /// 手势起点 y（窗口坐标）。
    drag_start_y: Option<f32>,
    /// 手势起点时的滚动偏移。
    drag_start_scroll: f32,
    /// 最近一次 MOVE 的时间与 y（速度估计用，单位 ms / px）。
    last_move_time: i64,
    last_move_y: f32,
    /// 惯性速度（px/s；正值 = 内容上滚方向，与 scroll_y 增方向一致）。
    fling_velocity: f32,
    /// 惯性是否进行中。
    fling_active: bool,
    /// 惯性时间基准（ms，事件基准）。
    fling_last_ms: i64,
}

impl ScrollView {
    /// 创建。
    pub fn new() -> Self {
        Self {
            view: View::new(),
            child: None,
            child_lp: None,
            scroll_y: 0.0,
            dragging: false,
            drag_start_y: None,
            drag_start_scroll: 0.0,
            last_move_time: 0,
            last_move_y: 0.0,
            fling_velocity: 0.0,
            fling_active: false,
            fling_last_ms: 0,
        }
    }

    /// 设置唯一子内容（任意控件子树）。
    pub fn set_child(&mut self, child: Box<dyn Widget>, lp: MarginLayoutParams) {
        self.child = Some(child);
        self.child_lp = Some(lp);
        self.view.request_layout();
    }

    /// 子内容访问。
    pub fn child(&self) -> Option<&dyn Widget> {
        self.child.as_deref()
    }

    /// 拦截判定（`onInterceptTouchEvent` 语义）：
    /// DOWN 时记录起点；MOVE 超过 TouchSlop 后接管手势。
    ///
    /// # 返回
    /// true = 本容器接管（子树停止接收后续事件，由本容器滚动）。
    pub fn on_intercept(&mut self, event: &MotionEvent) -> bool {
        // 注意：不能因 `dragging` 提前 return true——多指序列里第二根手指
        // 会再发一次 ACTION_DOWN，若此时不重锚起点，后续 MOVE 会用上一轮
        // 的 drag_start_y 计算增量（内容瞬间跳动）。
        match event.action() {
            Action::Down => {
                // 新手指按下必须终止惯性（Android `OverScroller.abortAnimation`）。
                self.stop_fling();
                let y = event.primary().map(|c| c.y);
                self.drag_start_y = y;
                self.drag_start_scroll = self.scroll_y;
                self.last_move_time = event.event_time;
                self.last_move_y = y.unwrap_or(0.0);
                false
            }
            Action::Move => {
                if let (Some(sy), Some(p)) = (self.drag_start_y, event.primary()) {
                    let dy = p.y - sy;
                    if dy * dy > touch_slop() * touch_slop() {
                        self.dragging = true;
                    }
                }
                self.dragging
            }
            Action::Up | Action::Cancel => {
                let was = self.dragging;
                if was {
                    self.end_drag(event);
                } else {
                    self.dragging = false;
                }
                was
            }
            _ => false,
        }
    }

    /// 拦截后的滚动处理：MOVE 按差量滚动（起点锚定，与 Android
    /// `scrollBy` 手势等价），其余动作仅消费。
    pub fn on_scroll_handle(&mut self, event: &MotionEvent) -> bool {
        match event.action() {
            Action::Move => {
                if let (Some(sy), Some(p)) = (self.drag_start_y, event.primary()) {
                    // 手势锚定：scroll = 起始偏移 + (起点 y - 当前 y)
                    //（手指下移 → 内容上滚展示上方内容）。
                    let target = self.drag_start_scroll + (sy - p.y);
                    self.scroll_y = target.clamp(0.0, self.max_scroll());
                    // 速度估计：手指位移/时间；scroll 方向与手指相反。
                    // 边界钳制期间速度清零（避免到顶后仍积累冲量）。
                    if self.scroll_y != target {
                        self.fling_velocity = 0.0;
                    } else if event.event_time > self.last_move_time {
                        let dt = (event.event_time - self.last_move_time) as f32 / 1000.0;
                        let finger_v = (p.y - self.last_move_y) / dt;
                        let sample = -finger_v;
                        // 低通：最后一次采样占 75%，抑制抖动。
                        self.fling_velocity = 0.75 * sample + 0.25 * self.fling_velocity;
                    }
                    self.last_move_time = event.event_time;
                    self.last_move_y = p.y;
                }
                true
            }
            Action::Up => {
                if self.dragging {
                    self.end_drag(event);
                } else {
                    self.dragging = false;
                }
                true
            }
            Action::Cancel => {
                self.stop_fling();
                self.dragging = false;
                true
            }
            _ => true,
        }
    }

    /// 手势结束时判定是否进入惯性（速度阈值 120px/s ≈ 轻微快滑）。
    ///
    /// # 参数
    /// `event`：保留给后续速度采样细化（V1 用最后一帧增量近似）。
    fn end_drag(&mut self, _event: &MotionEvent) {
        self.dragging = false;
        if self.fling_velocity.abs() >= 120.0 {
            self.fling_active = true;
            self.fling_last_ms = 0;
        } else {
            self.fling_velocity = 0.0;
        }
    }

    /// 终止惯性（新按下/取消时）。
    pub fn stop_fling(&mut self) {
        self.fling_active = false;
        self.fling_velocity = 0.0;
    }

    /// 惯性是否进行中（宿主据此请求帧回调）。
    pub fn is_flinging(&self) -> bool {
        self.fling_active
    }

    /// 惯性推进（每帧调用）。
    ///
    /// # 返回
    /// true = 偏移发生变化（需要重绘）。
    pub fn advance_fling(&mut self, now_ms: i64) -> bool {
        if !self.fling_active {
            return false;
        }
        // 惯性起始帧只锚定时间基准（宿主时钟与事件时钟不同源）。
        if self.fling_last_ms == 0 {
            self.fling_last_ms = now_ms;
            return false;
        }
        let dt = now_ms - self.fling_last_ms;
        if dt <= 0 {
            return false;
        }
        self.fling_last_ms = now_ms;
        let before = self.scroll_y;
        // 指数摩擦：每 16ms 衰减约 2%（Android OverScroller DECELERATION_RATE 近似）。
        let decay = (1.0f32 - 0.0013 * dt as f32).max(0.0);
        self.scroll_y = (self.scroll_y + self.fling_velocity * dt as f32 / 1000.0)
            .clamp(0.0, self.max_scroll());
        self.fling_velocity *= decay;
        // 到达边界或速度过低即停止。
        if self.scroll_y == before && (self.scroll_y <= 0.0 || self.scroll_y >= self.max_scroll()) {
            self.stop_fling();
        } else if self.fling_velocity.abs() < 30.0 {
            self.stop_fling();
        }
        self.scroll_y != before
    }

    /// 当前滚动偏移。
    pub fn scroll_y(&self) -> f32 {
        self.scroll_y
    }

    /// 内容视图只读访问（调试/测试观测用）。
    pub fn content_view(&self) -> Option<&View> {
        self.child.as_ref().map(|c| c.view())
    }

    /// 内容容器首个子项视图（调试观测用，仅内容为容器时有效）。
    pub fn content_first_child(&self) -> Option<&View> {
        let child = self.child.as_ref()?;
        if let Some(l) = child
            .as_any()
            .downcast_ref::<crate::ui::widgets::layout::LinearLayout>()
        {
            return l.inner.children.first().map(|c| c.child.view());
        }
        None
    }

    /// 遍历唯一子内容（无障碍快照等只读场景；不可变借用）。
    ///
    /// # 与 `for_each_child_mut` 的区别
    /// 此入口只读、不做坐标补偿——无障碍快照需要内容坐标系的真实值，
    /// 且不得因滚动偏移改变节点层级或 bounds 语义。
    pub fn for_each_visible_child<F: FnMut(&dyn Widget)>(&self, mut f: F) {
        if let Some(child) = self.child.as_ref() {
            f(child.as_ref());
        }
    }

    /// 最大可滚动距离（内容高 - 容器高，非负）。
    pub fn max_scroll(&self) -> f32 {
        let content = self
            .child
            .as_ref()
            .map(|c| c.view().height())
            .unwrap_or(0.0);
        (content - self.view.height()).max(0.0)
    }

    /// 请求滚动到指定偏移（Task 18 手势入口；自动钳制到 [0, max]）。
    pub fn request_scroll_to(&mut self, y: f32) {
        self.stop_fling();
        self.scroll_y = y.clamp(0.0, self.max_scroll());
        // 滚动只影响绘制偏移，不触发重新测量/布局。
    }

    /// 测量：容器按 spec，子内容按 LP 尺寸推导 spec
    /// （具体尺寸 → EXACTLY；WRAP/MATCH 高度 → UNSPECIFIED，内容自报，
    /// 撑出滚动范围）。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        let w = match width_spec.mode() {
            Mode::Exactly => width_spec.size() as f32,
            _ => 0.0,
        };
        let h = match height_spec.mode() {
            Mode::Exactly => height_spec.size() as f32,
            _ => 0.0,
        };
        self.view.set_measured_dimension(w, h);
        if let Some(child) = self.child.as_mut() {
            let lp = self.child_lp.as_ref().unwrap();
            let child_w = crate::ui::view_group::get_child_measure_spec(width_spec, 0, lp.width);
            // 高度：WRAP/MATCH 时 UNSPECIFIED（滚动内容允许溢出），
            // 具体尺寸时 EXACTLY（固定高度内容，仍可滚动）。
            let child_h = match lp.height {
                d if d >= 0 => MeasureSpec::make(Mode::Exactly, d as u32),
                _ => MeasureSpec::make(Mode::Unspecified, 0),
            };
            child.measure(child_w, child_h);
        }
    }

    /// 布局：子内容按内容高度完整布局（不压缩），滚动只改绘制偏移。
    pub fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.view.layout(left, top, right, bottom);
        let (cw, ch) = (right - left, bottom - top);
        if let Some(child) = self.child.as_mut() {
            let child_h = child.view().measured_height();
            child.layout(left, top, left + cw, top + child_h.max(ch));
        }
        // 布局后重新钳制滚动偏移（容器尺寸可能变化）。
        self.scroll_y = self.scroll_y.min(self.max_scroll());
    }

    /// 绘制：裁剪到自身 bounds，子内容按 -scroll_y 平移后并入。
    pub fn draw(&self, scene: &mut Scene) {
        self.view.draw(scene);
        let Some(child) = self.child.as_ref() else {
            return;
        };
        let (l, t) = (self.view.left(), self.view.top());
        let (w, h) = (self.view.width(), self.view.height());
        // 裁剪：子内容溢出部分不可见。
        scene.commands.push(Command::ClipRect {
            rect: crate::gfx::scene::Rect {
                x: f64::from(l),
                y: f64::from(t),
                w: f64::from(w),
                h: f64::from(h),
            },
        });
        // 子内容绘制进独立场景，整体平移后并入父场景。
        let mut child_scene = Scene::new(w as u32, h as u32);
        child.draw(&mut child_scene);
        child_scene.translate(0.0, -f64::from(self.scroll_y));
        scene.commands.extend(child_scene.commands);
        scene.commands.push(Command::PopClips { count: 1 });
    }
}

/// ScrollView 的 Widget 实现：触摸分发含子内容坐标补偿。
impl Widget for ScrollView {
    fn view(&self) -> &View {
        &self.view
    }
    fn view_mut(&mut self) -> &mut View {
        &mut self.view
    }

    fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        ScrollView::measure(self, width_spec, height_spec)
    }

    fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        ScrollView::layout(self, left, top, right, bottom)
    }

    fn draw(&self, scene: &mut Scene) {
        ScrollView::draw(self, scene)
    }
    fn advance(&mut self, now_ms: i64) -> bool {
        self.advance_fling(now_ms)
    }
    fn is_animating(&self) -> bool {
        self.is_flinging()
    }
    /// 触摸：拦截优先 → 子树 → 自身滚动。
    ///
    /// # 为什么必须在 Widget 层覆写
    /// 树中的 ScrollView 以 `dyn Widget` 挂载，分发走 `Node::Widget`，
    /// 不会命中 `Node::Scroll` 的拦截/滚动脉络；不覆写则手势全部落到
    /// 子项（行/卡片）的点击状态机，容器永远不会滚动。
    fn dispatch_touch(&mut self, event: &MotionEvent) -> bool {
        if self.on_intercept(event) {
            return self.on_scroll_handle(event);
        }
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
        self.on_scroll_handle(event)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    /// 子树遍历：把窗口坐标补偿进内容坐标系后递归子节点
    ///（子内容按 -scroll_y 绘制的逆变换；未拦截时才补偿分发）。
    fn for_each_child_mut(
        &mut self,
        event: &MotionEvent,
        f: &mut dyn FnMut(&mut dyn Widget, &MotionEvent) -> bool,
    ) -> bool {
        if self.dragging {
            // 拦截后子树收 CANCEL 语义：V1 子内容无状态，停止分发。
            return false;
        }
        let Some(child) = self.child.as_mut() else {
            return false;
        };
        let mut shifted = event.clone();
        if let Some(c) = shifted.pointers.first_mut() {
            c.y += self.scroll_y;
        }
        f(&mut **child, &shifted)
    }
}

impl Default for ScrollView {
    fn default() -> Self {
        Self::new()
    }
}
