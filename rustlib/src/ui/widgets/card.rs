//! M3 卡片（Material 3 Expressive）。
//!
//! `surface_container_low` 底 + `extra_large`（28dp）圆角容器，
//! 承载任意子内容（V1 不做点击涟漪）。

use crate::gfx::scene::Scene;
use crate::ui::dimen::dp;
use crate::ui::measure_spec::MeasureSpec;
use crate::ui::theme;
use crate::ui::view::View;
use crate::ui::widgets::widget::Widget;

/// M3 卡片：纯容器装饰。
pub struct Card {
    /// 基础视图。
    pub view: View,
}

impl Card {
    /// 创建空卡片。
    pub fn new() -> Self {
        Self { view: View::new() }
    }

    /// 测量：无约束时包裹内容 + 16dp 内边距（子内容由外部容器布局）。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        let w = match width_spec.mode() {
            crate::ui::measure_spec::Mode::Exactly => width_spec.size() as f32,
            _ => dp(320.0),
        };
        let h = match height_spec.mode() {
            crate::ui::measure_spec::Mode::Exactly => height_spec.size() as f32,
            _ => dp(120.0),
        };
        self.view.set_measured_dimension(w.max(0.0), h.max(0.0));
    }

    /// 布局。
    pub fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.view.layout(left, top, right, bottom);
    }

    /// 绘制：`surface_container_low` 底 + 28dp 圆角。
    pub fn draw(&self, scene: &mut Scene) {
        let t = theme::theme();
        let r = theme::shape::ShapeToken::ExtraLarge.radius_dp(0.0);
        scene.commands.push(crate::gfx::scene::Command::FillPath {
            path: crate::gfx::path::Path::rounded_rect(
                f64::from(self.view.left()),
                f64::from(self.view.top()),
                f64::from(self.view.width()),
                f64::from(self.view.height()),
                f64::from(dp(r)),
            ),
            paint: crate::gfx::paint::Paint::default().color(t.roles.surface_container_low),
        });
    }
}

impl Default for Card {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Card {
    fn view(&self) -> &View {
        &self.view
    }
    fn view_mut(&mut self) -> &mut View {
        &mut self.view
    }
    fn measure(&mut self, ws: MeasureSpec, hs: MeasureSpec) {
        Card::measure(self, ws, hs)
    }
    fn layout(&mut self, l: f32, t: f32, r: f32, b: f32) {
        Card::layout(self, l, t, r, b)
    }
    fn draw(&self, scene: &mut Scene) {
        Card::draw(self, scene)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 卡片绘制产生 28dp 圆角矩形底。
    #[test]
    fn card_draws_rounded_bg() {
        let mut c = Card::new();
        c.measure(
            MeasureSpec::make(crate::ui::measure_spec::Mode::Exactly, 320),
            MeasureSpec::make(crate::ui::measure_spec::Mode::Exactly, 120),
        );
        c.layout(0.0, 0.0, 320.0, 120.0);
        let mut scene = Scene::new(320, 120);
        c.draw(&mut scene);
        assert!(!scene.commands.is_empty());
    }
}
