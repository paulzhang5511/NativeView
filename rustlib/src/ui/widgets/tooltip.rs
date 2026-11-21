//! M3 Tooltip（Material 3）。
//!
//! 长按弹出，`inverse_surface` 底 4dp 圆角，纯文字提示。

use crate::gfx::scene::Scene;
use crate::text::layout::TextContext;
use crate::ui::dimen::dp;
use crate::ui::measure_spec::MeasureSpec;
use crate::ui::theme;
use crate::ui::view::View;
use crate::ui::widgets::text_view::TextView;
use crate::ui::widgets::widget::Widget;

/// M3 Tooltip。
pub struct Tooltip {
    /// 基础视图。
    pub view: View,
    /// 文本控件。
    pub text_view: TextView,
    /// 是否可见。
    visible: bool,
}

impl Tooltip {
    /// 创建（默认隐藏）。
    pub fn new() -> Self {
        let mut text_view = TextView::new();
        text_view.set_text_style(theme::typography::SCALE.body_small);
        text_view.set_text_color(theme::theme().roles.inverse_on_surface);
        Self {
            view: View::new(),
            text_view,
            visible: false,
        }
    }

    /// 设置提示文本。
    pub fn set_text(&mut self, s: impl Into<String>) {
        self.text_view.set_text(s);
        self.view.request_layout();
    }

    /// 显示/隐藏。
    pub fn set_visible(&mut self, v: bool) {
        self.visible = v;
    }

    /// 是否可见。
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// 注入排版上下文。
    pub fn set_context(&mut self, cx: TextContext) {
        self.text_view.set_context(cx);
    }

    /// 测量：文本 + 8dp 水平 / 4dp 垂直内边距。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        self.text_view.measure(width_spec, height_spec);
        let tw = self.text_view.view.measured_width();
        let th = self.text_view.view.measured_height();
        self.view
            .set_measured_dimension(tw + dp(16.0), th + dp(8.0));
    }

    /// 布局。
    pub fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.view.layout(left, top, right, bottom);
        let tw = self.text_view.view.measured_width();
        let th = self.text_view.view.measured_height();
        let tx = left + dp(8.0);
        let ty = top + (bottom - top - th) / 2.0;
        self.text_view.layout(tx, ty, tx + tw, ty + th);
    }

    /// 绘制（隐藏时跳过）。
    pub fn draw(&self, scene: &mut Scene) {
        if !self.visible {
            return;
        }
        let t = theme::theme();
        scene.commands.push(crate::gfx::scene::Command::FillPath {
            path: crate::gfx::path::Path::rounded_rect(
                f64::from(self.view.left()),
                f64::from(self.view.top()),
                f64::from(self.view.width()),
                f64::from(self.view.height()),
                f64::from(dp(4.0)),
            ),
            paint: crate::gfx::paint::Paint::default().color(t.roles.inverse_surface),
        });
        self.text_view.draw(scene);
    }
}

impl Default for Tooltip {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Tooltip {
    fn view(&self) -> &View {
        &self.view
    }
    fn view_mut(&mut self) -> &mut View {
        &mut self.view
    }
    fn measure(&mut self, ws: MeasureSpec, hs: MeasureSpec) {
        Tooltip::measure(self, ws, hs)
    }
    fn layout(&mut self, l: f32, t: f32, r: f32, b: f32) {
        Tooltip::layout(self, l, t, r, b)
    }
    fn draw(&self, scene: &mut Scene) {
        Tooltip::draw(self, scene)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 隐藏时不绘制，可见时绘制。
    #[test]
    fn visibility_gates_draw() {
        let mut t = Tooltip::new();
        t.set_text("tip");
        t.measure(
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 200),
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 100),
        );
        t.layout(0.0, 0.0, t.view.measured_width(), t.view.measured_height());
        let mut scene = Scene::new(200, 100);
        t.draw(&mut scene);
        assert!(scene.commands.is_empty());
        t.set_visible(true);
        t.draw(&mut scene);
        assert!(!scene.commands.is_empty());
    }
}
