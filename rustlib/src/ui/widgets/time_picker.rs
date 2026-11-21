//! M3 TimePicker（Material 3，V1 简化）。
//!
//! 时钟表盘：12 小时刻度圆点，点按刻度选择小时；
//! V1 简化：无指针拖动，仅刻度点按。

use crate::gfx::scene::Scene;
use crate::text::layout::TextContext;
use crate::ui::dimen::dp;
use crate::ui::measure_spec::MeasureSpec;
use crate::ui::theme;
use crate::ui::view::View;
use crate::ui::widgets::text_view::TextView;
use crate::ui::widgets::widget::Widget;

/// M3 时钟表盘 TimePicker。
pub struct TimePicker {
    /// 基础视图。
    pub view: View,
    /// 12 个刻度标签（1~12）。
    pub hour_views: Vec<TextView>,
    /// 选中小时（1~12；0 表示未选）。
    selected: u8,
}

impl TimePicker {
    /// 表盘直径（dp）。
    const DIAMETER: f32 = 280.0;

    /// 创建。
    pub fn new() -> Self {
        let mut hour_views = Vec::new();
        for h in 1..=12 {
            let mut tv = TextView::new();
            tv.set_text_style(theme::typography::SCALE.body_medium);
            tv.set_text_color(theme::theme().roles.on_surface);
            tv.set_text(h.to_string());
            hour_views.push(tv);
        }
        Self {
            view: View::new(),
            hour_views,
            selected: 0,
        }
    }

    /// 注入排版上下文（每刻度一份）。
    pub fn set_contexts(&mut self, contexts: Vec<TextContext>) {
        for (tv, cx) in self.hour_views.iter_mut().zip(contexts) {
            tv.set_context(cx);
        }
        self.view.request_layout();
    }

    /// 命中测试：屏幕坐标 → 小时（1~12），未命中返回 None。
    ///
    /// # 约束
    /// 刻度命中半径 = 表盘半径的一半（M3 触摸目标近似）。
    pub fn hit_test_hour(&self, x: f32, y: f32) -> Option<u8> {
        let cx = self.view.left() + self.view.width() / 2.0;
        let cy = self.view.top() + self.view.height() / 2.0;
        let r = self.view.width().min(self.view.height()) / 2.0;
        let hour_r = r * 0.8;
        let dx = x - cx;
        let dy = y - cy;
        let dist = (dx * dx + dy * dy).sqrt();
        // 命中环带：距刻度圆 ±r/6。
        if (dist - hour_r).abs() > r / 6.0 {
            return None;
        }
        // 数学角（12 点方向为 0，顺时针为正）。
        let angle = f64::from(dy).atan2(f64::from(dx)) + std::f64::consts::FRAC_PI_2;
        let mut sector = (angle / (std::f64::consts::TAU / 12.0))
            .round()
            .rem_euclid(12.0) as u8;
        if sector == 0 {
            sector = 12;
        }
        Some(sector)
    }

    /// 选中小时。
    pub fn set_selected(&mut self, hour: u8) {
        if (1..=12).contains(&hour) {
            self.selected = hour;
        }
    }

    /// 当前选中小时。
    pub fn selected(&self) -> u8 {
        self.selected
    }

