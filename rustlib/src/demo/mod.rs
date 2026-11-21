//! 三页 Demo 装配（PRD §5.4，Task 20）。
//!
//! # 页面
//! - P1 滚动列表：200 项行，点击高亮，`ScrollView` 拖拽滚动；
//! - P2 自定义绘制：Canvas 原语 + 渐变/裁剪/圆角/描边 + 属性动画；
//! - P3 文字排版：中英混排、多行折行、尾部省略、行距演示。
//!
//! # 装配模型（与 Task 16/18 平行结构的对接）
//! widgets 容器只持 `View`（结构几何），`TextView` 等内容控件无法
//! 挂入容器——Demo 采用「结构树 + 内容控件平行持有」模型：
//! - `PageTree`：容器树（布局 + 背景绘制）；
//! - `ContentSlots`：与结构位置对应的文本控件列表（measure 用容器
//!   产出的同一 LP 尺寸，draw 平移到结构位置）。
//! 每次状态变更（切页/高亮/滚动）后整页重建；确定性由纯函数保证。
//!
//! # 无限动画
//! P2 旋转/呼吸动画为无限往返（`RepeatMode::Reverse`）；真机帧驱动
//! 由 glue 的 Choreographer 帧回调推进（A11：合并进帧回调，无新线程）。

use crate::anim::{AccelerateDecelerate, ValueAnimator};
use crate::gfx::canvas::Canvas;
use crate::gfx::color::Color;
use crate::gfx::image::Image;
use crate::gfx::paint::{Paint, PaintStyle};
use crate::gfx::path::Path;
use crate::gfx::scene::{Rect, Scene};
use crate::gfx::shader::{Shader, TileMode};
use crate::text::font::FontData;
use crate::text::layout::TextContext;
use crate::text::metrics::{TextMeasurer, TextPaint};
use crate::ui::ime::text_input::{ImeOptions, InputType};
use crate::ui::input::MotionEvent;
use crate::ui::input::dispatch::gesture_registry::{self, Gesture};
use crate::ui::measure_spec::{MeasureSpec, Mode};
use crate::ui::view::{Gravity, View};
use crate::ui::view_group::MarginLayoutParams;
use crate::ui::widgets::edit_text::EditText;
use crate::ui::widgets::image_view::{ImageView, ScaleType};
use crate::ui::widgets::layout::{FrameLayout, LinearLayout};
use crate::ui::widgets::scroll_view::ScrollView;
use crate::ui::widgets::text_view::{Ellipsize, TextAlignment, TextView};

/// 页签条高度 48dp（Android 标准触控条高度；640dpi 上为 192px）。
fn tab_bar_height() -> f32 {
    crate::ui::dimen::dp(48.0)
}

/// P1 列表行高 56dp（Android 标准列表项高度；含触摸友好高度）。
fn row_height() -> f32 {
    crate::ui::dimen::dp(56.0)
}

/// 页面内容左右边距 16dp。
fn pad_x() -> f32 {
    crate::ui::dimen::dp(16.0)
}

/// 当前页面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    /// P1 滚动列表。
    List,
    /// P2 自定义绘制。
    Draw,
    /// P3 文字排版。
    Text,
}

/// 演示主机：状态 + 装配 + 帧入口。
///
/// # 线程模型
/// 全部状态由 glue 主循环线程独占访问（事件泵与帧回调同线程）；
/// 回调经原子变量桥接（单写多读无竞争）。
pub struct Demo {
    /// 当前页。
    pub page: Page,
    /// P1 被点击高亮的行下标（None = 无）。
    pub highlighted_row: Option<usize>,
    /// P1 滚动偏移（跨重建保留）。
    scroll_y: f32,
    /// P2 动画驱动器（无限往返）。
    animator: ValueAnimator,
    /// P2 当前相位输出 [0,1]。
    anim_phase: f32,
    /// 文本排版上下文（所有 TextView 共享注入；每次重建移动进新控件）。
    /// 字体数据（进程共享；None 时文本示例降级为空白）。
    font: Option<FontData>,
    /// 文本测量器（光标锚点 x 计算用；字体缺失时 None）。
    measurer: Option<TextMeasurer>,
    /// P2 双指缩放系数（1.0 = 原始大小；范围 [0.5, 3.0]）。
    p2_scale: f32,
    /// P2 上一次双指间距（px；单指/序列结束时清空）。
    p2_prev_dist: Option<f32>,
    /// 当前页树缓存（手势状态要求 Down/Up 命中同一控件实例，
    /// 状态变更才重建）。
    tree: Option<PageTree>,
    /// P3 输入框文本（跨重建保留，IME 回推写入）。
    input_text: String,
    /// P3 输入框预编辑区间（字符下标；None = 无 composing）。
    input_composing: Option<(usize, usize)>,
    /// 输入框是否聚焦（IME 打开）。
    input_focused: bool,
    /// 页树重建代数（无障碍快照按此判断是否需要重推）。
    tree_generation: u32,
    /// 窗口宽（px）。
    width: f32,
    /// 窗口高（px）。
    height: f32,
}

impl Demo {
    /// 创建：默认 P1 页。
    ///
    /// # 参数
    /// `font`：字体加载成功后注入；None 时文本示例降级为空白
    ///（结构/滚动/手势不受影响）。
    pub fn new(width: i32, height: i32, font: Option<FontData>) -> Self {
        let mut animator = ValueAnimator::of_float(0.0, 1.0);
        // P2 动画：3s 往返无限循环。
        animator.set_duration(3000);
        animator.set_repeat(u32::MAX, crate::anim::RepeatMode::Reverse);
        animator.set_interpolator(AccelerateDecelerate);
        // 相位经原子桥接写回（回调侧无 &mut Self）。
        animator.set_on_update(|v| {
            PHASE.store(v.to_bits(), std::sync::atomic::Ordering::Relaxed);
        });
        let measurer = font
            .as_ref()
            .map(|f| TextMeasurer::new(TextContext::new(f)));
        Self {
            page: Page::List,
            highlighted_row: None,
            scroll_y: 0.0,
            animator,
            anim_phase: 0.0,
            font,
            measurer,
            p2_scale: 1.0,
            p2_prev_dist: None,
            tree: None,
            input_text: String::new(),
            input_composing: None,
            input_focused: false,
            tree_generation: 0,
            width: width as f32,
            height: height as f32,
        }
    }

