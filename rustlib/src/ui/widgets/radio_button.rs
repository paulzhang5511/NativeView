//! M3 单选按钮（Material 3 Expressive）。
//!
//! # 设计
//! - 20dp 外环（2dp 描边），选中时 primary 描边 + 内点 primary；
//! - 状态层与 Checkbox 同构；
//! - V1 无互斥组逻辑（组语义由容器组合实现，组件只管自身选中态）。

use crate::gfx::color::Color;
use crate::gfx::scene::Scene;
use crate::ui::dimen::dp;
use crate::ui::measure_spec::MeasureSpec;
use crate::ui::theme;
use crate::ui::view::View;
use crate::ui::widgets::widget::Widget;

/// 单选按钮外环尺寸（M3 规范 20dp）。
pub const RADIO_SIZE: f32 = 20.0;

/// M3 单选按钮。
pub struct RadioButton {
    /// 基础视图。
    pub view: View,
    /// 是否选中。
    checked: bool,
    /// 按压态（状态层）。
    pressed: bool,
}

impl RadioButton {
    /// 创建未选中单选按钮。
    pub fn new() -> Self {
        let mut view = View::new();
        view.set_padding_dp(14.0, 14.0, 14.0, 14.0);
        view.set_clickable(true);
        view.set_a11y_role(crate::ui::a11y::A11yRole::RadioButton);
        Self {
            view,
            checked: false,
            pressed: false,
        }
    }

    /// 是否选中。
    pub fn is_checked(&self) -> bool {
        self.checked
    }

    /// 设置选中态。
    pub fn set_checked(&mut self, checked: bool) {
        self.checked = checked;
        self.view.set_a11y_checked(checked);
        self.view.request_layout();
    }

    /// 测量：48dp 触控目标。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        let size = dp(48.0);
        let w = match width_spec.mode() {
            crate::ui::measure_spec::Mode::Exactly => width_spec.size() as f32,
            _ => size,
        };
        let h = match height_spec.mode() {
            crate::ui::measure_spec::Mode::Exactly => height_spec.size() as f32,
            _ => size,
        };
        self.view.set_measured_dimension(w.max(0.0), h.max(0.0));
    }

    /// 布局。
    pub fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.view.layout(left, top, right, bottom);
    }

    /// 绘制：外环 + 内点。
    pub fn draw(&self, scene: &mut Scene) {
        let t = theme::theme();
        let pad = dp(14.0);
        let x = self.view.left() + pad;
        let y = self.view.top() + pad;
        let s = dp(RADIO_SIZE);
        let radius = s / 2.0;
        let cx = f64::from(x + radius);
        let cy = f64::from(y + radius);

        // 外环（2dp 描边圆）。
        let ring = crate::gfx::path::Path::circle(cx, cy, f64::from(radius - dp(1.0)));
        let ring_color = if self.checked {
            t.roles.primary
        } else {
            t.roles.on_surface_variant
        };
        scene.commands.push(crate::gfx::scene::Command::StrokePath {
            path: ring,
            paint: crate::gfx::paint::Paint::default()
                .color(ring_color)
                .stroke_width(dp(2.0)),
        });

        // 选中内点（半径的一半，M3 规范）。
        if self.checked {
            let dot_r = f64::from(radius - dp(6.0));
            let dot = crate::gfx::path::Path::circle(cx, cy, dot_r);
            scene.commands.push(crate::gfx::scene::Command::FillPath {
                path: dot,
                paint: crate::gfx::paint::Paint::default().color(t.roles.primary),
            });
        }

        // 状态层。
        if self.pressed {
            let overlay = if self.checked {
                t.roles.primary
            } else {
                t.roles.on_surface
            };
            let bg = t.roles.surface;
            let color = theme::state_layer(bg, overlay, theme::state::PRESSED_ALPHA);
            scene.commands.push(crate::gfx::scene::Command::FillPath {
                path: crate::gfx::path::Path::circle(cx, cy, f64::from(radius + dp(4.0))),
                paint: crate::gfx::paint::Paint::default().color(color),
            });
        }
    }
}

impl Default for RadioButton {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for RadioButton {
    fn view(&self) -> &View {
        &self.view
    }
    fn view_mut(&mut self) -> &mut View {
        &mut self.view
    }
    fn measure(&mut self, ws: MeasureSpec, hs: MeasureSpec) {
        RadioButton::measure(self, ws, hs)
    }
    fn layout(&mut self, l: f32, t: f32, r: f32, b: f32) {
        RadioButton::layout(self, l, t, r, b)
    }
    fn draw(&self, scene: &mut Scene) {
        RadioButton::draw(self, scene)
    }
    fn dispatch_touch(&mut self, event: &crate::ui::input::MotionEvent) -> bool {
        match event.action() {
            crate::ui::input::Action::Down => {
                self.pressed = true;
                true
            }
            crate::ui::input::Action::Move => self.pressed,
            crate::ui::input::Action::Up => {
                if self.pressed {
                    self.set_checked(true);
                }
                self.pressed = false;
                true
            }
            crate::ui::input::Action::Cancel => {
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

    /// 点击置选中（单选语义：只置位不清位，互斥由容器管）。
    #[test]
    fn tap_selects() {
        let mut r = RadioButton::new();
        assert!(!r.is_checked());
        r.set_checked(true);
        assert!(r.is_checked());
    }
}
