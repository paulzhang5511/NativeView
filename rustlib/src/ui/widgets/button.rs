//! M3 按钮控件（Material 3 Expressive）。
//!
//! # 设计
//! - 三变体：Filled（primary 底）/ Outlined（描边）/ Text（无底）；
//! - Expressive 形状：默认药丸形（full），按压时 morph 到 60% 高度圆角
//!   （Expressive 标志性按压形变，V1 为两态静态切换，动画在 P2 接入）；
//! - 状态层：替代旧版「整体压暗」，用 on_primary 色按 10% 叠加
//!   （M3 stateLayer 规范，按压与悬浮的视觉一致）。
//!
//! # 边界
//! Ripple 扩散动画不在 V1 范围（与主规格差异表登记）。

use super::text_view::TextView;
use crate::gfx::color::Color;
use crate::gfx::scene::Scene;
use crate::ui::measure_spec::MeasureSpec;
use crate::ui::theme::{self, shape::ShapeToken};

/// 按钮变体（M3 三型 + 容器扩展位）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    /// 实心（primary 底 on_primary 字，最高强调）。
    Filled,
    /// 描边（outline 边框 surface 底）。
    Outlined,
    /// 纯文本（无底无框，低强调）。
    Text,
}

/// M3 按钮。
pub struct Button {
    /// 文本控件（承载排版与文本绘制）。
    pub text_view: TextView,
    /// 变体（决定配色与形状的默认值）。
    variant: ButtonVariant,
    /// 形状令牌（默认 Full 药丸；可按容器需求覆盖）。
    shape: ShapeToken,
    /// 按压态（触发状态层与形状 morph）。
    pressed: bool,
}

impl Button {
    /// 创建 M3 Filled 按钮（primary 底药丸形）。
    pub fn new() -> Self {
        Self::with_variant(ButtonVariant::Filled)
    }

    /// 创建指定变体按钮（从主题取配色）。
    pub fn with_variant(variant: ButtonVariant) -> Self {
        let t = theme::theme();
        let mut text_view = TextView::new();
        // 字阶：label_large（14sp/20sp，M3 按钮 token）。
        text_view.set_text_style(typography_token());
        text_view.set_text_color(variant_text_color(variant, &t));
        // 内容边距：水平 24dp 垂直 10dp（M3 按钮内边距规范）。
        text_view.view.set_padding_dp(24.0, 10.0, 24.0, 10.0);
        Self {
            text_view,
            variant,
            shape: ShapeToken::Full,
            pressed: false,
        }
    }

    /// 设置文本。
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text_view.set_text(text);
    }

    /// 注入排版上下文（无字体时文本降级为空）。
    pub fn set_context(&mut self, cx: crate::text::layout::TextContext) {
        self.text_view.set_context(cx);
    }

    /// 当前文本。
    pub fn text(&self) -> &str {
        self.text_view.text()
    }

    /// 按压态。
    pub fn is_pressed(&self) -> bool {
        self.pressed
    }

    /// 设置按压态（仅状态位；重绘由输入层消费 Touch 后统一触发）。
    pub fn set_pressed(&mut self, pressed: bool) {
        self.pressed = pressed;
    }

    /// 测量（委托文本测量 + 语义扩边）。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        self.text_view.measure(width_spec, height_spec);
    }

    /// 布局。
    pub fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.text_view.layout(left, top, right, bottom);
    }

    /// 实际背景色（含状态层叠加）。
    fn effective_background(&self) -> Color {
        let t = theme::theme();
        let (bg, overlay) = match self.variant {
            ButtonVariant::Filled => (t.roles.primary, t.roles.on_primary),
            ButtonVariant::Outlined | ButtonVariant::Text => (t.roles.surface, t.roles.on_surface),
        };
        if self.pressed {
            theme::state_layer(bg, overlay, theme::state::PRESSED_ALPHA)
        } else {
            bg
        }
    }

    /// 实际圆角：Full 令牌在按压时 morph 到高度 60% 圆角
    /// （Expressive 按压形变：药丸 → 圆角矩形）。
    fn effective_radius(&self) -> f32 {
        match self.shape {
            ShapeToken::Full => {
                let h = self.text_view.view.height();
                if self.pressed { h * 0.30 } else { h * 0.5 }
            }
            other => other.radius_dp(half_height(&self.text_view.view)),
        }
    }

    /// 绘制：背景（Filled）+ 状态层 + 描边（Outlined）+ 文本。
    pub fn draw(&self, scene: &mut Scene) {
        let v = &self.text_view.view;
        let t = theme::theme();
        let rect = crate::gfx::scene::Rect {
            x: f64::from(v.left()),
            y: f64::from(v.top()),
            w: f64::from(v.width()),
            h: f64::from(v.height()),
        };
        let radius = self.effective_radius();
        let bg = self.effective_background();

        // Filled 变体画底；Outlined/Text 无底（状态层直接叠 surface）。
        if self.variant == ButtonVariant::Filled {
            scene.commands.push(crate::gfx::scene::Command::FillPath {
                path: crate::gfx::path::Path::rounded_rect(
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    f64::from(radius),
                ),
                paint: crate::gfx::paint::Paint::default().color(bg),
            });
        } else if self.pressed {
            // 无底变体的按压态：surface 底叠 on_surface 状态层。
            scene.commands.push(crate::gfx::scene::Command::FillPath {
                path: crate::gfx::path::Path::rounded_rect(
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    f64::from(radius),
                ),
                paint: crate::gfx::paint::Paint::default().color(bg),
            });
        }

        // Outlined 变体：1dp 描边。
        if self.variant == ButtonVariant::Outlined {
            let outline = t.roles.outline;
            scene.commands.push(crate::gfx::scene::Command::StrokePath {
                path: crate::gfx::path::Path::rounded_rect(
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    f64::from(radius),
                ),
                paint: crate::gfx::paint::Paint::default()
                    .color(outline)
                    .stroke_width(crate::ui::dimen::dp(1.0)),
            });
        }

        self.text_view.draw(scene);
    }
}