    /// 窗口尺寸变化（作废页树缓存，下次布局前重建）。
    pub fn resize(&mut self, width: i32, height: i32) {
        self.width = width as f32;
        self.height = height as f32;
        self.invalidate_tree();
        // 后端 surface 尺寸变化由平台层重建后端（此处只作废页树）。
    }

    /// 取布局好的页树：缓存缺失时按当前状态重建并测量布局。
    fn tree_for_layout(&mut self) -> &mut PageTree {
        if self.tree.is_none() {
            let mut tree = self.build_tree();
            let ws = MeasureSpec::make(Mode::Exactly, self.width as u32);
            let content_h =
                (self.height - self.insets_top() - self.insets_bottom()).max(0.0) as u32;
            let hs = MeasureSpec::make(Mode::Exactly, content_h);
            tree.measure(ws, hs);
            let top = self.content_top();
            let bottom = self.content_bottom();
            tree.layout(0.0, top, self.width, bottom);
            self.tree = Some(tree);
            // 每次实际重建都推进代数，无障碍快照据此判断需重推。
            self.tree_generation = self.tree_generation.wrapping_add(1);
        }
        self.tree.as_mut().unwrap()
    }

    /// 页树重建代数（无障碍快照去重用）。
    pub fn a11y_generation(&self) -> u32 {
        self.tree_generation
    }

    /// 状态变更后作废页树（下次渲染/分发前重建）。
    ///
    /// 重建会替换全部控件实例，因此同步清空手势注册表，避免旧回调
    /// 残留（注册表键是永不复用的 View id，清表是为了释放内存与闭包）。
    fn invalidate_tree(&mut self) {
        gesture_registry::clear_all();
        self.tree = None;
    }

    /// 平台参数（insets/键盘高度）变化后作废页树（重新布局）。
    pub fn invalidate_for_insets(&mut self) {
        // 键盘弹出（inset > 0）即视为输入框获得焦点：驱动光标绘制与锚点。
        if self.page == Page::Text && ime_inset_px() > 0 {
            self.input_focused = true;
            self.push_cursor_anchor();
        }
        self.invalidate_tree();
    }

    /// 页面切换：输入框失焦（收键盘）并清预编辑态。
    fn leave_text_page(&mut self) {
        if matches!(self.page, Page::Text) {
            crate::ui::ime::manager::request_blur();
        }
        self.input_focused = false;
        if self.input_composing.take().is_some() {
            self.invalidate_tree();
        }
    }

    /// 把插入点锚点（屏幕坐标）下发给平台 IME（候选窗/放大镜跟随）。
    fn push_cursor_anchor(&mut self) {
        let Some(measurer) = self.measurer.as_mut() else {
            return;
        };
        let mut paint = TextPaint::default();
        paint.font_size = crate::ui::dimen::sp(16.0);
        // 文本宽度（行尾光标位）；V1 光标恒在末尾。
        let width = measurer.measure_text(&self.input_text, &paint);
        let x = pad_x() + width + 1.0;
        let y = self.content_top() + tab_bar_height() + crate::ui::dimen::dp(16.0);
        let h = crate::ui::dimen::sp(16.0) * 1.3;
        #[cfg(target_os = "android")]
        crate::platform::android::ime_bridge::push_cursor_anchor(x, y, h);
        let _ = (x, y, h);
    }

    /// 当前页标识（日志观测用）。
    pub fn current_page(&self) -> u8 {
        match self.page {
            Page::List => 0,
            Page::Draw => 1,
            Page::Text => 2,
        }
    }

    /// 测试钩子：直接设定 P2 动画相位（黄金图静态帧用；绕过
    /// ValueAnimator 的时间驱动，避免基准对墙上时钟敏感）。
    pub fn set_anim_phase_for_test(&mut self, phase: f32) {
        self.anim_phase = phase;
    }

    /// 帧推进（Choreographer 帧回调入口）。
    ///
    /// # 返回
    /// true = P2 相位变化需要重绘（P1/P3 静态无帧驱动）。
    pub fn advance(&mut self, now_ms: u64) -> bool {
        let before = self.anim_phase;
        if self.animator.advance(now_ms) {
            self.anim_phase = f32::from_bits(PHASE.load(std::sync::atomic::Ordering::Relaxed));
        }
        let anim_dirty = self.page == Page::Draw && self.anim_phase != before;
        // 滚动惯性：与动画同帧推进（同一时钟基准）。
        let fling_dirty = self
            .tree
            .as_mut()
            .map(|t| t.advance_scroll(now_ms as i64))
            .unwrap_or(false);
        if fling_dirty && (self.page == Page::List || self.page == Page::Text) {
            let prev_scroll = self.scroll_y;
            self.scroll_y = self
                .tree
                .as_ref()
                .map(|t| t.current_scroll_y())
                .unwrap_or(self.scroll_y);
            if (self.scroll_y - prev_scroll).abs() > 0.5 {
                log::info!("list scrolled to {:.1}", self.scroll_y);
            }
        }
        anim_dirty || fling_dirty
    }

    /// 是否需要持续帧驱动（P2 动画 / 滚动惯性）。
    pub fn needs_frames(&self) -> bool {
        self.page == Page::Draw || self.fling_active()
    }

    /// 滚动惯性是否仍在进行（由页树里的 ScrollView 报告）。
    fn fling_active(&self) -> bool {
        self.tree.as_ref().is_some_and(|t| t.is_flinging())
    }

