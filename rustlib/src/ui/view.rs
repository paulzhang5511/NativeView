//! View 基类：L1 全量属性与生命周期（PRD §5.1.1，Task 14）。
//!
//! # 设计
//! - `View` 是无渲染后端的纯状态机：measure/layout 结果驱动几何，
//!   `draw` 把背景与内容翻译进 `gfx::Scene`；
//! - measure 不变量：spec 与上次一致且未 forceLayout 时直接返回，
//!   不回调 onMeasure（滚动流畅度前提）；
//! - 子类通过覆盖 `on_measure`/`on_draw`（V1 以闭包/继承经
//!   `ui/widgets` 演进）扩展行为。

use crate::gfx::color::Color;
use crate::gfx::scene::Scene;

use super::measure_spec::{MeasureSpec, Mode};

/// 可见性（Android 数值：VISIBLE=0，INVISIBLE=4，GONE=8）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Visibility {
    /// 可见。
    #[default]
    Visible,
    /// 不可见但占位（参与布局）。
    Invisible,
    /// 不可见且不占位（从布局中消失）。
    Gone,
}

/// Gravity 常量（与 Android `Gravity` 数值一致，PRD §5.1.9）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Gravity(pub u32);

impl Gravity {
    /// 常量：与 Android `Gravity.NO_GRAVITY`（0）一致。
    pub const NO_GRAVITY: Gravity = Gravity(0);
    pub const LEFT: Gravity = Gravity(0x00000003);
    pub const TOP: Gravity = Gravity(0x00000030);
    pub const RIGHT: Gravity = Gravity(0x00000005);
    pub const BOTTOM: Gravity = Gravity(0x00000050);
    pub const CENTER: Gravity = Gravity(0x00000011);
    pub const CENTER_HORIZONTAL: Gravity = Gravity(0x00000001);
    pub const CENTER_VERTICAL: Gravity = Gravity(0x00000010);
}

/// View 基类：属性 + 测量/布局/绘制生命周期。
pub struct View {
    // --- 身份 ---
    /// 进程内唯一且永不复用的标识。
    ///
    /// # 为什么需要
    /// 手势回调注册表不能用 View 地址作键：视图重建后释放的堆地址会被
    /// 新 View 复用，导致旧回调被新视图误触发。唯一 id 从根上消除别名。
    id: u64,
    // --- 几何（layout 结果）---
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    // --- 测量结果 ---
    measured_width: f32,
    measured_height: f32,
    old_width_spec: Option<MeasureSpec>,
    old_height_spec: Option<MeasureSpec>,
    on_measure_count: u32,
    // --- 布局脏标志 ---
    layout_requested: bool,
    // --- L1 属性 ---
    visibility: Visibility,
    enabled: bool,
    clickable: bool,
    alpha: f32,
    translation_x: f32,
    translation_y: f32,
    scale_x: f32,
    scale_y: f32,
    pivot_x: f32,
    pivot_y: f32,
    rotation: f32,
    padding_left: f32,
    padding_top: f32,
    padding_right: f32,
    padding_bottom: f32,
    background_color: Option<Color>,
    gravity: Gravity,
    // --- 无障碍属性（SPEC-accessibility.md §3）---
    /// 内容描述（TalkBack 优先播报，空 = 无）。
    content_description: String,
    /// 无障碍角色（映射为 className 与播报语义）。
    a11y_role: crate::ui::a11y::A11yRole,
    /// 是否被选中（复选框/开关语义）。
    a11y_checked: bool,
    /// 无障碍快照脏标记（属性或几何变更时置位）。
    a11y_dirty: bool,
    /// 无障碍动作标签（宿主自定义的可点击目标标识；-1 = 无动作）。
    ///
    /// # 用途
    /// TalkBack 的 `ACTION_CLICK` 回推只携带节点 id，框架需要把它映射回
    /// 业务动作。标签由宿主在装配时设置，框架只负责在快照 id 命中时
    /// 原样返回，从而避免框架层认识任何具体业务类型。
    a11y_action_tag: i32,
}

