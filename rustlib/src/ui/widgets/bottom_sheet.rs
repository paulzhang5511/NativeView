//! M3 BottomSheet（Material 3 底部抽屉）。
//!
//! 底部弹出 `extra_large` 顶圆角 + drag handle 32×4dp。

use crate::gfx::scene::Scene;
use crate::ui::dimen::dp;
use crate::ui::measure_spec::MeasureSpec;
use crate::ui::theme;
use crate::ui::view::View;
use crate::ui::widgets::widget::Widget;

/// M3 底部抽屉。
pub struct BottomSheet {
    /// 基础视图。
    pub view: View,
    /// 展开高度进度（0.0 收起 → 1.0 展开）。
    pub expand_progress: f32,
    /// 是否可见。
    visible: bool,
}

impl BottomSheet {
    /// 创建（隐藏，半展开 0.5）。
    pub fn new() -> Self {
        Self {
            view: View::new(),
            expand_progress: 0.5,
            visible: false,
        }
    }

    /// 显示/隐藏。
    pub fn set_visible(&mut self, v: bool) {
        self.visible = v;
    }

    /// 是否可见。
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// 设置展开进度。
    pub fn set_expand_progress(&mut self, p: f32) {
        self.expand_progress = p.clamp(0.0, 1.0);
    }

    /// 测量：全宽，高度 = peek 80dp + 展开量 × 剩余空间。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        let w = match width_spec.mode() {
            crate::ui::measure_spec::Mode::Exactly => width_spec.size() as f32,
            _ => dp(360.0),
        };
        let avail = match height_spec.mode() {
            crate::ui::measure_spec::Mode::Exactly => height_spec.size() as f32,
            _ => dp(640.0),
        };
        let peek = dp(80.0);
        let h = peek + (avail - peek) * self.expand_progress;
        self.view.set_measured_dimension(w.max(0.0), h.max(peek));
    }

    /// 布局。
    pub fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.view.layout(left, top, right, bottom);
    }

    /// 绘制：顶圆角面板（可见时）+ drag handle。
    pub fn draw(&self, scene: &mut Scene) {
        if !self.visible {
            return;
        }
        let t = theme::theme();
        // 面板：顶部 28dp 圆角（底部直角）。
        let w = self.view.width();
        let h = self.view.height();
        let r = dp(28.0).min(w / 2.0);
        scene.commands.push(crate::gfx::scene::Command::FillPath {
            path: top_rounded_rect(self.view.left(), self.view.top(), w, h, r),
            paint: crate::gfx::paint::Paint::default().color(t.roles.surface_container_low),
        });
        // drag handle：32×4dp 居中，距顶 8dp。
        let cx = self.view.left() + w / 2.0;
        let hy = self.view.top() + dp(8.0);
        scene.commands.push(crate::gfx::scene::Command::FillRect {
            rect: crate::gfx::scene::Rect {
                x: f64::from(cx - dp(16.0)),
                y: f64::from(hy),
                w: f64::from(dp(32.0)),
                h: f64::from(dp(4.0)),
            },
            paint: crate::gfx::paint::Paint::default().color(t.roles.outline_variant),
        });
    }
}

/// 顶圆角矩形路径（仅上两角圆角，下两角直角）。
fn top_rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> crate::gfx::path::Path {
    let (x, y, w, h, r) = (
        f64::from(x),
        f64::from(y),
        f64::from(w),
        f64::from(h),
        f64::from(r),
    );
    let k = r * 0.5522847498;
    let (right, bottom) = (x + w, y + h);
    let mut p = crate::gfx::path::Path::new();
    p.move_to(x, y + r);
    p.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    p.line_to(right - r, y);
    p.cubic_to(right - r + k, y, right, y + r - k, right, y + r);
    p.line_to(right, bottom);
    p.line_to(x, bottom);
    p.close();
    p
}

impl Default for BottomSheet {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for BottomSheet {
    fn view(&self) -> &View {
        &self.view
    }
    fn view_mut(&mut self) -> &mut View {
        &mut self.view
    }
    fn measure(&mut self, ws: MeasureSpec, hs: MeasureSpec) {
        BottomSheet::measure(self, ws, hs)
    }
    fn layout(&mut self, l: f32, t: f32, r: f32, b: f32) {
        BottomSheet::layout(self, l, t, r, b)
    }
    fn draw(&self, scene: &mut Scene) {
        BottomSheet::draw(self, scene)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 展开进度插值高度。
    #[test]
    fn expand_progress_scales_height() {
        let mut s = BottomSheet::new();
        s.set_expand_progress(0.0);
        s.measure(
            MeasureSpec::make(crate::ui::measure_spec::Mode::Exactly, 360),
            MeasureSpec::make(crate::ui::measure_spec::Mode::Exactly, 640),
        );
        assert_eq!(s.view.measured_height(), dp(80.0));
        s.set_expand_progress(1.0);
        s.measure(
            MeasureSpec::make(crate::ui::measure_spec::Mode::Exactly, 360),
            MeasureSpec::make(crate::ui::measure_spec::Mode::Exactly, 640),
        );
        assert_eq!(s.view.measured_height(), dp(640.0));
    }

    /// 隐藏不绘制。
    #[test]
    fn hidden_draws_nothing() {
        let mut s = BottomSheet::new();
        s.measure(
            MeasureSpec::make(crate::ui::measure_spec::Mode::Exactly, 360),
            MeasureSpec::make(crate::ui::measure_spec::Mode::Exactly, 640),
        );
        s.layout(0.0, 560.0, 360.0, 640.0);
        let mut scene = Scene::new(360, 640);
        s.draw(&mut scene);
        assert!(scene.commands.is_empty());
        s.set_visible(true);
        s.draw(&mut scene);
        assert_eq!(scene.commands.len(), 2);
    }
}
