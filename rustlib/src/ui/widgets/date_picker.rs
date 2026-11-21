//! M3 DatePicker（Material 3，V1 简化）。
//!
//! 月历网格：7 列星期 × 6 行周，支持月份切换；
//! V1 无年份选择。

use crate::gfx::scene::Scene;
use crate::text::layout::TextContext;
use crate::ui::dimen::dp;
use crate::ui::measure_spec::MeasureSpec;
use crate::ui::theme;
use crate::ui::view::View;
use crate::ui::widgets::text_view::TextView;
use crate::ui::widgets::widget::Widget;

/// M3 日历选择器。
pub struct DatePicker {
    /// 基础视图。
    pub view: View,
    /// 标题（月份年）文本。
    pub title_view: TextView,
    /// 日期单元格（6 行 × 7 列 = 42）。
    pub day_views: Vec<TextView>,
    /// 当前年。
    year: i32,
    /// 当前月（1~12）。
    month: u8,
    /// 选中日（None 未选）。
    selected_day: Option<u8>,
}

impl DatePicker {
    /// 单元格尺寸（dp）。
    const CELL: f32 = 40.0;
    /// 6×7 网格 + 标题区。
    const GRID_W: f32 = Self::CELL * 7.0;
    const GRID_H: f32 = Self::CELL * 6.0;

    /// 创建（默认 2026 年 11 月）。
    pub fn new() -> Self {
        let mut title_view = TextView::new();
        title_view.set_text_style(theme::typography::SCALE.title_medium);
        title_view.set_text_color(theme::theme().roles.on_surface);
        let mut day_views = Vec::new();
        for _ in 0..42 {
            let mut tv = TextView::new();
            tv.set_text_style(theme::typography::SCALE.body_medium);
            tv.set_text_color(theme::theme().roles.on_surface);
            day_views.push(tv);
        }
        Self {
            view: View::new(),
            title_view,
            day_views,
            year: 2026,
            month: 11,
            selected_day: None,
        }
    }