    /// 合成系统 CANCEL：窗口失焦/切页时复位全部手势状态。
    ///
    /// # 为什么必须合成
    /// ScrollView 一旦进入拖拽态，只有 UP/CANCEL 能复位；窗口失焦后
    /// 系统不再投递后续触摸，若不合成 CANCEL，该实例会永久吞掉所有触摸。
    pub fn cancel_touch(&mut self) {
        use crate::ui::input::motion_event::PointerCoords;
        let cancel = MotionEvent {
            packed_action: crate::ui::input::motion_event::Action::CANCEL,
            down_time: 0,
            event_time: 0,
            pointers: vec![PointerCoords {
                pointer_id: 0,
                x: 0.0,
                y: 0.0,
                pressure: 0.0,
            }],
            historical: Vec::new(),
        };
        if let Some(tree) = self.tree.as_mut() {
            tree.dispatch_touch(&cancel);
        }
        gesture_registry::clear_all();
    }

    /// 内容区顶部（窗口坐标）：状态栏下沿（edge-to-edge 避让）。
    fn content_top(&self) -> f32 {
        crate::platform::android::insets::status_bar() as f32
    }

    /// 内容区底部（窗口坐标）：取导航栏上沿与键盘上沿的较高者。
    ///
    /// # 键盘避让
    /// edge-to-edge 下窗口 surface 不随 IME 变化，故用 Kotlin 转发的
    /// IME 高度自行收缩内容区（PRD §5.1.9 输入法 L2 桥接语义）。
    fn content_bottom(&self) -> f32 {
        let nav = crate::platform::android::insets::nav_bar() as f32;
        let ime = ime_inset_px() as f32;
        self.height - nav.max(ime)
    }

    /// 状态栏高度（px）。
    fn insets_top(&self) -> f32 {
        crate::platform::android::insets::status_bar() as f32
    }

    /// 导航栏高度（px）。
    fn insets_bottom(&self) -> f32 {
        crate::platform::android::insets::nav_bar() as f32
    }

    /// 触摸入口：缓存树分发 → 消费回调状态。
    ///
    /// # 返回
    /// true = 状态变更需要重绘。
    pub fn dispatch_touch(&mut self, motion: &MotionEvent) -> bool {
        // 0) P2：双指缩放（Canvas 叠层整体缩放，手势独立于控件树）。
        if self.page == Page::Draw && self.handle_pinch(motion) {
            return true;
        }
        // 1) 回调残留状态清零（防上次未消费的变更串页）。
        clear_pending();
        // 2) 缓存树分发（手势状态机要求 Down/Up 命中同一实例；
        //    与 render 同布局参数，保证命中一致）。
        let handled = {
            let tree = self.tree_for_layout();
            tree.dispatch_touch(motion)
        };
        // 3) ScrollView 拖拽后的偏移回写（滚动即重绘）。
        if self.page == Page::List || self.page == Page::Text {
            let prev_scroll = self.scroll_y;
            self.scroll_y = self
                .tree
                .as_ref()
                .map(|t| t.current_scroll_y())
                .unwrap_or(self.scroll_y);
            if (self.scroll_y - prev_scroll).abs() > 0.5 {
                log::info!("list scrolled to {:.1}", self.scroll_y);
            }
        }
        // 4) 回调状态消费：页签切换/高亮变更 → 作废树缓存。
        let mut dirty = handled;
        if let Some(page) = take_page() {
            if page != self.page {
                self.leave_text_page();
            }
            self.page = page;
            self.scroll_y = 0.0;
            self.invalidate_tree();
            dirty = true;
        }
        if let Some(row) = take_row() {
            self.highlighted_row = if self.highlighted_row == Some(row) {
                None // 再点取消高亮。
            } else {
                Some(row)
            };
            self.invalidate_tree();
            dirty = true;
        }
        // 5) 未命中也无需重绘，但 Down 已消费时返回 true（状态机活跃）。
        dirty
    }

    /// P2 双指缩放：按两指间距比例乘性更新 `p2_scale`。
    ///
    /// # 返回
    /// true = 缩放系数变化（需要重绘）。
    fn handle_pinch(&mut self, motion: &MotionEvent) -> bool {
        let action = motion.action();
        if matches!(
            action,
            crate::ui::input::Action::Down
                | crate::ui::input::Action::PointerDown
                | crate::ui::input::Action::Up
                | crate::ui::input::Action::PointerUp
                | crate::ui::input::Action::Cancel
        ) {
            // 序列边界/指针集合变化：间距基准失效，等待下一次 MOVE 重建。
            self.p2_prev_dist = None;
            return false;
        }
        if motion.pointer_count() < 2 {
            self.p2_prev_dist = None;
            return false;
        }
        let (Some(a), Some(b)) = (motion.pointer(0), motion.pointer(1)) else {
            return false;
        };
        let dist = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
        let prev = self.p2_prev_dist.replace(dist);
        let Some(prev) = prev else {
            return false;
        };
        if prev <= 1.0 || dist <= 1.0 {
            return false;
        }
        let next = (self.p2_scale * dist / prev).clamp(0.5, 3.0);
        if (next - self.p2_scale).abs() < 1e-4 {
            return false;
        }
        self.p2_scale = next;
        log::info!("p2 scale {:.2}", self.p2_scale);
        true
    }

    /// 测试钩子：读取 P2 当前缩放系数。
    pub fn p2_scale(&self) -> f32 {
        self.p2_scale
    }

    /// IME 文本回推（平台桥转发）：写入输入框状态。
    ///
    /// # 返回
    /// true = 状态变化需要重绘。
    pub fn on_ime_text(&mut self, text: &str, composing: Option<(usize, usize)>) -> bool {
        // 仅 P3 页有输入框；其他页忽略（防串页）。
        if self.page != Page::Text {
            return false;
        }
        let text_changed = self.input_text != text;
        let composing_changed = self.input_composing != composing;
        if !text_changed && !composing_changed {
            return false;
        }
        self.input_text = text.to_string();
        self.input_composing = composing;
        log::info!(
            "ime text reached native: {} chars",
            self.input_text.chars().count()
        );
        self.input_focused = true;
        log::debug!(
            "ime text synced: chars={} composing={:?}",
            text.chars().count(),
            composing
        );
        // 输入框状态变化即作废页树（下次渲染按新文本重建）。
        self.invalidate_tree();
        self.push_cursor_anchor();
        true
    }

