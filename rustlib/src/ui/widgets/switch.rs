//! M3 开关（Material 3 Expressive）。
//!
//! # 设计
//! - M3 Expressive 宽轨：52×32dp，拇指 24dp；
//! - 选中 primary 轨；未选中 `surface_container_highest` 轨 + `outline` 边；
//! - 按压形变：轨宽压缩 4dp（Expressive 标志动态，V1 两态静态切换）。

use crate::gfx::color::Color;
use crate::gfx::scene::Scene;
use crate::ui::dimen::dp;
use crate::ui::measure_spec::MeasureSpec;
use crate::ui::theme;
use crate::ui::view::View;
use crate::ui::widgets::widget::Widget;

/// 轨宽（M3 Expressive 52dp）。
pub const TRACK_WIDTH: f32 = 52.0;
/// 轨高（M3 Expressive 32dp）。
pub const TRACK_HEIGHT: f32 = 32.0;

/// M3 开关。
pub struct Switch {
    /// 基础视图。
    pub view: View,
    /// 是否开启。
    checked: bool,
    /// 按压态。
    pressed: bool,
}

impl Switch {
    /// 创建关闭状态的开关。
    pub fn new() -> Self {
        let mut view = View::new();
        view.set_padding_dp(8.0, 8.0, 8.0, 8.0);
        view.set_clickable(true);
        view.set_a11y_role(crate::ui::a11y::A11yRole::Switch);
        Self {
            view,
            checked: false,
            pressed: false,
        }
    }

    /// 是否开启。
    pub fn is_checked(&self) -> bool {
        self.checked
    }

    /// 设置开关态。
    pub fn set_checked(&mut self, checked: bool) {
        self.checked = checked;
        self.view.set_a11y_checked(checked);
        self.view.request_layout();
    }

    /// 切换。
    pub fn toggle(&mut self) {
        self.set_checked(!self.checked);
    }

    /// 测量：52×32dp 轨 + 8dp 触控扩边 = 68×48dp。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        let w = match width_spec.mode() {
            crate::ui::measure_spec::Mode::Exactly => width_spec.size() as f32,
            _ => dp(TRACK_WIDTH + 16.0),
        };
        let h = match height_spec.mode() {
            crate::ui::measure_spec::Mode::Exactly => height_spec.size() as f32,
            _ => dp(TRACK_HEIGHT + 16.0),
        };
        self.view.set_measured_dimension(w.max(0.0), h.max(0.0));
    }

    /// 布局。
    pub fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.view.layout(left, top, right, bottom);
    }

    /// 绘制：轨（药丸）+ 拇指（圆）。
    pub fn draw(&self, scene: &mut Scene) {
        let t = theme::theme();
        let pad = dp(8.0);
        // 按压形变：轨宽收缩 4dp（Expressive 动态近似）。
        let shrink = if self.pressed { dp(4.0) } else { 0.0 };
        let track_w = dp(TRACK_WIDTH) - shrink;
        let track_h = dp(TRACK_HEIGHT);
        let x = self.view.left() + pad + shrink / 2.0;
        let y = self.view.top() + pad;

        // 轨。
        let track_color = if self.checked {
            t.roles.primary
        } else {
            t.roles.surface_container_highest
        };
        scene.commands.push(crate::gfx::scene::Command::FillPath {
            path: crate::gfx::path::Path::rounded_rect(
                f64::from(x),
                f64::from(y),
                f64::from(track_w),
                f64::from(track_h),
                f64::from(track_h / 2.0),
            ),
            paint: crate::gfx::paint::Paint::default().color(track_color),
        });
        // 未选中时轨加 outline 描边。
        if !self.checked {
            scene.commands.push(crate::gfx::scene::Command::StrokePath {
                path: crate::gfx::path::Path::rounded_rect(
                    f64::from(x),
                    f64::from(y),
                    f64::from(track_w),
                    f64::from(track_h),
                    f64::from(track_h / 2.0),
                ),
                paint: crate::gfx::paint::Paint::default()
                    .color(t.roles.outline)
                    .stroke_width(dp(2.0)),
            });
        }

        // 拇指。
        let thumb_d = if self.checked { dp(24.0) } else { dp(16.0) };
        let track_r = track_h / 2.0;
        let thumb_cy = y + track_r;
        let thumb_cx = if self.checked {
            x + track_w - track_r
        } else {
            x + track_r
        };
        scene.commands.push(crate::gfx::scene::Command::FillPath {
            path: crate::gfx::path::Path::circle(
                f64::from(thumb_cx),
                f64::from(thumb_cy),
                f64::from(thumb_d / 2.0),
            ),
            paint: crate::gfx::paint::Paint::default().color(if self.checked {
                t.roles.on_primary
            } else {
                t.roles.outline
            }),
        });

        // 状态层（轨之上）。
        if self.pressed {
            let overlay = if self.checked {
                t.roles.on_primary
            } else {
                t.roles.on_surface
            };
            let color = theme::state_layer(track_color, overlay, theme::state::PRESSED_ALPHA);
            scene.commands.push(crate::gfx::scene::Command::FillPath {
                path: crate::gfx::path::Path::rounded_rect(
                    f64::from(x),
                    f64::from(y),
                    f64::from(track_w),
                    f64::from(track_h),
                    f64::from(track_r),
                ),
                paint: crate::gfx::paint::Paint::default().color(color),
            });
        }
    }
}

impl Default for Switch {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Switch {
    fn view(&self) -> &View {
        &self.view
    }
    fn view_mut(&mut self) -> &mut View {
        &mut self.view
    }
    fn measure(&mut self, ws: MeasureSpec, hs: MeasureSpec) {
        Switch::measure(self, ws, hs)
    }
    fn layout(&mut self, l: f32, t: f32, r: f32, b: f32) {
        Switch::layout(self, l, t, r, b)
    }
    fn draw(&self, scene: &mut Scene) {
        Switch::draw(self, scene)
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

    /// 切换开关态并同步无障碍。
    #[test]
    fn toggle_syncs_a11y() {
        let mut s = Switch::new();
        assert!(!s.is_checked());
        s.toggle();
        assert!(s.is_checked());
        assert!(s.view.a11y_checked());
    }

    /// 固定尺寸测量。
    #[test]
    fn measures_track_size() {
        use crate::ui::measure_spec::Mode;
        let mut s = Switch::new();
        s.measure(
            MeasureSpec::make(Mode::AtMost, 200),
            MeasureSpec::make(Mode::AtMost, 200),
        );
        assert_eq!(s.view.measured_width(), dp(TRACK_WIDTH + 16.0));
    }
}
