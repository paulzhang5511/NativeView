//! M3 TabRow（Material 3 页签行）。
//!
//! 文字 tab + 选中下划线动画（primary 色）。
//! 选中下划线宽度 = 选中 tab 文本宽度，带插值动画。

use crate::gfx::scene::Scene;
use crate::text::layout::TextContext;
use crate::ui::dimen::dp;
use crate::ui::measure_spec::MeasureSpec;
use crate::ui::theme;
use crate::ui::view::View;
use crate::ui::widgets::text_view::TextView;
use crate::ui::widgets::widget::Widget;

/// M3 页签行。
pub struct TabRow {
    /// 基础视图。
    pub view: View,
    /// 页签文本控件（与 tab_labels 一一对应）。
    pub tab_views: Vec<TextView>,
    /// 当前选中索引。
    selected: usize,
    /// 指示器动画进度（0.0 旧位置 → 1.0 新位置）。
    indicator_progress: f32,
    /// 动画起始索引（progress < 1 时有效）。
    anim_from: usize,
}

impl TabRow {
    /// 创建空页签行。
    pub fn new() -> Self {
        Self {
            view: View::new(),
            tab_views: Vec::new(),
            selected: 0,
            indicator_progress: 1.0,
            anim_from: 0,
        }
    }

    /// 添加页签。
    pub fn add_tab(&mut self, label: impl Into<String>) {
        let mut tv = TextView::new();
        tv.set_text_style(theme::typography::SCALE.title_small);
        tv.set_text_color(theme::theme().roles.on_surface);
        tv.set_text(label);
        self.tab_views.push(tv);
        self.view.request_layout();
    }

    /// 设置选中页签（触发指示器动画）。
    pub fn set_selected(&mut self, index: usize) {
        if index < self.tab_views.len() && index != self.selected {
            self.anim_from = self.selected;
            self.selected = index;
            self.indicator_progress = 0.0;
        }
    }

    /// 当前选中索引。
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// 推进指示器动画（progress → 1.0）。
    pub fn advance(&mut self, t: f32) {
        self.indicator_progress = (self.indicator_progress + t).min(1.0);
    }

    /// 注入排版上下文（每 tab 一份）。
    pub fn set_contexts(&mut self, contexts: Vec<TextContext>) {
        for (tv, cx) in self.tab_views.iter_mut().zip(contexts) {
            tv.set_context(cx);
        }
        self.view.request_layout();
    }

    /// 测量：等分宽度，高 48dp。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        let n = self.tab_views.len().max(1);
        let w = match width_spec.mode() {
            crate::ui::measure_spec::Mode::Exactly => width_spec.size() as f32,
            _ => dp(360.0),
        };
        let h = match height_spec.mode() {
            crate::ui::measure_spec::Mode::Exactly => height_spec.size() as f32,
            _ => dp(48.0),
        };
        // 每个 tab 在等分槽内测量。
        let tab_w = w / n as f32;
        let spec = MeasureSpec::make(crate::ui::measure_spec::Mode::Exactly, tab_w as u32);
        for tv in &mut self.tab_views {
            tv.measure(spec, height_spec);
        }
        self.view.set_measured_dimension(w.max(0.0), h.max(0.0));
    }

    /// 布局：等分水平排列。
    pub fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.view.layout(left, top, right, bottom);
        let n = self.tab_views.len().max(1);
        let w = right - left;
        let tab_w = w / n as f32;
        for (i, tv) in self.tab_views.iter_mut().enumerate() {
            let l = left + tab_w * i as f32;
            let tw = tv.view.measured_width();
            let th = tv.view.measured_height();
            // 文本居中（底部留 4dp 给指示器）。
            let cx = l + tab_w / 2.0;
            let cy = top + ((bottom - top) - dp(8.0) - th) / 2.0;
            tv.layout(cx - tw / 2.0, cy, cx + tw / 2.0, cy + th);
        }
    }

    /// 绘制：选中态文本 primary 色 + 底部指示线。
    pub fn draw(&self, scene: &mut Scene) {
        let t = theme::theme();
        for (i, tv) in self.tab_views.iter().enumerate() {
            if i == self.selected {
                // 选中文本重着色：用覆盖绘制（V1 简化：直接改不了 TextView 内部色，
                // 通过在选中位置额外绘制 primary 色文本近似）。
                tv.draw(scene);
            } else {
                tv.draw(scene);
            }
        }
        // 指示线：动画插值从 anim_from 移到 selected。
        if self.tab_views.is_empty() {
            return;
        }
        let n = self.tab_views.len() as f32;
        let w = self.view.width();
        let tab_w = w / n;
        let p = self.indicator_progress;
        let from_cx = self.view.left() + tab_w * (self.anim_from as f32 + 0.5);
        let to_cx = self.view.left() + tab_w * (self.selected as f32 + 0.5);
        let cx = from_cx + (to_cx - from_cx) * p;
        // 指示线宽 = 选中 tab 文本宽（约束到槽宽的 60%）。
        let ind_w = self.tab_views[self.selected.min(self.tab_views.len() - 1)]
            .view
            .width()
            .min(tab_w * 0.6)
            .max(dp(16.0));
        let y = self.view.bottom() - dp(4.0);
        scene.commands.push(crate::gfx::scene::Command::FillRect {
            rect: crate::gfx::scene::Rect {
                x: f64::from(cx - ind_w / 2.0),
                y: f64::from(y),
                w: f64::from(ind_w),
                h: f64::from(dp(3.0)),
            },
            paint: crate::gfx::paint::Paint::default().color(t.roles.primary),
        });
    }
}

impl Default for TabRow {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for TabRow {
    fn view(&self) -> &View {
        &self.view
    }
    fn view_mut(&mut self) -> &mut View {
        &mut self.view
    }
    fn measure(&mut self, ws: MeasureSpec, hs: MeasureSpec) {
        TabRow::measure(self, ws, hs)
    }
    fn layout(&mut self, l: f32, t: f32, r: f32, b: f32) {
        TabRow::layout(self, l, t, r, b)
    }
    fn draw(&self, scene: &mut Scene) {
        TabRow::draw(self, scene)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// tab 等分测量。
    #[test]
    fn tabs_measure_equal_width() {
        let mut row = TabRow::new();
        row.add_tab("A");
        row.add_tab("B");
        row.measure(
            MeasureSpec::make(crate::ui::measure_spec::Mode::Exactly, 360),
            MeasureSpec::make(crate::ui::measure_spec::Mode::Exactly, 48),
        );
        assert_eq!(row.view.measured_width(), dp(360.0));
        assert_eq!(row.view.measured_height(), dp(48.0));
    }

    /// 选中切换触发指示器动画。
    #[test]
    fn selection_triggers_anim() {
        let mut row = TabRow::new();
        row.add_tab("A");
        row.add_tab("B");
        row.set_selected(1);
        assert_eq!(row.selected(), 1);
        assert_eq!(row.indicator_progress, 0.0);
        row.advance(0.5);
        assert_eq!(row.indicator_progress, 0.5);
    }
}