    /// IME 被系统收起（失焦）：清除预编辑态。
    ///
    /// # 返回
    /// true = 状态变化需要重绘。
    pub fn on_ime_blur(&mut self) -> bool {
        if !self.input_focused && self.input_composing.is_none() {
            return false;
        }
        self.input_focused = false;
        let had_composing = self.input_composing.take().is_some();
        if had_composing {
            self.invalidate_tree();
        }
        had_composing
    }

    /// 生成当前页的无障碍快照（SPEC-accessibility.md §3）。
    ///
    /// # 调用时机
    /// 仅由平台层在帧回调内、且 TalkBack 开启时调用——不得挂在 draw 路径。
    /// 内部会先确保页树已布局（与 `render` 同参数），因此快照中的
    /// `boundsInScreen` 与实际显示一致。
    pub fn a11y_snapshot(&mut self) -> Vec<crate::ui::a11y::A11yNode> {
        let tree = self.tree_for_layout();
        crate::ui::a11y::tree::snapshot_of(&tree.root)
    }

    /// 触发节点点击（TalkBack `ACTION_CLICK` 回推）。
    ///
    /// # 参数
    /// `node_id`：快照中的节点 id（下标语义，仅同一快照内有效）。
    ///
    /// # 返回
    /// true = 已触发点击且状态变更需要重绘。
    ///
    /// # 与手势点击的一致性
    /// 无障碍点击与手势点击必须走同一状态通路：这里按节点 id 取回
    /// 宿主动作标签，再分派到与手势回调相同的状态变更逻辑，避免
    /// TalkBack 路径与手势路径行为分叉。
    pub fn perform_a11y_click(&mut self, node_id: i32) -> bool {
        let Some(tag) = self.a11y_action_tag_of(node_id) else {
            return false;
        };
        match DemoAction::from_tag(tag) {
            Some(DemoAction::SwitchPage(page)) => {
                if self.page == page {
                    return false;
                }
                self.leave_text_page();
                self.page = page;
                self.scroll_y = 0.0;
                self.invalidate_tree();
                true
            }
            Some(DemoAction::ToggleRow(row)) => {
                self.highlighted_row = if self.highlighted_row == Some(row) {
                    None
                } else {
                    Some(row)
                };
                self.invalidate_tree();
                true
            }
            None => false,
        }
    }

    /// 查快照节点对应的动作标签（复用同一遍历顺序，id 与快照一一对应）。
    fn a11y_action_tag_of(&self, node_id: i32) -> Option<i32> {
        let tree = self.tree.as_ref()?;
        let snapshot = crate::ui::a11y::tree::snapshot_of(&tree.root);
        snapshot
            .get(node_id.max(0) as usize)
            .map(|n| n.action_tag)
            .filter(|t| *t >= 0)
    }

    /// 渲染当前页：布局结构树 + 绘制内容控件 → Scene。
    pub fn render(&mut self) -> Scene {
        let mut scene = Scene::new(self.width as u32, self.height as u32);
        scene.clear_color = Color::rgb(0x18, 0x18, 0x18);
        self.tree_for_layout().draw(&mut scene);
        // P2 叠加层（Canvas 原语 + 动画相位）。
        if self.page == Page::Draw {
            self.draw_p2_overlay(&mut scene);
        }
        scene
    }

    /// 装配当前页：树结构 + 内容控件（重建模型：状态→树，纯函数）。
    fn build_tree(&mut self) -> PageTree {
        match self.page {
            Page::List => self.build_list_page(),
            Page::Draw => self.build_draw_page(),
            Page::Text => self.build_text_page(),
        }
    }

    /// 通用页框：页签条 + 内容区。
    ///
    /// 页签条三个可点击 View（背景色区分选中态），点击回调切页；
    /// 内容区由各页装配函数填充。
    fn frame(&self, content_top: f32, content_bottom: f32) -> (FrameLayout, f32, f32) {
        let mut root = FrameLayout::new();
        // 页签条（横排三格）。
        let mut bar = LinearLayout::horizontal();
        bar.inner
            .view
            .set_background_color(Some(Color::rgb(0x21, 0x21, 0x21)));
        for page in [Page::List, Page::Draw, Page::Text] {
            // 页签用 TextView：既有可点击命中，也有可见文字标签
            //（纯色块在深色背景上无法辨识当前在哪一页）。
            let mut tab = if let Some(font) = self.font.as_ref() {
                let mut tv = TextView::new();
                tv.set_context(clone_text_context(font));
                tv.set_text(page_label(page));
                tv.set_text_size_sp(14.0);
                tv.set_text_color(Color::WHITE);
                tv.set_text_alignment(TextAlignment::Center);
                tv
            } else {
                TextView::new()
            };
            tab.view.set_clickable(true);
            // 无障碍：页签是语义按钮，需可被 TalkBack 聚焦并点按。
            tab.view.set_a11y_role(crate::ui::a11y::A11yRole::Button);
            tab.view.set_a11y_action_tag(DemoAction::tab_tag(page));
            let selected = self.page == page;
            tab.view.set_a11y_checked(selected);
            // 选中：蓝色底条；未选中：深灰。
            tab.view.set_background_color(Some(if selected {
                Color::rgb(0x20, 0x82, 0xF8)
            } else {
                Color::rgb(0x2C, 0x2C, 0x2C)
            }));
            tab.view.set_padding_dp(0.0, 8.0, 0.0, 8.0);
            // 三等分宽度：weight=1 平分容器（MATCH_PARENT 会各撑满
            // 容器宽度，横向排开后 tab2/3 被推出可视区）。
            let mut lp = MarginLayoutParams::new(0, tab_bar_height() as i32);
            lp.weight = 1.0;
            bar.add_widget(Box::new(tab), lp, Gravity::TOP);
        }
        // 装箱后注册：手势键必须用子项最终的堆地址（装箱会移动 View，
        // 注册栈地址将永远无法命中）。
        for (child, page) in bar
            .inner
            .children
            .iter_mut()
            .zip([Page::List, Page::Draw, Page::Text])
        {
            register_tab_switch(child.child.view_mut(), page);
        }
        root.add_widget(
            Box::new(bar),
            MarginLayoutParams::new(-1, tab_bar_height() as i32),
            Gravity::TOP,
        );
        (root, content_top, content_bottom)
    }

