//! M3 Chip（Material 3）。
//!
//! 32dp 高 `full` 药丸，`secondary_container` 底；
//! 支持 leading icon 槽位与文本。

use crate::gfx::scene::Scene;
use crate::text::layout::TextContext;
use crate::ui::dimen::dp;
use crate::ui::measure_spec::MeasureSpec;
use crate::ui::theme;
use crate::ui::view::View;
use crate::ui::widgets::text_view::TextView;
use crate::ui::widgets::widget::Widget;

/// M3 Chip。
pub struct Chip {
    /// 基础视图（布局与绘制槽位由 draw 组合）。
    pub view: View,
    /// 文本控件。
    pub text_view: TextView,
    /// leading icon 是否存在（预留槽位 18dp + 8dp 间距）。
    has_icon: bool,
}

impl Chip {
    /// 创建空文本 chip。
    pub fn new() -> Self {
        let mut text_view = TextView::new();
        text_view.set_text_style(theme::typography::SCALE.label_large);
        text_view.set_text_color(theme::theme().roles.on_secondary_container);
        Self {
            view: View::new(),
            text_view,
            has_icon: false,
        }
    }

    /// 设置文本。
    pub fn set_text(&mut self, s: impl Into<String>) {
        self.text_view.set_text(s);
        self.view.request_layout();
    }

    /// 是否有 leading icon（V1 绘制占位圆点）。
    pub fn set_has_icon(&mut self, has: bool) {
        self.has_icon = has;
        self.view.request_layout();
    }

    /// 注入排版上下文。
    pub fn set_context(&mut self, cx: TextContext) {
        self.text_view.set_context(cx);
    }

    /// 测量：32dp 高，宽 = 文本 + padding（含 icon 槽）。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        self.text_view.measure(width_spec, height_spec);
        let tw = self.text_view.view.measured_width();
        let th = self.text_view.view.measured_height();
        let icon_w = if self.has_icon {
            dp(18.0) + dp(8.0)
        } else {
            0.0
        };
        let h = dp(32.0).max(th);
        let w = (tw + dp(24.0) + icon_w).max(dp(32.0));
        self.view.set_measured_dimension(w, h);
        self.text_view.view.set_measured_dimension(tw, th);
    }

    /// 布局。
    pub fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.view.layout(left, top, right, bottom);
        // 文本在 icon 槽之后垂直居中。
        let icon_w = if self.has_icon {
            dp(18.0) + dp(8.0)
        } else {
            0.0
        };
        let tx = left + dp(12.0) + icon_w;
        let th = self.text_view.view.measured_height();
        let ty = top + (bottom - top - th) / 2.0;
        self.text_view
            .layout(tx, ty, tx + self.text_view.view.measured_width(), ty + th);
    }

    /// 绘制：药丸底 + icon 占位 + 文本。
    pub fn draw(&self, scene: &mut Scene) {
        let t = theme::theme();
        let x = self.view.left();
        let y = self.view.top();
        let w = self.view.width();
        let h = self.view.height();
        scene.commands.push(crate::gfx::scene::Command::FillPath {
            path: crate::gfx::path::Path::rounded_rect(
                f64::from(x),
                f64::from(y),
                f64::from(w),
                f64::from(h),
                f64::from(h / 2.0),
            ),
            paint: crate::gfx::paint::Paint::default().color(t.roles.secondary_container),
        });
        if self.has_icon {
            // V1 leading icon 占位：18dp 圆点。
            let cx = x + dp(12.0) + dp(9.0);
            let cy = y + h / 2.0;
            scene.commands.push(crate::gfx::scene::Command::FillPath {
                path: crate::gfx::path::Path::circle(
                    f64::from(cx),
                    f64::from(cy),
                    f64::from(dp(9.0)),
                ),
                paint: crate::gfx::paint::Paint::default().color(t.roles.on_secondary_container),
            });
        }
        self.text_view.draw(scene);
    }
}

impl Default for Chip {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Chip {
    fn view(&self) -> &View {
        &self.view
    }
    fn view_mut(&mut self) -> &mut View {
        &mut self.view
    }
    fn measure(&mut self, ws: MeasureSpec, hs: MeasureSpec) {
        Chip::measure(self, ws, hs)
    }
    fn layout(&mut self, l: f32, t: f32, r: f32, b: f32) {
        Chip::layout(self, l, t, r, b)
    }
    fn draw(&self, scene: &mut Scene) {
        Chip::draw(self, scene)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 有 icon 时宽度增加 26dp。
    #[test]
    fn icon_adds_width() {
        // 用仓库内 fixture 字体排版，保证文本宽度非零。
        let font_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../app/src/main/assets/fonts/NotoSerifSC-Regular.ttf"
        );
        let font = crate::text::font::load_from_path(std::path::Path::new(font_path))
            .expect("fixture font must load");
        let mut c = Chip::new();
        c.set_context(TextContext::new(&font));
        c.set_text("Chip");
        c.measure(
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 300),
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 100),
        );
        let w_no_icon = c.view.measured_width();
        c.set_has_icon(true);
        c.measure(
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 300),
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 100),
        );
        assert!((c.view.measured_width() - w_no_icon - dp(26.0)).abs() < 1e-3);
    }

    /// 高度至少 32dp。
    #[test]
    fn min_height_32dp() {
        let mut c = Chip::new();
        c.measure(
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 300),
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 100),
        );
        assert!(c.view.measured_height() >= dp(32.0));
    }
}