/// 文本色按变体取主题角色。
fn variant_text_color(variant: ButtonVariant, t: &theme::Theme) -> Color {
    match variant {
        ButtonVariant::Filled => t.roles.on_primary,
        ButtonVariant::Outlined | ButtonVariant::Text => t.roles.primary,
    }
}

/// 按钮字阶令牌（M3 label_large）。
fn typography_token() -> crate::ui::theme::typography::TypeToken {
    crate::ui::theme::typography::SCALE.label_large
}

/// 控件高度一半（Full 令牌解析基准）。
fn half_height(v: &crate::ui::view::View) -> f32 {
    v.height() / 2.0
}

impl Default for Button {
    fn default() -> Self {
        Self::new()
    }
}

/// Button 的 Widget 实现（叶子）。
impl crate::ui::widgets::widget::Widget for Button {
    fn view(&self) -> &crate::ui::view::View {
        &self.text_view.view
    }
    fn view_mut(&mut self) -> &mut crate::ui::view::View {
        &mut self.text_view.view
    }
    fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        Button::measure(self, width_spec, height_spec)
    }
    fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        Button::layout(self, left, top, right, bottom)
    }
    fn draw(&self, scene: &mut Scene) {
        Button::draw(self, scene)
    }

    /// 触摸：DOWN 置按压态、UP/CANCEL 复位。
    ///
    /// # 返回
    /// 手势序列内的所有事件都消费（按下即视为本按钮接管），
    /// 避免父容器在同序列中再次分发。
    fn dispatch_touch(&mut self, event: &crate::ui::input::MotionEvent) -> bool {
        match event.action() {
            crate::ui::input::Action::Down => {
                self.pressed = true;
                true
            }
            crate::ui::input::Action::Move => self.pressed,
            crate::ui::input::Action::Up | crate::ui::input::Action::Cancel => {
                self.pressed = false;
                true
            }
            _ => false,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::input::{Action, MotionEvent, PointerCoords};

    fn event(action: i32) -> MotionEvent {
        MotionEvent {
            packed_action: action,
            down_time: 0,
            event_time: 0,
            pointers: vec![PointerCoords {
                pointer_id: 0,
                x: 0.0,
                y: 0.0,
                pressure: 0.5,
            }],
            historical: vec![],
        }
    }

    /// 按压态：DOWN 置位（状态层叠加）、UP 复位。
    #[test]
    fn press_state_toggles_on_touch() {
        use crate::ui::widgets::widget::Widget;
        let mut b = Button::new();
        let normal = b.effective_background();
        assert!(b.dispatch_touch(&event(Action::DOWN)));
        assert!(b.is_pressed());
        assert_ne!(b.effective_background(), normal);
        assert!(b.dispatch_touch(&event(Action::UP)));
        assert!(!b.is_pressed());
        assert_eq!(b.effective_background(), normal);
    }

    /// 默认形状是 Expressive 药丸（Full 令牌）。
    #[test]
    fn default_shape_is_full_pill() {
        let b = Button::new();
        assert_eq!(b.shape, ShapeToken::Full);
    }

    /// Outlined 变体不带填充底色。
    #[test]
    fn outlined_has_surface_background() {
        let t = theme::theme();
        let b = Button::with_variant(ButtonVariant::Outlined);
        assert_eq!(b.effective_background(), t.roles.surface);
    }
}