    /// P1：200 项滚动列表。
    ///
    /// 结构：ScrollView > LinearLayout > 200 行（每行 = 背景 View 容器？
    /// V1 每行为一个可点击 TextView——文字直接在行控件上，命中/绘制
    /// 均由 TextView 承担）。
    fn build_list_page(&mut self) -> PageTree {
        let (mut root, _, _) = self.frame(tab_bar_height(), self.height);
        // 内容区高度：状态栏与导航栏之间再扣除页签条
        //（多算了 insets 会让 Gravity::BOTTOM 内容上移盖住页签条）。
        let content_h = self.content_bottom() - self.content_top() - tab_bar_height();

        let mut scroll = ScrollView::new();
        let mut list = LinearLayout::vertical();

        for i in 0..200usize {
            // 行控件：文字行直接用 TextView（可点击 + 高亮背景）。
            let mut row = if let Some(font) = self.font.as_ref() {
                let mut tv = TextView::new();
                tv.set_context(clone_text_context(font));
                tv.set_text(format!("Row #{i} — item {}", i + 1));
                tv.set_text_size_sp(15.0);
                tv.set_text_color(Color::WHITE);
                tv
            } else {
                // 无字体降级：空文本行（结构/滚动/手势仍完整）。
                TextView::new()
            };
            row.view.set_clickable(true);
            // 无障碍：行是语义按钮（可点按切换高亮）。
            row.view.set_a11y_role(crate::ui::a11y::A11yRole::Button);
            row.view.set_a11y_action_tag(DemoAction::row_tag(i));
            // 行内边距 16dp（文字不贴屏边）。
            row.view.set_padding_horizontal_dp(16.0, 16.0);
            let highlighted = self.highlighted_row == Some(i);
            row.view.set_background_color(Some(if highlighted {
                Color::rgb(0x2A, 0x6E, 0xE0)
            } else if i % 2 == 0 {
                Color::rgb(0x22, 0x22, 0x22)
            } else {
                Color::rgb(0x2A, 0x2A, 0x2A)
            }));
            // 手势注册键：装箱后按最终堆地址注册（TextView 内部 View
            // 地址在 Box 后稳定，重建后地址变化则旧键自然失效）。
            list.add_widget(
                Box::new(row),
                MarginLayoutParams::new(-1, row_height() as i32),
                Gravity::TOP,
            );
            register_row_highlight(list.inner.children.last_mut().unwrap().child.view_mut(), i);
        }
        scroll.set_child(Box::new(list), MarginLayoutParams::new(-1, -2));
        scroll.request_scroll_to(self.scroll_y);
        root.add_widget(
            Box::new(scroll),
            MarginLayoutParams::new(-1, content_h as i32),
            Gravity::BOTTOM,
        );
        PageTree { root }
    }

    /// P2：自定义绘制（结构仅底色页框，内容由 `draw_p2_overlay`）。
    fn build_draw_page(&mut self) -> PageTree {
        let (mut root, _, _) = self.frame(tab_bar_height(), self.height);
        // 内容区高度：状态栏与导航栏之间再扣除页签条
        //（多算了 insets 会让 Gravity::BOTTOM 内容上移盖住页签条）。
        let content_h = self.content_bottom() - self.content_top() - tab_bar_height();
        let mut bg = View::new();
        bg.set_background_color(Some(Color::rgb(0x18, 0x18, 0x18)));
        root.add_child(
            bg,
            MarginLayoutParams::new(-1, content_h as i32),
            Gravity::BOTTOM,
        );
        // P2 图片控件：验证 ImageView 的 decode → 场景 DrawImage 链路。
        if let Ok(img) = Image::decode(include_bytes!("../../assets/demo_image.png")) {
            let mut iv = ImageView::new();
            iv.set_image(Some(img));
            iv.set_scale_type(ScaleType::FitCenter);
            iv.view.set_a11y_role(crate::ui::a11y::A11yRole::Image);
            root.add_widget(
                Box::new(iv),
                MarginLayoutParams::new(
                    crate::ui::dimen::dp(240.0) as i32,
                    crate::ui::dimen::dp(150.0) as i32,
                ),
                Gravity::CENTER,
            );
        }
        PageTree { root }
    }

