//! 触摸事件分发链（PRD §5.2，Task 18）。
//!
//! # 分发语义（对齐 Android `ViewGroup.dispatchTouchEvent`）
//! 1. 根节点入口 `dispatch_touch`；
//! 2. 容器先问 `on_intercept`（拦截后接管手势，子树不再接收后续事件）；
//! 3. 子节点逆序命中测试（后添加的在上层，先测——与绘制 z-order 一致）；
//! 4. 未命中/未消费（返回 false）→ 回抛给父级处理；
//! 5. DOWN 命中即捕获：后续 MOVE/UP 全部路由给捕获者，直到 UP/CANCEL 释放。
//!
//! # 节点模型
//! widgets 是「平行结构」（各控件持有自身 `View`），分发链需要统一
//! 树视图——`Node` 以枚举包住各控件的可变引用，零拷贝复用现有树，
//! 不改变 widgets 的所有权结构（Demo 装配在 Task 20 收敛树根）。
//!
//! # 时间基准
//! `event_time`/`down_time` 沿用 NDK `AMotionEvent_get*Time` 的
//! 毫秒基准（uptime base），手势阈值按毫秒比较。

use super::coords::{LocalPoint, hit_test};
use super::motion_event::{Action, MotionEvent};
use crate::ui::view::View;
use crate::ui::view_group::ViewGroup;
use crate::ui::widgets::button::Button;
use crate::ui::widgets::image_view::ImageView;
use crate::ui::widgets::layout::{FrameLayout, LinearLayout};
use crate::ui::widgets::scroll_view::ScrollView;
use crate::ui::widgets::text_view::TextView;
use crate::ui::widgets::widget::Widget;