impl Default for View {
    /// Android 默认值对齐。
    fn default() -> Self {
        Self::new()
    }
}

impl View {
    /// 创建默认 View。
    pub fn new() -> Self {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self {
            id,
            left: 0.0,
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            measured_width: 0.0,
            measured_height: 0.0,
            old_width_spec: None,
            old_height_spec: None,
            on_measure_count: 0,
            layout_requested: false,
            visibility: Visibility::Visible,
            enabled: true,
            clickable: false,
            alpha: 1.0,
            translation_x: 0.0,
            translation_y: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            pivot_x: 0.0,
            pivot_y: 0.0,
            rotation: 0.0,
            padding_left: 0.0,
            padding_top: 0.0,
            padding_right: 0.0,
            padding_bottom: 0.0,
            background_color: None,
            gravity: Gravity::NO_GRAVITY,
            content_description: String::new(),
            a11y_role: crate::ui::a11y::A11yRole::None,
            a11y_checked: false,
            a11y_dirty: true,
            a11y_action_tag: -1,
        }
    }

    /// 进程内唯一标识（手势注册表键；永不复用）。
    pub fn unique_id(&self) -> u64 {
        self.id
    }

    // --- 几何 getter（layout 派生）---

    /// 左边界的 x。
    pub fn left(&self) -> f32 {
        self.left
    }
    /// 顶边的 y。
    pub fn top(&self) -> f32 {
        self.top
    }
    /// 右边界的 x。
    pub fn right(&self) -> f32 {
        self.right
    }
    /// 底边的 y。
    pub fn bottom(&self) -> f32 {
        self.bottom
    }
    /// 布局宽度（right - left）。
    pub fn width(&self) -> f32 {
        self.right - self.left
    }
    /// 布局高度（bottom - top）。
    pub fn height(&self) -> f32 {
        self.bottom - self.top
    }

    // --- 测量结果 getter ---

    /// 测量宽度（onMeasure 设定）。
    pub fn measured_width(&self) -> f32 {
        self.measured_width
    }
    /// 测量高度。
    pub fn measured_height(&self) -> f32 {
        self.measured_height
    }

    // --- 属性 setter/getter ---

    /// 可见性。
    pub fn visibility(&self) -> Visibility {
        self.visibility
    }
    /// 设置可见性。
    pub fn set_visibility(&mut self, v: Visibility) {
        self.visibility = v;
    }
    /// 是否启用。
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
    /// 设置启用状态。
    pub fn set_enabled(&mut self, e: bool) {
        self.enabled = e;
    }
    /// 是否可点击。
    pub fn is_clickable(&self) -> bool {
        self.clickable
    }
    /// 设置可点击。
    pub fn set_clickable(&mut self, c: bool) {
        self.clickable = c;
        self.a11y_dirty = true;
    }

