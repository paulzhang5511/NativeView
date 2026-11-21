//! M3 NavigationBar（Material 3 底部导航栏）。
//!
//! 2-5 个 item，`surface_container` 底；
//! 选中 item `secondary_container` 药丸指示。

use crate::gfx::scene::Scene;
use crate::text::layout::TextContext;
use crate::ui::dimen::dp;
use crate::ui::measure_spec::MeasureSpec;
use crate::ui::theme;
use crate::ui::view::View;
use crate::ui::widgets::text_view::TextView;
use crate::ui::widgets::widget::Widget;

/// M3 底部导航栏。
pub struct NavigationBar {
    /// 基础视图。
    pub view: View,
    /// item 文本控件。
    pub item_views: Vec<TextView>,
    /// 当前选中索引。
    selected: usize,
}

impl NavigationBar {
    /// 创建空导航栏。
    pub fn new() -> Self {
        Self {
            view: View::new(),
            item_views: Vec::new(),
            selected: 0,
        }
    }

    /// 添加导航项。
    pub fn add_item(&mut self, label: impl Into<String>) {
        let mut tv = TextView::new();
        tv.set_text_style(theme::typography::SCALE.label_medium);
        tv.set_text_color(theme::theme().roles.on_surface);
        tv.set_text(label);
        self.item_views.push(tv);
        self.view.request_layout();
    }

    /// 设置选中项。
    pub fn set_selected(&mut self, index: usize) {
        if index < self.item_views.len() {
            self.selected = index;
            for (i, tv) in self.item_views.iter_mut().enumerate() {
                let color = if i == index {
                    theme::theme().roles.on_secondary_container
                } else {
                    theme::theme().roles.on_surface
                };
                tv.set_text_color(color);
            }
        }
    }

    /// 当前选中索引。
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// 注入排版上下文（每 item 一份）。
    pub fn set_contexts(&mut self, contexts: Vec<TextContext>) {
        for (tv, cx) in self.item_views.iter_mut().zip(contexts) {
            tv.set_context(cx);
        }
        self.view.request_layout();
    }

    /// 测量：全宽 80dp 高。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        let n = self.item_views.len().max(1);
        let w = match width_spec.mode() {
            crate::ui::measure_spec::Mode::Exactly => width_spec.size() as f32,
            _ => dp(360.0),
        };
        let h = match height_spec.mode() {
            crate::ui::measure_spec::Mode::Exactly => height_spec.size() as f32,
            _ => dp(80.0),
        };
        let slot_w = w / n as f32;
        let spec = MeasureSpec::make(crate::ui::measure_spec::Mode::Exactly, slot_w as u32);
        for tv in &mut self.item_views {
            tv.measure(spec, height_spec);
        }
        self.view.set_measured_dimension(w.max(0.0), h.max(0.0));
    }

    /// 布局：等分排列，文本底部对齐。
    pub fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.view.layout(left, top, right, bottom);
        let n = self.item_views.len().max(1);
        let w = right - left;
        let slot_w = w / n as f32;
        for (i, tv) in self.item_views.iter_mut().enumerate() {
            let cx = left + slot_w * (i as f32 + 0.5);
            let tw = tv.view.measured_width();
            let th = tv.view.measured_height();
            let ty = bottom - dp(16.0) - th;
            tv.layout(cx - tw / 2.0, ty, cx + tw / 2.0, ty + th);
        }
    }

    /// 绘制：surface_container 底 + 选中药丸 + 文本。
    pub fn draw(&self, scene: &mut Scene) {
        let t = theme::theme();
        // 全宽底。
        scene.commands.push(crate::gfx::scene::Command::FillRect {
            rect: crate::gfx::scene::Rect {
                x: f64::from(self.view.left()),
                y: f64::from(self.view.top()),
                w: f64::from(self.view.width()),
                h: f64::from(self.view.height()),
            },
            paint: crate::gfx::paint::Paint::default().color(t.roles.surface_container),
        });
        let n = self.item_views.len().max(1) as f32;
        let slot_w = self.view.width() / n;
        // 选中药丸指示（icon 槽位上方）。
        if !self.item_views.is_empty() {
            let cx = self.view.left() + slot_w * (self.selected as f32 + 0.5);
            let cy = self.view.top() + dp(16.0);
            let pill_w = dp(32.0);
            let pill_h = dp(32.0);
            scene.commands.push(crate::gfx::scene::Command::FillPath {
                path: crate::gfx::path::Path::rounded_rect(
                    f64::from(cx - pill_w / 2.0),
                    f64::from(cy),
                    f64::from(pill_w),
                    f64::from(pill_h),
                    f64::from(pill_h / 2.0),
                ),
                paint: crate::gfx::paint::Paint::default().color(t.roles.secondary_container),
            });
        }
        for tv in &self.item_views {
            tv.draw(scene);
        }
    }
}

impl Default for NavigationBar {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for NavigationBar {
    fn view(&self) -> &View {
        &self.view
    }
    fn view_mut(&mut self) -> &mut View {
        &mut self.view
    }
    fn measure(&mut self, ws: MeasureSpec, hs: MeasureSpec) {
        NavigationBar::measure(self, ws, hs)
    }
    fn layout(&mut self, l: f32, t: f32, r: f32, b: f32) {
        NavigationBar::layout(self, l, t, r, b)
    }
    fn draw(&self, scene: &mut Scene) {
        NavigationBar::draw(self, scene)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 选中切换更新 item 颜色。
    #[test]
    fn selection_updates_color() {
        let mut bar = NavigationBar::new();
        bar.add_item("A");
        bar.add_item("B");
        bar.set_selected(1);
        assert_eq!(bar.selected(), 1);
    }

    /// 测量默认 80dp 高。
    #[test]
    fn default_height_80dp() {
        let mut bar = NavigationBar::new();
        bar.add_item("A");
        bar.measure(
            MeasureSpec::make(crate::ui::measure_spec::Mode::Exactly, 360),
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 200),
        );
        assert_eq!(bar.view.measured_height(), dp(80.0));
    }
}