    /// 当月天数。
    fn days_in_month(&self) -> u8 {
        match self.month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if self.year % 4 == 0 && (self.year % 100 != 0 || self.year % 400 == 0) {
                    29
                } else {
                    28
                }
            }
            _ => 30,
        }
    }

    /// 当月 1 日的星期（0=周日，M3 网格惯例）。
    fn first_weekday(&self) -> u8 {
        // 采 Zeller 同余（公历）。
        let q = 1;
        let m = if self.month < 3 {
            self.month as i32 + 12
        } else {
            self.month as i32
        };
        let k = if self.month < 3 {
            (self.year - 1) % 100
        } else {
            self.year % 100
        };
        let j = if self.month < 3 {
            (self.year - 1) / 100
        } else {
            self.year / 100
        };
        let h = (q + 13 * (m + 1) / 5 + k + k / 4 + j / 4 + 5 * j) % 7;
        // Zeller h：0=周六，1=周日；转周日=0 索引：weekday = (h + 6) % 7。
        ((h + 6) % 7) as u8
    }

    /// 上一个月（跨年回退）。
    pub fn previous_month(&mut self) {
        self.month -= 1;
        if self.month == 0 {
            self.month = 12;
            self.year -= 1;
        }
        self.selected_day = None;
        self.refresh_cells();
    }

    /// 下一个月（跨年推进）。
    pub fn next_month(&mut self) {
        self.month += 1;
        if self.month > 12 {
            self.month = 1;
            self.year += 1;
        }
        self.selected_day = None;
        self.refresh_cells();
    }

    /// 刷新单元格文本与标题。
    fn refresh_cells(&mut self) {
        let offset = self.first_weekday() as usize;
        let days = self.days_in_month();
        for (i, tv) in self.day_views.iter_mut().enumerate() {
            if i >= offset && i < offset + days as usize {
                tv.set_text(((i - offset + 1) as u8).to_string());
            } else {
                tv.set_text("");
            }
        }
        self.title_view.set_text(format!(
            "{} {}",
            MONTH_NAMES[self.month as usize - 1],
            self.year
        ));
    }

    /// 初始化文本（构造后调用一次，需先注入排版上下文）。
    pub fn refresh(&mut self) {
        self.refresh_cells();
    }

    /// 注入排版上下文（标题一份 + 42 单元格）。
    pub fn set_contexts(&mut self, title_cx: TextContext, day_cxs: Vec<TextContext>) {
        self.title_view.set_context(title_cx);
        for (tv, cx) in self.day_views.iter_mut().zip(day_cxs) {
            tv.set_context(cx);
        }
        self.refresh_cells();
    }

    /// 选中日（1~当月天数）。
    pub fn set_selected_day(&mut self, day: u8) {
        if day >= 1 && day <= self.days_in_month() {
            self.selected_day = Some(day);
        }
    }

    /// 当前选中日。
    pub fn selected_day(&self) -> Option<u8> {
        self.selected_day
    }

    /// 测量：网格宽 + 标题区高。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        self.title_view.measure(width_spec, height_spec);
        let cell_spec = MeasureSpec::make(
            crate::ui::measure_spec::Mode::Exactly,
            dp(Self::CELL) as u32,
        );
        for tv in &mut self.day_views {
            tv.measure(cell_spec, cell_spec);
        }
        let w = match width_spec.mode() {
            crate::ui::measure_spec::Mode::Exactly => width_spec.size() as f32,
            _ => dp(Self::GRID_W),
        };
        let th = dp(48.0);
        let h = match height_spec.mode() {
            crate::ui::measure_spec::Mode::Exactly => height_spec.size() as f32,
            _ => th + dp(Self::GRID_H),
        };
        self.view.set_measured_dimension(w.max(0.0), h.max(0.0));
    }

    /// 布局：标题 + 6×7 网格。
    pub fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.view.layout(left, top, right, bottom);
        let tw = self.title_view.view.measured_width();
        let th = self.title_view.view.measured_height();
        self.title_view.layout(
            left + dp(16.0),
            top + dp(16.0),
            left + dp(16.0) + tw,
            top + dp(16.0) + th,
        );
        let cell = dp(Self::CELL);
        let grid_top = top + dp(48.0);
        for (i, tv) in self.day_views.iter_mut().enumerate() {
            let col = (i % 7) as f32;
            let row = (i / 7) as f32;
            let x = left + col * cell;
            let y = grid_top + row * cell;
            tv.layout(x, y, x + cell, y + cell);
        }
    }

    /// 绘制：面板 + 标题 + 选中日高亮圈 + 单元格。
    pub fn draw(&self, scene: &mut Scene) {
        let t = theme::theme();
        scene.commands.push(crate::gfx::scene::Command::FillPath {
            path: crate::gfx::path::Path::rounded_rect(
                f64::from(self.view.left()),
                f64::from(self.view.top()),
                f64::from(self.view.width()),
                f64::from(self.view.height()),
                f64::from(dp(28.0)),
            ),
            paint: crate::gfx::paint::Paint::default().color(t.roles.surface_container_low),
        });
        // 选中日高亮圈。
        if let Some(day) = self.selected_day {
            let offset = self.first_weekday() as usize;
            let idx = offset + day as usize - 1;
            if idx < self.day_views.len() {
                let cell = dp(Self::CELL);
                let col = (idx % 7) as f32;
                let row = (idx / 7) as f32;
                let cx = self.view.left() + col * cell + cell / 2.0;
                let cy = self.view.top() + dp(48.0) + row * cell + cell / 2.0;
                scene.commands.push(crate::gfx::scene::Command::FillPath {
                    path: crate::gfx::path::Path::circle(
                        f64::from(cx),
                        f64::from(cy),
                        f64::from(cell / 2.0),
                    ),
                    paint: crate::gfx::paint::Paint::default().color(t.roles.primary),
                });
            }
        }
        self.title_view.draw(scene);
        for tv in &self.day_views {
            tv.draw(scene);
        }
    }
}

/// 英文月份名（标题文案；组件内部默认文案，宿主可覆盖）。
const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

impl Default for DatePicker {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for DatePicker {
    fn view(&self) -> &View {
        &self.view
    }
    fn view_mut(&mut self) -> &mut View {
        &mut self.view
    }
    fn measure(&mut self, ws: MeasureSpec, hs: MeasureSpec) {
        DatePicker::measure(self, ws, hs)
    }
    fn layout(&mut self, l: f32, t: f32, r: f32, b: f32) {
        DatePicker::layout(self, l, t, r, b)
    }
    fn draw(&self, scene: &mut Scene) {
        DatePicker::draw(self, scene)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026 年 11 月：30 天，1 日是周日。
    #[test]
    fn nov_2026_has_30_days_first_sunday() {
        let p = DatePicker::new();
        assert_eq!(p.days_in_month(), 30);
        assert_eq!(p.first_weekday(), 0);
    }

    /// 月份切换跨年。
    #[test]
    fn month_navigation_wraps_year() {
        let mut p = DatePicker::new();
        p.year = 2026;
        p.month = 12;
        p.next_month();
        assert_eq!((p.year, p.month), (2027, 1));
        p.previous_month();
        assert_eq!((p.year, p.month), (2026, 12));
        // 切换后选中态清除。
        assert_eq!(p.selected_day(), None);
    }

    /// 闰年二月 29 天。
    #[test]
    fn leap_year_february() {
        let mut p = DatePicker::new();
        p.year = 2028;
        p.month = 2;
        assert_eq!(p.days_in_month(), 29);
    }
}
