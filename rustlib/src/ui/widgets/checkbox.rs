//! M3 复选框（Material 3 Expressive）。
//!
//! # 设计
//! - 18dp 方框（2dp 圆角），选中时 primary 底 + 白勾路径；
//! - 勾为 SVG 路径多段线近似（无图标字体依赖）；
//! - 状态层：按压时叠 on_surface 10%。

use crate::gfx::color::Color;
use crate::gfx::scene::Scene;
use crate::ui::dimen::dp;
use crate::ui::measure_spec::MeasureSpec;
use crate::ui::theme;
use crate::ui::view::View;
use crate::ui::widgets::widget::Widget;

/// 复选框边框尺寸（M3 规范 18dp）。
pub const CHECKBOX_SIZE: f32 = 18.0;

/// M3 复选框。
pub struct Checkbox {
    /// 基础视图（尺寸/边距/状态）。
    pub view: View,
    /// 是否选中。
    checked: bool,
    /// 按压态（状态层）。
    pressed: bool,
}

impl Checkbox {
    /// 创建未选中复选框。
    pub fn new() -> Self {
        let mut view = View::new();
        // 触控目标 48dp（M3 无障碍最小可点区域）。
        view.set_padding_dp(15.0, 15.0, 15.0, 15.0);
        view.set_clickable(true);
        view.set_a11y_role(crate::ui::a11y::A11yRole::Checkbox);
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

    /// 设置选中态（触发重布局）。
    pub fn set_checked(&mut self, checked: bool) {
        self.checked = checked;
        self.view.set_a11y_checked(checked);
        self.view.request_layout();
    }

    /// 切换选中态。
    pub fn toggle(&mut self) {
        self.set_checked(!self.checked);
    }

    /// 测量：固定 48dp 触控目标。
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

    /// 绘制：方框/填充 + 勾。
    pub fn draw(&self, scene: &mut Scene) {
        let t = theme::theme();
        let pad = dp(15.0);
        let x = self.view.left() + pad;
        let y = self.view.top() + pad;
        let s = dp(CHECKBOX_SIZE);
        let r = dp(2.0);

        let (fill, stroke) = if self.checked {
            (Some(t.roles.primary), None)
        } else {
            (None, Some(t.roles.on_surface_variant))
        };
        // 框体：选中 = primary 填充；未选中 = outline 描边。
        let mut path = crate::gfx::path::Path::rounded_rect(
            f64::from(x),
            f64::from(y),
            f64::from(s),
            f64::from(s),
            f64::from(r),
        );
        if let Some(c) = fill {
            scene.commands.push(crate::gfx::scene::Command::FillPath {
                path: path.clone(),
                paint: crate::gfx::paint::Paint::default().color(c),
            });
        }
        if let Some(c) = stroke {
            scene.commands.push(crate::gfx::scene::Command::StrokePath {
                path,
                paint: crate::gfx::paint::Paint::default()
                    .color(c)
                    .stroke_width(dp(2.0)),
            });
        }
        // 勾：两段线（9,4)-(4,9)-(16,-1 相对方框原点，dp 换算。
        if self.checked {
            path = crate::gfx::path::Path::new();
            let ox = f64::from(x);
            let oy = f64::from(y);
            path.move_to(ox + f64::from(dp(5.0)), oy + f64::from(dp(10.0)));
            path.line_to(ox + f64::from(dp(3.0)), oy + f64::from(dp(8.0)));
            path.line_to(ox + f64::from(dp(7.0)), oy + f64::from(dp(4.0)));
            scene.commands.push(crate::gfx::scene::Command::StrokePath {
                path,
                paint: crate::gfx::paint::Paint::default()
                    .color(t.roles.on_primary)
                    .stroke_width(dp(2.0)),
            });
        }
        // 状态层。
        if self.pressed {
            let bg = t.roles.surface;
            let overlay = if self.checked {
                t.roles.on_primary
            } else {
                t.roles.on_surface
            };
            theme::draw_state_layer(
                scene,
                crate::gfx::scene::Rect {
                    x: f64::from(x),
                    y: f64::from(y),
                    w: f64::from(s),
                    h: f64::from(s),
                },
                bg,
                overlay,
                true,
                false,
                r,
            );
        }
    }
}

impl Default for Checkbox {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Checkbox {
    fn view(&self) -> &View {
        &self.view
    }
    fn view_mut(&mut self) -> &mut View {
        &mut self.view
    }
    fn measure(&mut self, ws: MeasureSpec, hs: MeasureSpec) {
        Checkbox::measure(self, ws, hs)
    }
    fn layout(&mut self, l: f32, t: f32, r: f32, b: f32) {
        Checkbox::layout(self, l, t, r, b)
    }
    fn draw(&self, scene: &mut Scene) {
        Checkbox::draw(self, scene)
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
                    self.toggle();
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

    /// 点击切换选中态。
    #[test]
    fn toggle_on_tap() {
        let mut c = Checkbox::new();
        assert!(!c.is_checked());
        c.toggle();
        assert!(c.is_checked());
    }

    /// 选中态写入无障碍属性。
    #[test]
    fn checked_syncs_a11y() {
        let mut c = Checkbox::new();
        c.set_checked(true);
        assert!(c.view.a11y_checked());
    }

    /// 固定 48dp 触控目标。
    #[test]
    fn measures_touch_target() {
        use crate::ui::measure_spec::Mode;
        let mut c = Checkbox::new();
        c.measure(
            MeasureSpec::make(Mode::AtMost, 200),
            MeasureSpec::make(Mode::AtMost, 200),
        );
        assert_eq!(c.view.measured_width(), dp(48.0));
    }
}
