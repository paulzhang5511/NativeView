//! M3 分隔线（Material 3）。
//!
//! 纯展示组件：1dp `outline_variant` 水平线，支持左右 inset。

use crate::gfx::scene::Scene;
use crate::ui::dimen::dp;
use crate::ui::measure_spec::MeasureSpec;
use crate::ui::theme;
use crate::ui::view::View;
use crate::ui::widgets::widget::Widget;

/// M3 分隔线。
pub struct Divider {
    /// 基础视图。
    pub view: View,
    /// 起始 inset（dp）。
    inset_start: f32,
    /// 结束 inset（dp）。
    inset_end: f32,
}

impl Divider {
    /// 创建全宽分隔线。
    pub fn new() -> Self {
        let mut view = View::new();
        view.set_padding_dp(0.0, 8.0, 0.0, 8.0);
        Self {
            view,
            inset_start: 0.0,
            inset_end: 0.0,
        }
    }

    /// 设置 inset（dp）。
    pub fn set_inset(&mut self, start_dp: f32, end_dp: f32) {
        self.inset_start = start_dp;
        self.inset_end = end_dp;
        self.view.request_layout();
    }

    /// 测量：全宽 1dp 线 + 8dp 垂直留白。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        let w = match width_spec.mode() {
            crate::ui::measure_spec::Mode::Exactly => width_spec.size() as f32,
            _ => dp(100.0),
        };
        let h = match height_spec.mode() {
            crate::ui::measure_spec::Mode::Exactly => height_spec.size() as f32,
            _ => dp(16.0),
        };
        self.view.set_measured_dimension(w.max(0.0), h.max(0.0));
    }

    /// 布局。
    pub fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.view.layout(left, top, right, bottom);
    }

    /// 绘制：居中 1dp 线。
    pub fn draw(&self, scene: &mut Scene) {
        let t = theme::theme();
        let y = self.view.top() + self.view.height() / 2.0;
        let x0 = self.view.left() + dp(self.inset_start);
        let x1 = self.view.right() - dp(self.inset_end);
        if x1 <= x0 {
            return;
        }
        scene.commands.push(crate::gfx::scene::Command::FillRect {
            rect: crate::gfx::scene::Rect {
                x: f64::from(x0),
                y: f64::from(y),
                w: f64::from(x1 - x0),
                h: f64::from(dp(1.0)),
            },
            paint: crate::gfx::paint::Paint::default().color(t.roles.outline_variant),
        });
    }
}

impl Default for Divider {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Divider {
    fn view(&self) -> &View {
        &self.view
    }
    fn view_mut(&mut self) -> &mut View {
        &mut self.view
    }
    fn measure(&mut self, ws: MeasureSpec, hs: MeasureSpec) {
        Divider::measure(self, ws, hs)
    }
    fn layout(&mut self, l: f32, t: f32, r: f32, b: f32) {
        Divider::layout(self, l, t, r, b)
    }
    fn draw(&self, scene: &mut Scene) {
        Divider::draw(self, scene)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// inset 收窄线条宽度。
    #[test]
    fn inset_narrows_line() {
        let mut d = Divider::new();
        d.set_inset(16.0, 16.0);
        d.measure(
            MeasureSpec::make(crate::ui::measure_spec::Mode::Exactly, 100),
            MeasureSpec::make(crate::ui::measure_spec::Mode::Exactly, 16),
        );
        d.layout(0.0, 0.0, 100.0, 16.0);
        let mut scene = Scene::new(100, 16);
        d.draw(&mut scene);
        // 找到 FillRect 命令验证宽度（100 - 32 = 68）。
        for cmd in &scene.commands {
            if let crate::gfx::scene::Command::FillRect { rect, .. } = cmd {
                assert!((rect.w - f64::from(dp(68.0))).abs() < 1e-6);
            }
        }
    }
}