    /// 内容描述（TalkBack 优先播报此文本）。
    pub fn content_description(&self) -> &str {
        &self.content_description
    }
    /// 设置内容描述（无障碍名称）。
    pub fn set_content_description(&mut self, desc: impl Into<String>) {
        self.content_description = desc.into();
        self.a11y_dirty = true;
    }
    /// 无障碍角色。
    pub fn a11y_role(&self) -> crate::ui::a11y::A11yRole {
        self.a11y_role
    }
    /// 设置无障碍角色（同时影响 className 与是否参与聚焦遍历）。
    pub fn set_a11y_role(&mut self, role: crate::ui::a11y::A11yRole) {
        self.a11y_role = role;
        self.a11y_dirty = true;
    }
    /// 是否被选中（复选框语义）。
    pub fn a11y_checked(&self) -> bool {
        self.a11y_checked
    }
    /// 设置选中态。
    pub fn set_a11y_checked(&mut self, checked: bool) {
        if self.a11y_checked != checked {
            self.a11y_checked = checked;
            self.a11y_dirty = true;
        }
    }
    /// 无障碍快照是否需要更新。
    pub fn is_a11y_dirty(&self) -> bool {
        self.a11y_dirty
    }
    /// 清除无障碍脏标记（快照生成后调用）。
    pub fn clear_a11y_dirty(&mut self) {
        self.a11y_dirty = false;
    }
    /// 无障碍动作标签（宿主自定义可点击目标标识；-1 = 无）。
    pub fn a11y_action_tag(&self) -> i32 {
        self.a11y_action_tag
    }
    /// 设置无障碍动作标签（宿主装配时设置，框架原样回传）。
    pub fn set_a11y_action_tag(&mut self, tag: i32) {
        if self.a11y_action_tag != tag {
            self.a11y_action_tag = tag;
            self.a11y_dirty = true;
        }
    }
    /// 透明度 [0,1]。
    pub fn alpha(&self) -> f32 {
        self.alpha
    }
    /// 设置透明度。
    pub fn set_alpha(&mut self, a: f32) {
        self.alpha = a;
    }
    /// x 平移。
    pub fn translation_x(&self) -> f32 {
        self.translation_x
    }
    /// 设置 x 平移。
    pub fn set_translation_x(&mut self, t: f32) {
        self.translation_x = t;
    }
    /// y 平移。
    pub fn translation_y(&self) -> f32 {
        self.translation_y
    }
    /// 设置 y 平移。
    pub fn set_translation_y(&mut self, t: f32) {
        self.translation_y = t;
    }
    /// x 缩放。
    pub fn scale_x(&self) -> f32 {
        self.scale_x
    }
    /// 设置 x 缩放。
    pub fn set_scale_x(&mut self, s: f32) {
        self.scale_x = s;
    }
    /// y 缩放。
    pub fn scale_y(&self) -> f32 {
        self.scale_y
    }
    /// 设置 y 缩放。
    pub fn set_scale_y(&mut self, s: f32) {
        self.scale_y = s;
    }
    /// 旋转轴心 x。
    pub fn pivot_x(&self) -> f32 {
        self.pivot_x
    }
    /// 设置旋转轴心 x。
    pub fn set_pivot_x(&mut self, p: f32) {
        self.pivot_x = p;
    }
    /// 旋转轴心 y。
    pub fn pivot_y(&self) -> f32 {
        self.pivot_y
    }
    /// 设置旋转轴心 y。
    pub fn set_pivot_y(&mut self, p: f32) {
        self.pivot_y = p;
    }
    /// 旋转角（度）。
    pub fn rotation(&self) -> f32 {
        self.rotation
    }
    /// 设置旋转角。
    pub fn set_rotation(&mut self, r: f32) {
        self.rotation = r;
    }
    /// 左内边距。
    pub fn padding_left(&self) -> f32 {
        self.padding_left
    }
    /// 顶内边距。
    pub fn padding_top(&self) -> f32 {
        self.padding_top
    }
    /// 右内边距。
    pub fn padding_right(&self) -> f32 {
        self.padding_right
    }
    /// 底内边距。
    pub fn padding_bottom(&self) -> f32 {
        self.padding_bottom
    }
    /// 设置四向内边距。
    pub fn set_padding(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.padding_left = left;
        self.padding_top = top;
        self.padding_right = right;
        self.padding_bottom = bottom;
    }
    /// 按 dp 设置四向内边距（随设备密度缩放）。
    ///
    /// # 为什么提供
    /// 布局尺寸的设计语义是 dp；若调用方随手传 px，同一份代码在高密度
    /// 设备上内边距会缩到几乎不可见。走此入口可保证跨设备一致。
    pub fn set_padding_dp(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        let dp = crate::ui::dimen::dp;
        self.set_padding(dp(left), dp(top), dp(right), dp(bottom));
    }
    /// 按 dp 设置水平内边距（垂直不变）。
    pub fn set_padding_horizontal_dp(&mut self, left: f32, right: f32) {
        let dp = crate::ui::dimen::dp;
        self.padding_left = dp(left);
        self.padding_right = dp(right);
    }
    /// 背景色（`None` = 无背景）。
    pub fn background_color(&self) -> Option<Color> {
        self.background_color
    }
    /// 设置背景色。
    pub fn set_background_color(&mut self, c: Option<Color>) {
        self.background_color = c;
    }
    /// 内容对齐。
    pub fn gravity(&self) -> Gravity {
        self.gravity
    }
    /// 设置内容对齐。
    pub fn set_gravity(&mut self, g: Gravity) {
        self.gravity = g;
    }