    /// P3：文字排版四组示例（滚动可浏览全部）。
    fn build_text_page(&mut self) -> PageTree {
        let (mut root, _, _) = self.frame(tab_bar_height(), self.height);
        // 内容区高度：状态栏与导航栏之间再扣除页签条
        //（多算了 insets 会让 Gravity::BOTTOM 内容上移盖住页签条）。
        let content_h = self.content_bottom() - self.content_top() - tab_bar_height();

        let mut scroll = ScrollView::new();
        let mut list = LinearLayout::vertical();

        // P3 顶部：输入框（点击拉起软键盘，验证 IME 桥全链路）。
        let mut input = EditText::new();
        if let Some(font) = self.font.as_ref() {
            input.set_context(clone_text_context(font));
        }
        input.set_text(self.input_text.clone());
        input.set_text_size_sp(16.0);
        input.set_text_color(Color::WHITE);
        input.set_input_type(InputType::Text);
        input.set_ime_options(ImeOptions::Done);
        crate::ui::widgets::widget::Widget::view_mut(&mut input)
            .set_a11y_role(crate::ui::a11y::A11yRole::EditText);
        crate::ui::widgets::widget::Widget::view_mut(&mut input)
            .set_background_color(Some(Color::rgb(0x33, 0x33, 0x33)));
        crate::ui::widgets::widget::Widget::view_mut(&mut input)
            .set_padding_dp(16.0, 16.0, 16.0, 16.0);
        if self.input_focused {
            input.set_focused(true);
        }
        if let Some((start, end)) = self.input_composing {
            // 恢复预编辑态（重建后 composing 下划线不丢）。
            input.on_ime_text(&self.input_text.clone(), Some((start, end)));
        }
        list.add_widget(
            Box::new(input),
            MarginLayoutParams::new(-1, crate::ui::dimen::dp(56.0) as i32),
            Gravity::TOP,
        );

        let samples: [(&str, f32, TextAlignment, Ellipsize, f32); 4] = [
            (
                "NativeView 跨语言排版：Chinese-English mixed text rendering with parley.",
                18.0,
                TextAlignment::Left,
                Ellipsize::None,
                1.0,
            ),
            (
                "这是一段用于验证自动折行能力的长文本，它应当根据容器宽度自动换行而不是溢出边界，行高按行距倍率排列。",
                16.0,
                TextAlignment::Left,
                Ellipsize::None,
                1.5,
            ),
            (
                "Ellipsize sample: the quick brown fox jumps over the lazy dog again and again",
                16.0,
                TextAlignment::Left,
                Ellipsize::End,
                1.0,
            ),
            (
                "Center aligned text",
                20.0,
                TextAlignment::Center,
                Ellipsize::None,
                1.0,
            ),
        ];
        for (text, size, align, ellip, spacing) in samples {
            let mut card = TextView::new();
            if let Some(font) = self.font.as_ref() {
                card.set_context(clone_text_context(font));
                card.set_text(text);
                card.set_text_size_sp(size);
                // 深色卡片必须显式指定白字（默认黑色在深底上不可读）。
                card.set_text_color(Color::WHITE);
                card.set_text_alignment(align);
                card.set_ellipsize(ellip);
                card.set_line_spacing_multiplier(spacing);
            }
            // 卡片背景（结构色 + 排版文本同体）+ 16dp 内边距。
            card.view.set_a11y_role(crate::ui::a11y::A11yRole::Text);
            card.view
                .set_background_color(Some(Color::rgb(0x24, 0x24, 0x24)));
            card.view.set_padding_dp(16.0, 12.0, 16.0, 12.0);
            list.add_widget(
                Box::new(card),
                MarginLayoutParams::new(-1, -2),
                Gravity::TOP,
            );
        }
        scroll.set_child(Box::new(list), MarginLayoutParams::new(-1, -2));
        scroll.request_scroll_to(self.scroll_y);
        root.add_widget(
            Box::new(scroll),
            MarginLayoutParams::new(-1, content_h as i32),
            Gravity::BOTTOM,
        );
        PageTree { root }
    }

    /// P2 叠加层：Canvas 全原语演示（旋转方块/呼吸圆/渐变圆角条/描边环）。
    fn draw_p2_overlay(&self, scene: &mut Scene) {
        let w = self.width as f64;
        // 内容区：tab 条下沿 → 导航栏上沿（与树布局同坐标系）。
        let oy = self.content_top() as f64 + tab_bar_height() as f64;
        let h = (self.content_bottom() - self.content_top() - tab_bar_height()) as f64;
        let phase = self.anim_phase;

        let mut c = Canvas::new(self.width as u32, self.height as u32);
        // 双指缩放：围绕内容区中心整体缩放（图片/图形同步跟随）。
        let (cx0, cy0) = (w * 0.5, oy + h * 0.5);
        c.save();
        c.translate(cx0, cy0);
        c.scale(f64::from(self.p2_scale), f64::from(self.p2_scale));
        c.translate(-cx0, -cy0);

        // 1) 旋转方块：相位驱动整圈旋转（边长为 36dp，随密度缩放）。
        let cx = w * 0.5;
        let cy = oy + h * 0.32;
        let size = f64::from(crate::ui::dimen::dp(36.0));
        c.save();
        c.translate(cx, cy);
        c.rotate(f64::from(phase * std::f32::consts::TAU));
        c.draw_rect(
            Rect {
                x: -size / 2.0,
                y: -size / 2.0,
                w: size,
                h: size,
            },
            &Paint::default().color(Color::rgb(0x20, 0x82, 0xF8)),
        );
        c.restore();

        // 2) 呼吸圆：半径与透明度随相位变化（18dp → 30dp 脉动）。
        let alpha = ((0.35 + 0.65 * (1.0 - phase)) * 255.0) as u8;
        let r = crate::ui::dimen::dp(18.0) + crate::ui::dimen::dp(12.0) * phase;
        c.draw_path(
            &Path::circle(w * 0.22, oy + h * 0.72, f64::from(r)),
            &Paint::default().color(Color::rgba(0xF8, 0xA2, 0x20, alpha)),
        );

        // 3) 渐变圆角条：线性渐变 + clip 圆角（条高 12dp，圆角 6dp）。
        let strip_y = oy + h * 0.88;
        let strip_h = f64::from(crate::ui::dimen::dp(12.0));
        let strip_radius = f64::from(crate::ui::dimen::dp(6.0));
        let padx = pad_x() as f64;
        c.save();
        c.clip_path(&Path::rounded_rect(
            padx,
            strip_y - strip_h / 2.0,
            w - 2.0 * padx,
            strip_h,
            strip_radius,
        ));
        let shader = Shader::linear(
            padx,
            strip_y,
            w - padx,
            strip_y,
            &[Color::RED, Color::YELLOW, Color::GREEN, Color::BLUE],
            None,
            TileMode::Clamp,
        );
        c.draw_rect(
            Rect {
                x: padx,
                y: strip_y - strip_h / 2.0,
                w: w - 2.0 * padx,
                h: strip_h,
            },
            &Paint::default().shader(Some(shader)),
        );
        c.restore();

        // 4) 描边圆环：PaintStyle::Stroke 演示（半径 24dp、线宽 3dp）。
        c.draw_path(
            &Path::circle(
                w * 0.78,
                oy + h * 0.72,
                f64::from(crate::ui::dimen::dp(24.0)),
            ),
            &Paint::default()
                .style(PaintStyle::Stroke)
                .stroke_width(crate::ui::dimen::dp(3.0))
                .color(Color::WHITE),
        );
        // 关闭叠层整体缩放（与开头 save 配对）。
        c.restore();

        let overlay = c.finish();
        scene.commands.extend(overlay.commands);
    }
}

