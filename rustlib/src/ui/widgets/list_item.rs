//! M3 列表（Material 3）。
//!
//! `ListItem` 单行图文项；M3 列表高度三档（one-line 56dp / two-line 72dp / three-line 88dp），
//! leading/trailing 槽位。

use crate::gfx::scene::Scene;
use crate::text::layout::TextContext;
use crate::ui::dimen::dp;
use crate::ui::measure_spec::MeasureSpec;
use crate::ui::theme;
use crate::ui::view::View;
use crate::ui::widgets::text_view::TextView;
use crate::ui::widgets::widget::Widget;

/// 列表行数（决定最小高度）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListLines {
    /// 单行：56dp。
    One,
    /// 两行：72dp。
    Two,
    /// 三行：88dp。
    Three,
}

/// M3 列表项：leading 图标 + 标题 + 副标题（两/三行）。
pub struct ListItem {
    /// 基础视图。
    pub view: View,
    /// 标题文本。
    pub text_view: TextView,
    /// 副标题文本（可选）。
    pub subtitle_view: TextView,
    /// 是否有 leading icon（占位圆点 24dp 槽）。
    has_icon: bool,
    /// 行数档位。
    lines: ListLines,
}

impl ListItem {
    /// 创建单行项。
    pub fn new() -> Self {
        let mut text_view = TextView::new();
        text_view.set_text_style(theme::typography::SCALE.body_large);
        text_view.set_text_color(theme::theme().roles.on_surface);
        let mut subtitle_view = TextView::new();
        subtitle_view.set_text_style(theme::typography::SCALE.body_medium);
        subtitle_view.set_text_color(theme::theme().roles.on_surface_variant);
        Self {
            view: View::new(),
            text_view,
            subtitle_view,
            has_icon: false,
            lines: ListLines::One,
        }
    }

    /// 设置标题。
    pub fn set_text(&mut self, s: impl Into<String>) {
        self.text_view.set_text(s);
        self.view.request_layout();
    }

    /// 设置副标题（自动升到两行档）。
    pub fn set_subtitle(&mut self, s: impl Into<String>) {
        self.subtitle_view.set_text(s);
        self.lines = ListLines::Two;
        self.view.request_layout();
    }

    /// 是否有 leading icon。
    pub fn set_has_icon(&mut self, has: bool) {
        self.has_icon = has;
        self.view.request_layout();
    }

    /// 注入排版上下文（标题一份，副标题一份；TextContext 不支持 Clone）。
    pub fn set_contexts(&mut self, title_cx: TextContext, subtitle_cx: TextContext) {
        self.text_view.set_context(title_cx);
        self.subtitle_view.set_context(subtitle_cx);
    }

    /// 最小高度（按行数档位）。
    fn min_height(&self) -> f32 {
        match self.lines {
            ListLines::One => dp(56.0),
            ListLines::Two => dp(72.0),
            ListLines::Three => dp(88.0),
        }
    }

    /// 测量：宽度包裹文本，高度取行数档最小值。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        self.text_view.measure(width_spec, height_spec);
        self.subtitle_view.measure(width_spec, height_spec);
        let tw = self.text_view.view.measured_width();
        let sw = self.subtitle_view.view.measured_width();
        let icon_w = if self.has_icon {
            dp(24.0) + dp(16.0)
        } else {
            0.0
        };
        let w = tw.max(sw) + icon_w + dp(32.0);
        let content_h =
            self.text_view.view.measured_height() + self.subtitle_view.view.measured_height();
        let h = content_h.max(self.min_height());
        self.view.set_measured_dimension(w, h);
    }

    /// 布局。
    pub fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.view.layout(left, top, right, bottom);
        let icon_w = if self.has_icon {
            dp(24.0) + dp(16.0)
        } else {
            0.0
        };
        let tx = left + dp(16.0) + icon_w;
        let th = self.text_view.view.measured_height();
        let sh = self.subtitle_view.view.measured_height();
        // 标题+副标题整体垂直居中。
        let total = th + sh;
        let y0 = top + ((bottom - top) - total) / 2.0;
        let tw = self.text_view.view.measured_width();
        self.text_view.layout(tx, y0, tx + tw, y0 + th);
        let sw = self.subtitle_view.view.measured_width();
        self.subtitle_view
            .layout(tx, y0 + th, tx + sw, y0 + th + sh);
    }

    /// 绘制：leading icon 占位圆点 + 文本。
    pub fn draw(&self, scene: &mut Scene) {
        if self.has_icon {
            let t = theme::theme();
            let cx = self.view.left() + dp(16.0) + dp(12.0);
            let cy = self.view.top() + self.view.height() / 2.0;
            scene.commands.push(crate::gfx::scene::Command::FillPath {
                path: crate::gfx::path::Path::circle(
                    f64::from(cx),
                    f64::from(cy),
                    f64::from(dp(12.0)),
                ),
                paint: crate::gfx::paint::Paint::default().color(t.roles.primary),
            });
        }
        self.text_view.draw(scene);
        self.subtitle_view.draw(scene);
    }
}

impl Default for ListItem {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for ListItem {
    fn view(&self) -> &View {
        &self.view
    }
    fn view_mut(&mut self) -> &mut View {
        &mut self.view
    }
    fn measure(&mut self, ws: MeasureSpec, hs: MeasureSpec) {
        ListItem::measure(self, ws, hs)
    }
    fn layout(&mut self, l: f32, t: f32, r: f32, b: f32) {
        ListItem::layout(self, l, t, r, b)
    }
    fn draw(&self, scene: &mut Scene) {
        ListItem::draw(self, scene)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 单行项最小高度 56dp。
    #[test]
    fn one_line_min_height() {
        let mut i = ListItem::new();
        i.measure(
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 300),
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 200),
        );
        assert!(i.view.measured_height() >= dp(56.0));
    }

    /// 副标题升至两行档（72dp 最小高）。
    #[test]
    fn subtitle_upgrades_to_two_lines() {
        let mut i = ListItem::new();
        i.set_subtitle("sub");
        i.measure(
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 300),
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 200),
        );
        assert!(i.view.measured_height() >= dp(72.0));
    }
}
