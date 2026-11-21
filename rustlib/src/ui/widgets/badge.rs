//! M3 徽章（Material 3）。
//!
//! 小圆点/数字徽章：`error` 底、纯展示（不做交互）。
//! 数字徽章宽随内容（3 字符以上变药丸）。

use super::text_view::TextView;
use crate::gfx::scene::Scene;
use crate::ui::dimen::dp;
use crate::ui::measure_spec::MeasureSpec;
use crate::ui::theme;
use crate::ui::view::View;
use crate::ui::widgets::widget::Widget;

/// M3 徽章。
pub struct Badge {
    /// 文本控件（数字徽章时承载排版）。
    pub text_view: TextView,
    /// 是否有数字（决定尺寸：点 6dp vs 数字药丸）。
    has_number: bool,
}

impl Badge {
    /// 创建小圆点徽章（无数字）。
    pub fn new() -> Self {
        Self {
            text_view: TextView::new(),
            has_number: false,
        }
    }

    /// 创建数字徽章。
    pub fn with_number(n: u32) -> Self {
        let mut text_view = TextView::new();
        // M3 label_small 字阶。
        text_view.set_text_style(theme::typography::SCALE.label_small);
        text_view.set_text_color(theme::theme().roles.on_error);
        text_view.set_text(n.to_string());
        Self {
            text_view,
            has_number: true,
        }
    }

    /// 注入排版上下文（数字徽章需要）。
    pub fn set_context(&mut self, cx: crate::text::layout::TextContext) {
        self.text_view.set_context(cx);
    }

    /// 测量：点 6dp；数字徽章 = 文本 + 水平 4dp 内边距（最小 16dp 宽）。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        if !self.has_number {
            self.text_view.view.set_measured_dimension(dp(6.0), dp(6.0));
            let _ = (width_spec, height_spec);
            return;
        }
        // 数字徽章：文本测量 + padding，最小 16dp 直径。
        self.text_view.measure(width_spec, height_spec);
        let tw = self.text_view.view.measured_width();
        let th = self.text_view.view.measured_height();
        let w = (tw + dp(8.0)).max(dp(16.0));
        let h = th.max(dp(16.0));
        self.text_view.view.set_measured_dimension(w, h);
    }

    /// 布局。
    pub fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.text_view.layout(left, top, right, bottom);
    }

    /// 绘制：error 底 + 文本。
    pub fn draw(&self, scene: &mut Scene) {
        let t = theme::theme();
        let v = &self.text_view.view;
        let w = v.width();
        let h = if self.has_number {
            v.height().max(dp(16.0))
        } else {
            dp(6.0)
        };
        let x = v.left();
        let y = if self.has_number {
            v.top()
        } else {
            v.top() + (v.height() - h) / 2.0
        };
        // 底：error 色药丸/圆。
        scene.commands.push(crate::gfx::scene::Command::FillPath {
            path: crate::gfx::path::Path::rounded_rect(
                f64::from(x),
                f64::from(y),
                f64::from(w),
                f64::from(h),
                f64::from(h / 2.0),
            ),
            paint: crate::gfx::paint::Paint::default().color(t.roles.error),
        });
        if self.has_number {
            self.text_view.draw(scene);
        }
    }
}

impl Default for Badge {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Badge {
    fn view(&self) -> &View {
        &self.text_view.view
    }
    fn view_mut(&mut self) -> &mut View {
        &mut self.text_view.view
    }
    fn measure(&mut self, ws: MeasureSpec, hs: MeasureSpec) {
        Badge::measure(self, ws, hs)
    }
    fn layout(&mut self, l: f32, t: f32, r: f32, b: f32) {
        Badge::layout(self, l, t, r, b)
    }
    fn draw(&self, scene: &mut Scene) {
        Badge::draw(self, scene)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 点徽章固定 6dp。
    #[test]
    fn dot_badge_measures_6dp() {
        let mut b = Badge::new();
        b.measure(
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 100),
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 100),
        );
        assert_eq!(b.text_view.view.measured_width(), dp(6.0));
    }

    /// 数字徽章有最小宽度。
    #[test]
    fn number_badge_has_min_width() {
        let mut b = Badge::with_number(3);
        b.measure(
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 100),
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 100),
        );
        assert!(b.text_view.view.measured_width() >= dp(16.0));
    }
}
