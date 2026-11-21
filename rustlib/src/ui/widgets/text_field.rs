//! M3 TextField（Material 3 outlined 文本域）。
//!
//! label 浮动动画（缩至 12sp 上移），focus 描边 `primary` 2dp。

use crate::gfx::scene::Scene;
use crate::text::layout::TextContext;
use crate::ui::dimen::dp;
use crate::ui::measure_spec::MeasureSpec;
use crate::ui::theme;
use crate::ui::view::View;
use crate::ui::widgets::text_view::TextView;
use crate::ui::widgets::widget::Widget;

/// M3 outlined 文本域。
pub struct TextField {
    /// 基础视图。
    pub view: View,
    /// 输入文本控件。
    pub text_view: TextView,
    /// 浮动 label。
    pub label_view: TextView,
    /// 是否获得焦点。
    focused: bool,
    /// label 浮动进度（0.0 内联 → 1.0 上浮）。
    label_progress: f32,
    /// 是否非空（非空时 label 保持上浮）。
    has_content: bool,
}

impl TextField {
    /// 创建。
    pub fn new() -> Self {
        let mut text_view = TextView::new();
        text_view.set_text_style(theme::typography::SCALE.body_large);
        text_view.set_text_color(theme::theme().roles.on_surface);
        let mut label_view = TextView::new();
        label_view.set_text_style(theme::typography::SCALE.body_small);
        label_view.set_text_color(theme::theme().roles.on_surface_variant);
        Self {
            view: View::new(),
            text_view,
            label_view,
            focused: false,
            label_progress: 0.0,
            has_content: false,
        }
    }

    /// 设置 label。
    pub fn set_label(&mut self, s: impl Into<String>) {
        self.label_view.set_text(s);
        self.view.request_layout();
    }

    /// 注入排版上下文（内容与 label 各一份；TextContext 不支持 Clone）。
    pub fn set_contexts(&mut self, text_cx: TextContext, label_cx: TextContext) {
        self.text_view.set_context(text_cx);
        self.label_view.set_context(label_cx);
    }

    /// 请求焦点（触发 label 浮动动画）。
    pub fn request_focus(&mut self) {
        if !self.focused {
            self.focused = true;
            self.label_view.set_text_color(theme::theme().roles.primary);
        }
    }

    /// 清除焦点。
    pub fn clear_focus(&mut self) {
        self.focused = false;
        self.label_progress = if self.has_content { 1.0 } else { 0.0 };
        self.label_view
            .set_text_color(theme::theme().roles.on_surface_variant);
    }

    /// 是否获得焦点。
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// 推进 label 浮动动画。
    pub fn advance(&mut self, t: f32) {
        let target = if self.focused || self.has_content {
            1.0
        } else {
            0.0
        };
        self.label_progress = if target > self.label_progress {
            (self.label_progress + t).min(target)
        } else {
            (self.label_progress - t).max(target)
        };
    }

    /// 测量：宽 280dp 默认，高 56dp（含 label 槽）。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        self.text_view.measure(width_spec, height_spec);
        self.label_view.measure(width_spec, height_spec);
        let w = match width_spec.mode() {
            crate::ui::measure_spec::Mode::Exactly => width_spec.size() as f32,
            _ => dp(280.0),
        };
        let h = match height_spec.mode() {
            crate::ui::measure_spec::Mode::Exactly => height_spec.size() as f32,
            _ => dp(56.0),
        };
        self.view.set_measured_dimension(w.max(0.0), h.max(0.0));
    }

    /// 布局。
    pub fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.view.layout(left, top, right, bottom);
        // 内容文本：左侧 16dp，垂直居中偏下（给上浮 label 留位）。
        let tw = self.text_view.view.measured_width();
        let th = self.text_view.view.measured_height();
        let tx = left + dp(16.0);
        let ty = top + dp(16.0);
        self.text_view.layout(tx, ty, tx + tw, ty + th);
        // label：按浮动进度从内联位置移到顶部。
        let lw = self.label_view.view.measured_width();
        let lh = self.label_view.view.measured_height();
        let inline_y = top + (bottom - top - th) / 2.0;
        let floated_y = top - lh / 2.0;
        let ly = inline_y + (floated_y - inline_y) * self.label_progress;
        self.label_view.layout(tx, ly, tx + lw, ly + lh);
    }

    /// 绘制：outlined 描边（focus 2dp primary / 非焦点 1dp outline）+ 文本。
    pub fn draw(&self, scene: &mut Scene) {
        let t = theme::theme();
        let stroke = if self.focused { dp(2.0) } else { dp(1.0) };
        let color = if self.focused {
            t.roles.primary
        } else {
            t.roles.outline
        };
        scene.commands.push(crate::gfx::scene::Command::StrokePath {
            path: crate::gfx::path::Path::rounded_rect(
                f64::from(self.view.left()),
                f64::from(self.view.top()),
                f64::from(self.view.width()),
                f64::from(self.view.height()),
                f64::from(dp(4.0)),
            ),
            paint: crate::gfx::paint::Paint::default()
                .color(color)
                .stroke_width(stroke),
        });
        self.text_view.draw(scene);
        self.label_view.draw(scene);
    }
}

impl Default for TextField {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for TextField {
    fn view(&self) -> &View {
        &self.view
    }
    fn view_mut(&mut self) -> &mut View {
        &mut self.view
    }
    fn measure(&mut self, ws: MeasureSpec, hs: MeasureSpec) {
        TextField::measure(self, ws, hs)
    }
    fn layout(&mut self, l: f32, t: f32, r: f32, b: f32) {
        TextField::layout(self, l, t, r, b)
    }
    fn draw(&self, scene: &mut Scene) {
        TextField::draw(self, scene)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// focus 触发 label 上浮动画。
    #[test]
    fn focus_floats_label() {
        let mut tf = TextField::new();
        assert_eq!(tf.label_progress, 0.0);
        tf.request_focus();
        tf.advance(0.5);
        assert_eq!(tf.label_progress, 0.5);
        tf.advance(1.0);
        assert_eq!(tf.label_progress, 1.0);
    }

    /// 默认测量 56dp 高。
    #[test]
    fn default_height_56dp() {
        let mut tf = TextField::new();
        tf.measure(
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 360),
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 200),
        );
        assert_eq!(tf.view.measured_height(), dp(56.0));
    }
}