    // --- 生命周期 ---

    /// 测量入口：spec 一致且未强制时直接返回（不变量）。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        let same =
            self.old_width_spec == Some(width_spec) && self.old_height_spec == Some(height_spec);
        // Android 判定：!(mPrivateFlags & PFLAG_LAYOUT_REQUIRED) 时同 spec 跳过；
        // forceLayout 置 LAYOUT_REQUIRED。这里 layout_requested 兼任该标志。
        if same && !self.layout_requested {
            return;
        }
        self.old_width_spec = Some(width_spec);
        self.old_height_spec = Some(height_spec);
        self.on_measure_count += 1;
        self.on_measure(width_spec, height_spec);
    }

    /// 强制重新测量/布局。
    pub fn force_layout(&mut self) {
        self.layout_requested = true;
    }

    /// 请求布局（置脏，合并到下帧）。
    pub fn request_layout(&mut self) {
        self.layout_requested = true;
    }

    /// 是否有待处理的布局请求。
    pub fn is_layout_requested(&self) -> bool {
        self.layout_requested
    }

    /// onMeasure 回调计数（测试不变量用）。
    pub fn on_measure_count(&self) -> u32 {
        self.on_measure_count
    }

    /// 默认 onMeasure：按 spec 设定测量尺寸（无内容基类）。
    ///
    /// - EXACTLY：取 spec 尺寸；
    /// - AT_MOST / UNSPECIFIED：无内容时取 0（子类覆盖后按内容收缩）。
    pub fn on_measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        let w = resolve_default(width_spec);
        let h = resolve_default(height_spec);
        self.set_measured_dimension(w, h);
    }

    /// 设定测量结果（Android `setMeasuredDimension`）。
    pub fn set_measured_dimension(&mut self, width: f32, height: f32) {
        self.measured_width = width;
        self.measured_height = height;
    }

    /// 布局：写入最终几何并清脏标志。
    pub fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        // 几何变化必须让无障碍快照失效（boundsInScreen 已变）。
        if self.left != left || self.top != top || self.right != right || self.bottom != bottom {
            self.a11y_dirty = true;
        }
        self.left = left;
        self.top = top;
        self.right = right;
        self.bottom = bottom;
        self.layout_requested = false;
    }

    /// 绘制入口：背景 → onDraw。
    pub fn draw(&self, scene: &mut Scene) {
        if self.visibility == Visibility::Gone {
            return;
        }
        self.on_draw_background(scene);
        self.on_draw(scene);
    }

    /// 背景绘制：纯色矩形覆盖自身 bounds。
    fn on_draw_background(&self, scene: &mut Scene) {
        if let Some(color) = self.background_color {
            let rect = crate::gfx::scene::Rect {
                x: f64::from(self.left + self.translation_x),
                y: f64::from(self.top + self.translation_y),
                w: f64::from(self.width()),
                h: f64::from(self.height()),
            };
            let paint = crate::gfx::paint::Paint::default().color(color);
            scene
                .commands
                .push(crate::gfx::scene::Command::FillRect { rect, paint });
        }
    }

    /// 默认 onDraw：基类无内容（子类扩展点，V1 由 widgets 覆盖）。
    pub fn on_draw(&self, scene: &mut Scene) {
        let _ = scene;
    }
}

/// 默认尺寸解析：EXACTLY 取 spec，其余无内容取 0。
fn resolve_default(spec: MeasureSpec) -> f32 {
    match spec.mode() {
        Mode::Exactly => spec.size() as f32,
        Mode::AtMost | Mode::Unspecified => 0.0,
    }
}
