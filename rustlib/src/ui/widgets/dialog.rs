//! M3 Dialog（Material 3 对话框）。
//!
//! 居中 `elevation` 面板（`surface_container_high` 底）`extra_large` 28dp 圆角
//! + scrim 60% 黑遮罩。

use crate::gfx::scene::Scene;
use crate::text::layout::TextContext;
use crate::ui::dimen::dp;
use crate::ui::measure_spec::MeasureSpec;
use crate::ui::theme;
use crate::ui::view::View;
use crate::ui::widgets::text_view::TextView;
use crate::ui::widgets::widget::Widget;

/// M3 对话框。
pub struct Dialog {
    /// 基础视图（面板矩形）。
    pub view: View,
    /// 标题文本。
    pub title_view: TextView,
    /// 正文文本。
    pub body_view: TextView,
    /// 是否可见。
    visible: bool,
}

impl Dialog {
    /// scrim 颜色（60% 黑）。
    const SCRIM: crate::gfx::color::Color = crate::gfx::color::Color::rgba(0, 0, 0, 153);

    /// 创建（隐藏）。
    pub fn new() -> Self {
        let mut title_view = TextView::new();
        title_view.set_text_style(theme::typography::SCALE.headline_small);
        title_view.set_text_color(theme::theme().roles.on_surface);
        let mut body_view = TextView::new();
        body_view.set_text_style(theme::typography::SCALE.body_medium);
        body_view.set_text_color(theme::theme().roles.on_surface_variant);
        Self {
            view: View::new(),
            title_view,
            body_view,
            visible: false,
        }
    }

    /// 显示/隐藏。
    pub fn set_visible(&mut self, v: bool) {
        self.visible = v;
    }

    /// 是否可见。
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// 设置标题。
    pub fn set_title(&mut self, s: impl Into<String>) {
        self.title_view.set_text(s);
        self.view.request_layout();
    }

    /// 设置正文。
    pub fn set_body(&mut self, s: impl Into<String>) {
        self.body_view.set_text(s);
        self.view.request_layout();
    }

    /// 注入排版上下文（标题/正文各一份）。
    pub fn set_contexts(&mut self, title_cx: TextContext, body_cx: TextContext) {
        self.title_view.set_context(title_cx);
        self.body_view.set_context(body_cx);
    }

    /// 测量：默认 280dp 宽（M3 最小对话框宽），高包裹内容。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        self.title_view.measure(width_spec, height_spec);
        self.body_view.measure(width_spec, height_spec);
        let w = match width_spec.mode() {
            crate::ui::measure_spec::Mode::Exactly => width_spec.size() as f32,
            _ => dp(280.0),
        };
        let th = self.title_view.view.measured_height();
        let bh = self.body_view.view.measured_height();
        let h = match height_spec.mode() {
            crate::ui::measure_spec::Mode::Exactly => height_spec.size() as f32,
            _ => dp(24.0) + th + dp(16.0) + bh + dp(24.0),
        };
        self.view.set_measured_dimension(w.max(0.0), h.max(0.0));
    }

    /// 布局：标题/正文左对齐 24dp。
    pub fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.view.layout(left, top, right, bottom);
        let tw = self.title_view.view.measured_width();
        let th = self.title_view.view.measured_height();
        let tx = left + dp(24.0);
        self.title_view
            .layout(tx, top + dp(24.0), tx + tw, top + dp(24.0) + th);
        let bw = self.body_view.view.measured_width();
        let bh = self.body_view.view.measured_height();
        let by = top + dp(24.0) + th + dp(16.0);
        self.body_view.layout(tx, by, tx + bw, by + bh);
    }

    /// 绘制：scrim + 面板 + 文本（隐藏时跳过）。
    pub fn draw(&self, scene: &mut Scene) {
        if !self.visible {
            return;
        }
        // scrim：铺满整个 scene 视口（用面板父区域近似：以面板外扩为界）。
        scene.commands.push(crate::gfx::scene::Command::FillRect {
            rect: crate::gfx::scene::Rect {
                x: 0.0,
                y: 0.0,
                w: f64::from(self.view.right().max(1080.0)),
                h: f64::from(self.view.bottom().max(2280.0)),
            },
            paint: crate::gfx::paint::Paint::default().color(Self::SCRIM),
        });
        let t = theme::theme();
        scene.commands.push(crate::gfx::scene::Command::FillPath {
            path: crate::gfx::path::Path::rounded_rect(
                f64::from(self.view.left()),
                f64::from(self.view.top()),
                f64::from(self.view.width()),
                f64::from(self.view.height()),
                f64::from(dp(28.0)),
            ),
            paint: crate::gfx::paint::Paint::default().color(t.roles.surface_container_high),
        });
        self.title_view.draw(scene);
        self.body_view.draw(scene);
    }
}

impl Default for Dialog {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Dialog {
    fn view(&self) -> &View {
        &self.view
    }
    fn view_mut(&mut self) -> &mut View {
        &mut self.view
    }
    fn measure(&mut self, ws: MeasureSpec, hs: MeasureSpec) {
        Dialog::measure(self, ws, hs)
    }
    fn layout(&mut self, l: f32, t: f32, r: f32, b: f32) {
        Dialog::layout(self, l, t, r, b)
    }
    fn draw(&self, scene: &mut Scene) {
        Dialog::draw(self, scene)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 隐藏不绘制，显示绘制。
    #[test]
    fn visibility_gates_draw() {
        let mut d = Dialog::new();
        d.set_title("Title");
        d.set_body("Body");
        d.measure(
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 360),
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 640),
        );
        let (w, h) = (d.view.measured_width(), d.view.measured_height());
        d.layout(40.0, 200.0, 40.0 + w, 200.0 + h);
        let mut scene = Scene::new(360, 640);
        d.draw(&mut scene);
        assert!(scene.commands.is_empty());
        d.set_visible(true);
        d.draw(&mut scene);
        assert!(scene.commands.len() >= 2);
    }

    /// 默认最小宽 280dp。
    #[test]
    fn default_min_width_280dp() {
        let mut d = Dialog::new();
        d.measure(
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 480),
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 640),
        );
        assert_eq!(d.view.measured_width(), dp(280.0));
    }
}
