//! M3 Expressive FAB（悬浮操作按钮）。
//!
//! 56dp `primary_container` 底，`large` 圆角（16dp）；
//! Expressive 按压 morph 到 `full` 药丸。

use crate::gfx::scene::Scene;
use crate::ui::dimen::dp;
use crate::ui::measure_spec::MeasureSpec;
use crate::ui::theme;
use crate::ui::view::View;
use crate::ui::widgets::text_view::TextView;
use crate::ui::widgets::widget::Widget;

/// M3 FAB。
pub struct Fab {
    /// 基础视图。
    pub view: View,
    /// 图标/文字承载（V1 用文本字符近似图标）。
    pub text_view: TextView,
    /// 按压进度（0.0~1.0，驱动圆角 morph 16dp→full）。
    pressed: f32,
}

impl Fab {
    /// 创建 56dp FAB。
    pub fn new() -> Self {
        let mut text_view = TextView::new();
        text_view.set_text_style(theme::typography::SCALE.headline_small);
        text_view.set_text_color(theme::theme().roles.on_primary_container);
        let mut view = View::new();
        view.set_padding_dp(0.0, 16.0, 0.0, 16.0);
        Self {
            view,
            text_view,
            pressed: 0.0,
        }
    }

    /// 设置中心图标字符。
    pub fn set_icon(&mut self, s: impl Into<String>) {
        self.text_view.set_text(s);
        self.view.request_layout();
    }

    /// 注入排版上下文。
    pub fn set_context(&mut self, cx: crate::text::layout::TextContext) {
        self.text_view.set_context(cx);
    }

    /// 设置按压进度（0.0 松开 → 1.0 全按，驱动 morph）。
    pub fn set_pressed_progress(&mut self, p: f32) {
        self.pressed = p.clamp(0.0, 1.0);
    }

    /// 测量：固定 56×56dp。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        let size = dp(56.0);
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
        // 图标字符居中。
        let cx = (left + right) / 2.0;
        let tw = self.text_view.view.measured_width();
        let th = self.text_view.view.measured_height();
        let tx = cx - tw / 2.0;
        let ty = top + (bottom - top - th) / 2.0;
        self.text_view.layout(tx, ty, tx + tw, ty + th);
    }

    /// 绘制：morph 圆角矩形底 + 图标。
    pub fn draw(&self, scene: &mut Scene) {
        let t = theme::theme();
        let w = self.view.width();
        let h = self.view.height();
        // 圆角：large 16dp → 按压插值到 full（h/2）。
        let r0 = dp(16.0);
        let r1 = h / 2.0;
        let r = r0 + (r1 - r0) * self.pressed;
        scene.commands.push(crate::gfx::scene::Command::FillPath {
            path: crate::gfx::path::Path::rounded_rect(
                f64::from(self.view.left()),
                f64::from(self.view.top()),
                f64::from(w),
                f64::from(h),
                f64::from(r),
            ),
            paint: crate::gfx::paint::Paint::default().color(t.roles.primary_container),
        });
        self.text_view.draw(scene);
    }
}

impl Default for Fab {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Fab {
    fn view(&self) -> &View {
        &self.view
    }
    fn view_mut(&mut self) -> &mut View {
        &mut self.view
    }
    fn measure(&mut self, ws: MeasureSpec, hs: MeasureSpec) {
        Fab::measure(self, ws, hs)
    }
    fn layout(&mut self, l: f32, t: f32, r: f32, b: f32) {
        Fab::layout(self, l, t, r, b)
    }
    fn draw(&self, scene: &mut Scene) {
        Fab::draw(self, scene)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FAB 默认 56×56dp。
    #[test]
    fn fab_measures_56dp() {
        let mut f = Fab::new();
        f.measure(
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 200),
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 200),
        );
        assert_eq!(f.view.measured_width(), dp(56.0));
        assert_eq!(f.view.measured_height(), dp(56.0));
    }

    /// 绘制不 panic 且产出底色命令。
    #[test]
    fn fab_draws_bg() {
        let mut f = Fab::new();
        f.measure(
            MeasureSpec::make(crate::ui::measure_spec::Mode::Exactly, 56),
            MeasureSpec::make(crate::ui::measure_spec::Mode::Exactly, 56),
        );
        f.layout(0.0, 0.0, dp(56.0), dp(56.0));
        let mut scene = Scene::new(56, 56);
        f.draw(&mut scene);
        assert!(!scene.commands.is_empty());
    }
}