    /// 测量：280dp 表盘。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        let spec = MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, Self::DIAMETER as u32);
        for tv in &mut self.hour_views {
            tv.measure(spec, spec);
        }
        let w = match width_spec.mode() {
            crate::ui::measure_spec::Mode::Exactly => width_spec.size() as f32,
            _ => dp(Self::DIAMETER),
        };
        let h = match height_spec.mode() {
            crate::ui::measure_spec::Mode::Exactly => height_spec.size() as f32,
            _ => dp(Self::DIAMETER),
        };
        self.view.set_measured_dimension(w.max(0.0), h.max(0.0));
    }

    /// 布局：刻度沿表盘分布（12 点方向为 1）。
    pub fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.view.layout(left, top, right, bottom);
        let cx = (left + right) / 2.0;
        let cy = (top + bottom) / 2.0;
        let r = (right - left).min(bottom - top) / 2.0;
        let hour_r = r * 0.8;
        for (i, tv) in self.hour_views.iter_mut().enumerate() {
            // 小时 1..=12：12 点方向角 = 0。
            let angle = (i as f32) / 12.0 * std::f32::consts::TAU;
            let hx = cx + hour_r * angle.sin();
            let hy = cy - hour_r * angle.cos();
            let tw = tv.view.measured_width();
            let th = tv.view.measured_height();
            tv.layout(hx - tw / 2.0, hy - th / 2.0, hx + tw / 2.0, hy + th / 2.0);
        }
    }

    /// 绘制：表盘底 + 选中刻度高亮 + 标签。
    pub fn draw(&self, scene: &mut Scene) {
        let t = theme::theme();
        let cx = self.view.left() + self.view.width() / 2.0;
        let cy = self.view.top() + self.view.height() / 2.0;
        let r = self.view.width().min(self.view.height()) / 2.0;
        // 表盘底。
        scene.commands.push(crate::gfx::scene::Command::FillPath {
            path: crate::gfx::path::Path::circle(f64::from(cx), f64::from(cy), f64::from(r)),
            paint: crate::gfx::paint::Paint::default().color(t.roles.surface_container),
        });
        // 选中刻度高亮圈。
        if (1..=12).contains(&self.selected) {
            let i = self.selected as usize - 1;
            let angle = (i as f32) / 12.0 * std::f32::consts::TAU;
            let hour_r = r * 0.8;
            let hx = cx + hour_r * angle.sin();
            let hy = cy - hour_r * angle.cos();
            scene.commands.push(crate::gfx::scene::Command::FillPath {
                path: crate::gfx::path::Path::circle(
                    f64::from(hx),
                    f64::from(hy),
                    f64::from(dp(20.0)),
                ),
                paint: crate::gfx::paint::Paint::default().color(t.roles.primary_container),
            });
        }
        for tv in &self.hour_views {
            tv.draw(scene);
        }
    }
}

impl Default for TimePicker {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for TimePicker {
    fn view(&self) -> &View {
        &self.view
    }
    fn view_mut(&mut self) -> &mut View {
        &mut self.view
    }
    fn measure(&mut self, ws: MeasureSpec, hs: MeasureSpec) {
        TimePicker::measure(self, ws, hs)
    }
    fn layout(&mut self, l: f32, t: f32, r: f32, b: f32) {
        TimePicker::layout(self, l, t, r, b)
    }
    fn draw(&self, scene: &mut Scene) {
        TimePicker::draw(self, scene)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 表盘 280dp 测量。
    #[test]
    fn measures_280dp() {
        let mut p = TimePicker::new();
        p.measure(
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 480),
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 480),
        );
        assert_eq!(p.view.measured_width(), dp(280.0));
    }

    /// 命中测试：12 点方向 → 12，3 点方向 → 3。
    #[test]
    fn hit_test_finds_hours() {
        let mut p = TimePicker::new();
        p.measure(
            MeasureSpec::make(crate::ui::measure_spec::Mode::Exactly, 280),
            MeasureSpec::make(crate::ui::measure_spec::Mode::Exactly, 280),
        );
        p.layout(0.0, 0.0, 280.0, 280.0);
        let cx = 140.0;
        let cy = 140.0;
        let r = 112.0;
        assert_eq!(p.hit_test_hour(cx, cy - r), Some(12));
        assert_eq!(p.hit_test_hour(cx + r, cy), Some(3));
        assert_eq!(p.hit_test_hour(cx, cy + r), Some(6));
        assert_eq!(p.hit_test_hour(cx - r, cy), Some(9));
        // 中心不命中。
        assert_eq!(p.hit_test_hour(cx, cy), None);
    }
}
