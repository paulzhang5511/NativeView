//! M3 SearchBar（Material 3 搜索条）。
//!
//! `surface_container_high` 底 `full` 圆角，leading icon 槽 + 文本。

use crate::gfx::scene::Scene;
use crate::text::layout::TextContext;
use crate::ui::dimen::dp;
use crate::ui::measure_spec::MeasureSpec;
use crate::ui::theme;
use crate::ui::view::View;
use crate::ui::widgets::text_view::TextView;
use crate::ui::widgets::widget::Widget;

/// M3 搜索条。
pub struct SearchBar {
    /// 基础视图。
    pub view: View,
    /// 查询文本。
    pub text_view: TextView,
    /// leading icon 字符（V1 文本近似放大镜）。
    pub icon_view: TextView,
}

impl SearchBar {
    /// 创建。
    pub fn new() -> Self {
        let mut text_view = TextView::new();
        text_view.set_text_style(theme::typography::SCALE.body_large);
        text_view.set_text_color(theme::theme().roles.on_surface);
        let mut icon_view = TextView::new();
        icon_view.set_text_style(theme::typography::SCALE.title_large);
        icon_view.set_text_color(theme::theme().roles.on_surface_variant);
        Self {
            view: View::new(),
            text_view,
            icon_view,
        }
    }

    /// 设置占位/查询文本。
    pub fn set_text(&mut self, s: impl Into<String>) {
        self.text_view.set_text(s);
        self.view.request_layout();
    }

    /// 设置 leading icon 字符。
    pub fn set_icon(&mut self, s: impl Into<String>) {
        self.icon_view.set_text(s);
        self.view.request_layout();
    }

    /// 注入排版上下文（文本与 icon 各一份）。
    pub fn set_contexts(&mut self, text_cx: TextContext, icon_cx: TextContext) {
        self.text_view.set_context(text_cx);
        self.icon_view.set_context(icon_cx);
    }

    /// 测量：默认 360dp 宽 48dp 高（M3 search bar）。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        self.text_view.measure(width_spec, height_spec);
        self.icon_view.measure(width_spec, height_spec);
        let w = match width_spec.mode() {
            crate::ui::measure_spec::Mode::Exactly => width_spec.size() as f32,
            _ => dp(360.0),
        };
        let h = match height_spec.mode() {
            crate::ui::measure_spec::Mode::Exactly => height_spec.size() as f32,
            _ => dp(48.0),
        };
        self.view.set_measured_dimension(w.max(0.0), h.max(0.0));
    }

    /// 布局：icon 左 16dp，文本随 icon 后 8dp。
    pub fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.view.layout(left, top, right, bottom);
        let iw = self.icon_view.view.measured_width();
        let ih = self.icon_view.view.measured_height();
        let ix = left + dp(16.0);
        let iy = top + (bottom - top - ih) / 2.0;
        self.icon_view.layout(ix, iy, ix + iw, iy + ih);
        let tw = self.text_view.view.measured_width();
        let th = self.text_view.view.measured_height();
        let tx = ix + iw + dp(8.0);
        let ty = top + (bottom - top - th) / 2.0;
        self.text_view.layout(tx, ty, tx + tw, ty + th);
    }

    /// 绘制：full 圆角底 + icon + 文本。
    pub fn draw(&self, scene: &mut Scene) {
        let t = theme::theme();
        let h = self.view.height();
        scene.commands.push(crate::gfx::scene::Command::FillPath {
            path: crate::gfx::path::Path::rounded_rect(
                f64::from(self.view.left()),
                f64::from(self.view.top()),
                f64::from(self.view.width()),
                f64::from(h),
                f64::from(h / 2.0),
            ),
            paint: crate::gfx::paint::Paint::default().color(t.roles.surface_container_high),
        });
        self.icon_view.draw(scene);
        self.text_view.draw(scene);
    }
}

impl Default for SearchBar {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for SearchBar {
    fn view(&self) -> &View {
        &self.view
    }
    fn view_mut(&mut self) -> &mut View {
        &mut self.view
    }
    fn measure(&mut self, ws: MeasureSpec, hs: MeasureSpec) {
        SearchBar::measure(self, ws, hs)
    }
    fn layout(&mut self, l: f32, t: f32, r: f32, b: f32) {
        SearchBar::layout(self, l, t, r, b)
    }
    fn draw(&self, scene: &mut Scene) {
        SearchBar::draw(self, scene)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 默认 48dp 高。
    #[test]
    fn default_height_48dp() {
        let mut b = SearchBar::new();
        b.measure(
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 480),
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 200),
        );
        assert_eq!(b.view.measured_height(), dp(48.0));
    }

    /// 绘制产出底 + 文本命令。
    #[test]
    fn draws_bg() {
        let mut b = SearchBar::new();
        b.set_icon("Q");
        b.set_text("query");
        b.measure(
            MeasureSpec::make(crate::ui::measure_spec::Mode::Exactly, 360),
            MeasureSpec::make(crate::ui::measure_spec::Mode::Exactly, 48),
        );
        b.layout(0.0, 0.0, 360.0, 48.0);
        let mut scene = Scene::new(360, 48);
        b.draw(&mut scene);
        assert!(!scene.commands.is_empty());
    }
}