/// 触摸分发节点：包住 widgets 的统一树视图。
pub enum Node<'a> {
    /// 纯 View（静态结构块）。
    View(&'a mut View),
    /// 任意 `dyn Widget` 子树（Task 20：容器子项的统一入口）。
    Widget(&'a mut dyn Widget),
    /// FrameLayout 容器。
    Frame(&'a mut FrameLayout),
    /// LinearLayout 容器。
    Linear(&'a mut LinearLayout),
    /// ScrollView（可拦截拖拽）。
    Scroll(&'a mut ScrollView),
    /// TextView。
    Text(&'a mut TextView),
    /// Button。
    Button(&'a mut Button),
    /// ImageView。
    Image(&'a mut ImageView),
    /// 纯 ViewGroup（基座容器）。
    Group(&'a mut ViewGroup),
}

impl Node<'_> {
    /// 本节点的 View 几何引用（命中测试用）。
    pub fn view(&self) -> &View {
        match self {
            Node::View(v) => v,
            Node::Widget(w) => w.view(),
            Node::Frame(f) => &f.inner.view,
            Node::Linear(l) => &l.inner.view,
            Node::Scroll(s) => &s.view,
            Node::Text(t) => &t.view,
            Node::Button(b) => &b.text_view.view,
            Node::Image(i) => &i.view,
            Node::Group(g) => &g.view,
        }
    }

    /// 触摸分发入口（对应 Android `dispatchTouchEvent`）。
    ///
    /// # 参数
    /// `event`：窗口坐标系触摸事件（所有控件 layout 均为窗口绝对坐标，
    /// 命中测试直接使用，无需逐层换算——见 `coords::hit_test`）。
    ///
    /// # 返回
    /// true = 本子树消费（false = 回抛父级/系统）。
    pub fn dispatch_touch(&mut self, event: &MotionEvent) -> bool {
        let p = LocalPoint {
            x: event.primary().map(|c| c.x).unwrap_or(0.0),
            y: event.primary().map(|c| c.y).unwrap_or(0.0),
        };
        match event.action() {
            // DOWN：先命中测试，命中才进入处理链。
            Action::Down => {
                if !hit_test(self.view(), p) {
                    return false;
                }
                // 具体控件优先走自身 `dispatch_touch`（如 ScrollView 拦截 /
                // Button 按压态）；Node 层只负责几何命中与容器递归。
                if let Node::Widget(w) = self {
                    return w.dispatch_touch(event);
                }
                self.on_touch(event)
            }
            // MOVE/UP/CANCEL：仅在 DOWN 命中的子树内继续分发
            //（逐层 hit_test 已由 DOWN 结果保证捕获关系；未捕获的
            // 兄弟节点因 DOWN 未命中而不会进入此分支）。
            Action::Move | Action::Up | Action::Cancel => {
                if let Node::Widget(w) = self {
                    return w.dispatch_touch(event);
                }
                self.on_touch(event)
            }
            // 多指事件 V1 未消费（缩放手势在 Demo 页另行接入）。
            _ => false,
        }
    }

    /// 本节点 onTouch（对应 `View.onTouchEvent` + 容器分发）。
    ///
    /// 容器先判拦截，再递归分发给子树，子树未消费时自身兜底处理。
    fn on_touch(&mut self, event: &MotionEvent) -> bool {
        // 1) 容器拦截（ScrollView 拖拽）：拦截后本节点接管手势。
        if self.intercept_child(event) {
            return self.on_self_touch(event);
        }
        // 2) 子树递归分发（逆序 z-order）。
        if self.dispatch_child(event) {
            return true;
        }
        // 3) 自身兜底处理（可点击叶子 / 可滚动容器）。
        self.on_self_touch(event)
    }

    /// 容器拦截判定（对应 `onInterceptTouchEvent`；V1 仅 ScrollView 拖拽）。
    fn intercept_child(&mut self, event: &MotionEvent) -> bool {
        match self {
            Node::Scroll(s) => s.on_intercept(event),
            _ => false,
        }
    }
    /// 子树递归分发。
    ///
    /// # 注意
    /// ScrollView 的坐标补偿由其 `for_each_child_mut` 覆写实现
    ///（子内容按 -scroll_y 绘制，命中测试把窗口坐标 y 补偿进内容
    /// 坐标系）；其余容器直接按窗口坐标递归。
    fn dispatch_child(&mut self, event: &MotionEvent) -> bool {
        let mut consumed = false;
        self.for_each_child_mut(event, &mut |child, ev| {
            // 子节点统一走 Node 分发语义（命中测试 → 拦截 → 子树 → 兜底），
            // 不能直接调 Widget trait 的 dispatch_touch（默认实现不递归）。
            if Node::Widget(child).dispatch_touch(ev) {
                consumed = true;
                true
            } else {
                false
            }
        });
        consumed
    }

    /// 回调式遍历子节点（`f(child, event)` 返回 true 时提前终止，
    /// 表示已消费）。
    ///
    /// 各容器/控件的具体遍历逻辑由 `Widget::for_each_child_mut` 实现；
    /// 本方法仅做 Node → trait 方法的路由（ScrollView 特例：拖拽
    /// 拦截后子树停发，由 on_self_touch 承接）。
    fn for_each_child_mut(
        &mut self,
        event: &MotionEvent,
        f: &mut dyn FnMut(&mut dyn Widget, &MotionEvent) -> bool,
    ) -> bool {
        match self {
            Node::Widget(w) => w.for_each_child_mut(event, f),
            Node::Frame(g) => g.for_each_child_mut(event, f),
            Node::Linear(g) => g.for_each_child_mut(event, f),
            Node::Scroll(s) => s.for_each_child_mut(event, f),
            // 叶子节点无子树。
            _ => false,
        }
    }

    /// 自身触摸处理：
    /// - Scroll：拖拽滚动；
    /// - 其他可点击节点：点击/长按手势状态机（回调注册表分发）。
    fn on_self_touch(&mut self, event: &MotionEvent) -> bool {
        match self {
            Node::Scroll(s) => s.on_scroll_handle(event),
            Node::Widget(w) => {
                // 控件自定义触摸优先（如 EditText 请求焦点）；默认实现
                // 即「可点击 → 手势推进」，与容器叶子的行为一致。
                w.dispatch_touch(event)
            }
            _ => {
                if !self.view().is_clickable() {
                    return false;
                }
                gesture_registry::advance(self.gesture_id(), event)
            }
        }
    }

    /// 节点手势 ID（回调注册表键）：用进程内唯一、永不复用的 View id，
    /// 避免视图重建后堆地址复用导致旧回调误触发。
    fn gesture_id(&self) -> u64 {
        self.view().unique_id()
    }
}

/// 手势回调注册表（V1 进程级单例；Demo 页装配时注册回调）。
pub mod gesture_registry {
    use super::super::coords::LocalPoint;
    use super::super::motion_event::{Action, MotionEvent};
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// 手势事件种类。
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum Gesture {
        /// 单击（DOWN-UP 未超 slop）。
        Click,
        /// 长按（DOWN 后停留超 LongPressTimeout 且未超 slop）。
        LongPress,
    }

    /// LongPressTimeout（ms）：Android `ViewConfiguration` 默认 400ms。
    pub const LONG_PRESS_TIMEOUT_MS: i64 = 400;

    /// 注册回调（同 ID+手势覆盖注册）。
    pub fn on(id: u64, gesture: Gesture, f: impl FnMut() + Send + 'static) {
        let mut reg = lock_registry();
        reg.entry(id)
            .or_default()
            .callbacks
            .insert(gesture, Box::new(f));
    }

    /// 清空注册表（宿主重建页树前调用）。
    ///
    /// # 为什么整表清空
    /// 页树按「状态 → 新树」重建，旧控件实例全部失效；逐视图反注册需要
    /// 宿主跟踪每个实例，成本高于直接清表。重建流程保证清表后立即重新注册。
    pub fn clear_all() {
        lock_registry().clear();
    }

    /// 反注册单个节点的全部手势（控件被移除且树未重建时使用）。
    pub fn unregister(id: u64) {
        lock_registry().remove(&id);
    }

    /// 取注册表锁；中毒时取回内层数据（单次回调 panic 不应让后续触摸全崩）。
    fn lock_registry() -> std::sync::MutexGuard<'static, HashMap<u64, NodeGestures>> {
        registry().lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 推进单节点手势状态机；识别成功时触发回调。
    ///
    /// # 返回
    /// true = 事件被手势消费（DOWN/活动手势期间的 MOVE/UP）。
    pub fn advance(id: u64, event: &MotionEvent) -> bool {
        let mut reg = lock_registry();
        // 未注册手势的视图不落表：否则每次触摸任意视图都会插入条目，
        // 长会话下注册表无界增长（且旧条目会残留过期识别器状态）。
        let Some(state) = reg.get_mut(&id) else {
            return false;
        };
        let handled = state.advance(event);
        if let Some(g) = state.take_recognized() {
            if let Some(cb) = state.callbacks.get_mut(&g) {
                cb();
            }
        }
        handled
    }

    /// 单节点手势状态（回调 + 识别器；节点本体不加字段，
    /// 避免 Task 14 已稳定的 widgets 结构再改动）。
    struct NodeGestures {
        callbacks: HashMap<Gesture, Box<dyn FnMut() + Send>>,
        recognizer: TapRecognizer,
    }

    impl Default for NodeGestures {
        fn default() -> Self {
            Self {
                callbacks: HashMap::new(),
                recognizer: TapRecognizer::default(),
            }
        }
    }

    impl NodeGestures {
        fn advance(&mut self, event: &MotionEvent) -> bool {
            self.recognizer.advance(event)
        }

        fn take_recognized(&mut self) -> Option<Gesture> {
            self.recognizer.take_recognized()
        }
    }

    /// 点击/长按识别器（TouchSlop + LongPressTimeout 语义，PRD §5.2）。
    struct TapRecognizer {
        /// DOWN 位置（未开始为 None）。
        down: Option<LocalPoint>,
        /// DOWN 事件时间（ms）。
        down_time: i64,
        /// 是否已移动超出 slop（true = 拖拽，取消点击/长按资格）。
        moved: bool,
        /// 已识别待触发手势。
        recognized: Option<Gesture>,
    }

    impl Default for TapRecognizer {
        fn default() -> Self {
            Self {
                down: None,
                down_time: 0,
                moved: false,
                recognized: None,
            }
        }
    }

    /// 触摸滑动阈值（px）：按设备密度换算（Android `ViewConfiguration`
    /// 基准 8dp）——唯一真源在 [`crate::ui::view_config::touch_slop`]。
    fn touch_slop() -> f32 {
        crate::ui::view_config::touch_slop()
    }

    impl TapRecognizer {
        /// 推进识别。
        ///
        /// # 消费语义
        /// - DOWN：开始手势，恒消费（与 Android clickable 视图一致）；
        /// - MOVE：活动手势期间消费；
        /// - UP：识别 Click；CANCEL：复位不消费。
        fn advance(&mut self, event: &MotionEvent) -> bool {
            match event.action() {
                Action::Down => {
                    self.down = event.primary().map(|c| LocalPoint { x: c.x, y: c.y });
                    self.down_time = event.event_time;
                    self.moved = false;
                    true
                }
                Action::Move => {
                    if let (Some(d), Some(p)) = (self.down, event.primary()) {
                        let dx = p.x - d.x;
                        let dy = p.y - d.y;
                        if dx * dx + dy * dy > touch_slop() * touch_slop() {
                            self.moved = true;
                        }
                        // 长按：停留未超 slop 且超时（时间由事件携带，
                        // 与 Android 定时触发器语义等价的离散近似）。
                        if !self.moved && event.event_time - self.down_time >= LONG_PRESS_TIMEOUT_MS
                        {
                            self.recognized = Some(Gesture::LongPress);
                        }
                    }
                    self.down.is_some()
                }
                Action::Up => {
                    let consumed = self.down.is_some();
                    if let (Some(d), Some(p)) = (self.down.take(), event.primary()) {
                        let dx = p.x - d.x;
                        let dy = p.y - d.y;
                        if !self.moved && dx * dx + dy * dy <= touch_slop() * touch_slop() {
                            self.recognized = Some(Gesture::Click);
                        }
                    }
                    consumed
                }
                Action::Cancel => {
                    self.down = None;
                    self.moved = false;
                    false
                }
                _ => false,
            }
        }

        fn take_recognized(&mut self) -> Option<Gesture> {
            self.recognized.take()
        }
    }

    fn registry() -> &'static Mutex<HashMap<u64, NodeGestures>> {
        static REGISTRY: std::sync::OnceLock<Mutex<HashMap<u64, NodeGestures>>> =
            std::sync::OnceLock::new();
        REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
    }

    #[cfg(test)]
    mod tests {
        use super::super::super::motion_event::{Action, MotionEvent, PointerCoords};

        /// 构造单指事件的便捷函数。
        fn ev(action: i32, x: f32, y: f32, t: i64) -> MotionEvent {
            MotionEvent {
                packed_action: action,
                down_time: 0,
                event_time: t,
                pointers: vec![PointerCoords {
                    pointer_id: 0,
                    x,
                    y,
                    pressure: 0.5,
                }],
                historical: vec![],
            }
        }

        /// 点击识别：DOWN→UP 未超 slop 触发 Click。
        #[test]
        fn tap_recognized() {
            let mut r = super::TapRecognizer::default();
            assert!(r.advance(&ev(Action::DOWN, 10.0, 10.0, 0)));
            assert!(r.advance(&ev(Action::UP, 12.0, 10.0, 50)));
            assert_eq!(r.take_recognized(), Some(super::Gesture::Click));
        }

        /// 移动超 slop：拖拽，不触发 Click。
        #[test]
        fn drag_not_click() {
            let mut r = super::TapRecognizer::default();
            assert!(r.advance(&ev(Action::DOWN, 0.0, 0.0, 0)));
            assert!(r.advance(&ev(Action::MOVE, 100.0, 0.0, 10)));
            assert!(r.advance(&ev(Action::UP, 100.0, 0.0, 20)));
            assert_eq!(r.take_recognized(), None, "drag must not click");
        }

        /// 长按：停留未超 slop 且超 400ms。
        #[test]
        fn long_press_recognized() {
            let mut r = super::TapRecognizer::default();
            assert!(r.advance(&ev(Action::DOWN, 0.0, 0.0, 0)));
            assert!(r.advance(&ev(Action::MOVE, 2.0, 0.0, 500)));
            assert_eq!(r.take_recognized(), Some(super::Gesture::LongPress));
        }

        /// CANCEL 复位手势。
        #[test]
        fn cancel_resets() {
            let mut r = super::TapRecognizer::default();
            assert!(r.advance(&ev(Action::DOWN, 0.0, 0.0, 0)));
            assert!(!r.advance(&ev(Action::CANCEL, 0.0, 0.0, 10)));
            // CANCEL 后 UP 不再属于本手势。
            assert!(
                !r.advance(&ev(Action::UP, 0.0, 0.0, 20)),
                "UP after CANCEL must be ignored"
            );
        }
    }
}