/// P2 相位桥接（回调写、advance 读；Relaxed 足够——同线程序）。
static PHASE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

// ---- 回调桥接（gesture_registry 回调 → Demo 状态消费） ----

/// 待切换页（0/1/2 = List/Draw/Text；u8::MAX = 无）。
static PENDING_PAGE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(u8::MAX);
/// 待高亮行（usize::MAX 上限哨兵 = 无）。
static PENDING_ROW: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(u64::MAX);

fn register_tab_switch(view: &View, page: Page) {
    let code = match page {
        Page::List => 0u8,
        Page::Draw => 1,
        Page::Text => 2,
    };
    gesture_registry::on(view.unique_id(), Gesture::Click, move || {
        PENDING_PAGE.store(code, std::sync::atomic::Ordering::Relaxed);
    });
}

fn register_row_highlight(view: &View, row: usize) {
    gesture_registry::on(view.unique_id(), Gesture::Click, move || {
        PENDING_ROW.store(row as u64, std::sync::atomic::Ordering::Relaxed);
    });
}

fn clear_pending() {
    PENDING_PAGE.store(u8::MAX, std::sync::atomic::Ordering::Relaxed);
    PENDING_ROW.store(u64::MAX, std::sync::atomic::Ordering::Relaxed);
}

fn take_page() -> Option<Page> {
    match PENDING_PAGE.swap(u8::MAX, std::sync::atomic::Ordering::Relaxed) {
        0 => Some(Page::List),
        1 => Some(Page::Draw),
        2 => Some(Page::Text),
        _ => None,
    }
}

fn take_row() -> Option<usize> {
    let v = PENDING_ROW.swap(u64::MAX, std::sync::atomic::Ordering::Relaxed);
    if v == u64::MAX {
        None
    } else {
        Some(v as usize)
    }
}

// ---- 结构辅助 ----

/// 输入法遮挡高度（px；0 = 键盘收起）。
///
/// # 平台差异
/// 仅 Android 有 IME 桥；主机侧恒为 0（黄金图/单测布局不受键盘影响）。
fn ime_inset_px() -> u32 {
    #[cfg(target_os = "android")]
    {
        crate::platform::android::ime_bridge::ime_inset()
    }
    #[cfg(not(target_os = "android"))]
    {
        0
    }
}

/// TextContext 构造：每次重建对同一 FontData 重新构造
///（parley FontContext 不 Clone——重建即重新注册字体；代价可接受：
/// 字体字节 mmap 共享，注册仅建索引）。
fn clone_text_context(cx: &FontData) -> TextContext {
    TextContext::new(cx)
}

/// 无障碍动作标签编码（Demo 内部约定，框架不理解其语义）。
///
/// # 编码规则
/// - 0..=2：切换到对应页（0=List，1=Draw，2=Text）；
/// - 1000 + i：切换第 i 行高亮。
///
/// 框架只负责把 `View::a11y_action_tag` 原样透传进快照并经 JNI 回推，
/// 因此这套编码属于宿主私有协议，变更不影响框架。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DemoAction {
    /// 切换页。
    SwitchPage(Page),
    /// 切换列表行高亮。
    ToggleRow(usize),
}

impl DemoAction {
    /// 页签动作 → 标签值。
    fn tab_tag(page: Page) -> i32 {
        match page {
            Page::List => 0,
            Page::Draw => 1,
            Page::Text => 2,
        }
    }

    /// 行动作 → 标签值。
    fn row_tag(index: usize) -> i32 {
        1000 + index as i32
    }

    /// 标签值 → 动作（非法值返回 None）。
    fn from_tag(tag: i32) -> Option<Self> {
        match tag {
            0 => Some(DemoAction::SwitchPage(Page::List)),
            1 => Some(DemoAction::SwitchPage(Page::Draw)),
            2 => Some(DemoAction::SwitchPage(Page::Text)),
            t if t >= 1000 => Some(DemoAction::ToggleRow((t - 1000) as usize)),
            _ => None,
        }
    }
}

/// 页签显示标签（英文，与全应用日志/文案语言一致）。
fn page_label(page: Page) -> &'static str {
    match page {
        Page::List => "List",
        Page::Draw => "Draw",
        Page::Text => "Text",
    }
}

/// 页树：结构根（FrameLayout 树，Task 20 起容器持 `Box<dyn Widget>`）。
struct PageTree {
    root: FrameLayout,
}

impl PageTree {
    /// 结构树测量。
    fn measure(&mut self, ws: MeasureSpec, hs: MeasureSpec) {
        self.root.measure(ws, hs);
    }

    /// 结构树布局。
    fn layout(&mut self, l: f32, t: f32, r: f32, b: f32) {
        self.root.layout(l, t, r, b);
    }

    /// 触摸分发（结构树根）。
    fn dispatch_touch(&mut self, motion: &MotionEvent) -> bool {
        use crate::ui::input::dispatch::Node;
        Node::Frame(&mut self.root).dispatch_touch(motion)
    }

    /// 绘制。
    fn draw(&self, scene: &mut Scene) {
        self.root.draw(scene);
    }

    /// 从树中读回当前页 ScrollView 的滚动偏移（跨重建保留）。
    fn current_scroll_y(&self) -> f32 {
        find_scroll_y(&self.root)
    }

    /// 推进树内可动画控件的帧状态（惯性/动画）。
    fn advance_scroll(&mut self, now_ms: i64) -> bool {
        let mut changed = false;
        for child in &mut self.root.inner.children {
            // 页内容容器（ScrollView）负责其子树动画；V1 只有它可动画。
            if child.child.as_mut().advance(now_ms) {
                changed = true;
            }
        }
        changed
    }

    /// 树内是否存在进行中的惯性。
    fn is_flinging(&self) -> bool {
        self.root
            .inner
            .children
            .iter()
            .any(|c| c.child.as_ref().is_animating())
    }
}

/// 递归找树中首个 ScrollView 的滚动偏移（V1 每页至多一个滚动容器）。
fn find_scroll_y(root: &FrameLayout) -> f32 {
    for child in &root.inner.children {
        if let Some(s) = child.child.as_ref().as_any().downcast_ref::<ScrollView>() {
            return s.scroll_y();
        }
    }
    0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// P2 叠加层确定性：同相位两次渲染一致。
    #[test]
    fn p2_overlay_deterministic() {
        let mut d = Demo::new(1080, 2400, None);
        d.page = Page::Draw;
        d.anim_phase = 0.5;
        let a = d.render();
        let b = d.render();
        assert_eq!(a.commands, b.commands);
    }

    /// 动画相位改变 P2 输出。
    #[test]
    fn anim_phase_changes_overlay() {
        let mut d = Demo::new(1080, 2400, None);
        d.page = Page::Draw;
        d.anim_phase = 0.0;
        let a = d.render();
        d.anim_phase = 0.5;
        let b = d.render();
        assert_ne!(a.commands, b.commands);
    }

    /// P2 双指缩放：间距变大 → 系数增大；序列结束复位基准。
    #[test]
    fn pinch_scales_p2() {
        use crate::ui::input::PointerCoords;
        let mut d = Demo::new(1080, 2400, None);
        d.page = Page::Draw;
        let ev = |a: i32, d0: f32, d1: f32| MotionEvent {
            packed_action: a,
            down_time: 0,
            event_time: 0,
            pointers: vec![
                PointerCoords {
                    pointer_id: 0,
                    x: 540.0 - d0,
                    y: 1200.0,
                    pressure: 0.5,
                },
                PointerCoords {
                    pointer_id: 1,
                    x: 540.0 + d1,
                    y: 1200.0,
                    pressure: 0.5,
                },
            ],
            historical: vec![],
        };
        // 首帧 Move 只建立基准。
        assert!(!d.dispatch_touch(&ev(crate::ui::input::Action::MOVE, 100.0, 100.0)));
        // 间距 100 → 150：系数应同比放大 1.5 倍。
        assert!(d.dispatch_touch(&ev(crate::ui::input::Action::MOVE, 150.0, 150.0)));
        assert!(
            (d.p2_scale() - 1.5).abs() < 1e-3,
            "scale = {}",
            d.p2_scale()
        );
        // Cancel 清基准，且系数保持。
        d.dispatch_touch(&ev(crate::ui::input::Action::CANCEL, 0.0, 0.0));
        assert!(d.p2_prev_dist.is_none());
        assert!((d.p2_scale() - 1.5).abs() < 1e-3);
    }

    /// P1 单指拖拽必须真正滚动 ScrollView（Widget 分发路径）。
    #[test]
    fn drag_scrolls_list_in_widget_dispatch() {
        use crate::ui::input::PointerCoords;
        let mut d = Demo::new(1080, 2400, None);
        d.page = Page::List;
        let ev = |a: i32, y: f32, t: i64| MotionEvent {
            packed_action: a,
            down_time: 0,
            event_time: t,
            pointers: vec![PointerCoords {
                pointer_id: 0,
                x: 540.0,
                y,
                pressure: 0.5,
            }],
            historical: vec![],
        };
        d.dispatch_touch(&ev(crate::ui::input::Action::DOWN, 1500.0, 0));
        d.dispatch_touch(&ev(crate::ui::input::Action::MOVE, 1054.0, 50));
        d.dispatch_touch(&ev(crate::ui::input::Action::MOVE, 900.0, 100));
        d.dispatch_touch(&ev(crate::ui::input::Action::UP, 900.0, 150));
        let y = d
            .tree
            .as_ref()
            .map(|t| t.current_scroll_y())
            .unwrap_or(-1.0);
        assert!(y > 100.0, "scroll_y must advance, got {y}");
    }

    /// 三页均可装配渲染（无字体降级路径）。
    #[test]
    fn all_pages_render() {
        let mut d = Demo::new(1080, 2400, None);
        for page in [Page::List, Page::Draw, Page::Text] {
            d.page = page;
            let scene = d.render();
            assert!(!scene.commands.is_empty());
        }
    }

    /// 页面切换状态正确。
    #[test]
    fn page_switch() {
        let mut d = Demo::new(1080, 2400, None);
        d.page = Page::Text;
        assert_eq!(d.page, Page::Text);
    }

    /// a11y 点击回推：tab 节点触发切页，且与手势路径共用同一状态通路。
    ///
    /// # 不变量
    /// 快照节点 id 是「同一快照内」的下标；本测试按 `action_tag` 定位
    /// 目标 tab，避免依赖文案（无字体时标签为空）或硬编码下标。
    #[test]
    fn a11y_click_switches_page() {
        let mut d = Demo::new(1080, 2400, None);
        assert_eq!(d.page, Page::List);
        // 先布局一次，使 a11y_action_tag_of 能拿到树。
        let _ = d.render();
        let nodes = d.a11y_snapshot();
        let draw_tab = nodes
            .iter()
            .find(|n| n.action_tag == DemoAction::tab_tag(Page::Draw))
            .expect("Draw tab node must carry its action tag");
        assert!(draw_tab.clickable, "tab node must be clickable");
        assert!(d.perform_a11y_click(draw_tab.id), "click must be handled");
        assert_eq!(d.page, Page::Draw);
    }

    /// a11y 点击回推：不可点击节点（无 action tag）返回 false 且不改状态。
    #[test]
    fn a11y_click_on_non_actionable_node_is_noop() {
        let mut d = Demo::new(1080, 2400, None);
        let _ = d.render();
        let nodes = d.a11y_snapshot();
        let container = nodes
            .iter()
            .find(|n| !n.clickable && n.action_tag < 0)
            .expect("container node must exist");
        let before = d.page;
        assert!(!d.perform_a11y_click(container.id));
        assert_eq!(d.page, before);
    }
}
