//! M3 进度指示器（Material 3）：线形 + 圆形。
//!
//! # 设计
//! - Linear：4dp 轨，active 段 primary；determinate（0-1）与
//!   indeterminate（往返动画，由宿主推进 phase）两种；
//! - Circular：48dp 圆环，determinate 扫角；indeterminate 旋转由
//!   宿主调 `advance` 推进；
//! - 纯展示组件（无触摸交互）。

use crate::gfx::scene::Scene;
use crate::ui::dimen::dp;
use crate::ui::measure_spec::MeasureSpec;
use crate::ui::theme;
use crate::ui::view::View;
use crate::ui::widgets::widget::Widget;

/// 线形进度指示器。
pub struct LinearProgressIndicator {
    /// 基础视图。
    pub view: View,
    /// 进度（determinate 模式 0.0-1.0）。
    progress: f32,
    /// indeterminate 模式相位（0.0-1.0，由宿主逐帧推进）。
    phase: f32,
    /// 是否 indeterminate。
    indeterminate: bool,
}

impl LinearProgressIndicator {
    /// 创建 determinate 指示器（进度 0）。
    pub fn new() -> Self {
        Self {
            view: View::new(),
            progress: 0.0,
            phase: 0.0,
            indeterminate: false,
        }
    }

    /// 切换 indeterminate 模式。
    pub fn set_indeterminate(&mut self, ind: bool) {
        self.indeterminate = ind;
        self.view.request_layout();
    }

    /// 设置 determinate 进度。
    pub fn set_progress(&mut self, p: f32) {
        self.progress = p.clamp(0.0, 1.0);
        self.view.request_layout();
    }

    /// 推进 indeterminate 相位（帧回调调用）。
    pub fn advance_phase(&mut self, delta: f32) {
        self.phase = (self.phase + delta).rem_euclid(1.0);
        self.view.request_layout();
    }

    /// 测量：全宽 4dp。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        let w = match width_spec.mode() {
            crate::ui::measure_spec::Mode::Exactly => width_spec.size() as f32,
            _ => dp(200.0),
        };
        let h = match height_spec.mode() {
            crate::ui::measure_spec::Mode::Exactly => height_spec.size() as f32,
            _ => dp(4.0),
        };
        self.view.set_measured_dimension(w.max(0.0), h.max(0.0));
    }

    /// 布局。
    pub fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.view.layout(left, top, right, bottom);
    }

    /// 绘制。
    pub fn draw(&self, scene: &mut Scene) {
        let t = theme::theme();
        let h = self.view.height().min(dp(4.0));
        let w = self.view.width();
        let y = self.view.top();
        // 轨。
        scene.commands.push(crate::gfx::scene::Command::FillRect {
            rect: crate::gfx::scene::Rect {
                x: f64::from(self.view.left()),
                y: f64::from(y),
                w: f64::from(w),
                h: f64::from(h),
            },
            paint: crate::gfx::paint::Paint::default().color(t.roles.surface_container_highest),
        });
        // active 段：determinate 全宽比；indeterminate 1/3 宽度往返。
        let seg_w = if self.indeterminate {
            w / 3.0
        } else {
            w * self.progress
        };
        let seg_x = if self.indeterminate {
            self.view.left() + (w - seg_w) * self.phase
        } else {
            self.view.left()
        };
        if seg_w > 0.0 {
            scene.commands.push(crate::gfx::scene::Command::FillRect {
                rect: crate::gfx::scene::Rect {
                    x: f64::from(seg_x),
                    y: f64::from(y),
                    w: f64::from(seg_w),
                    h: f64::from(h),
                },
                paint: crate::gfx::paint::Paint::default().color(t.roles.primary),
            });
        }
    }
}

impl Default for LinearProgressIndicator {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for LinearProgressIndicator {
    fn view(&self) -> &View {
        &self.view
    }
    fn view_mut(&mut self) -> &mut View {
        &mut self.view
    }
    fn measure(&mut self, ws: MeasureSpec, hs: MeasureSpec) {
        LinearProgressIndicator::measure(self, ws, hs)
    }
    fn layout(&mut self, l: f32, t: f32, r: f32, b: f32) {
        LinearProgressIndicator::layout(self, l, t, r, b)
    }
    fn draw(&self, scene: &mut Scene) {
        LinearProgressIndicator::draw(self, scene)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// 圆形进度指示器（48dp）。
pub struct CircularProgressIndicator {
    /// 基础视图。
    pub view: View,
    /// 进度（determinate 0.0-1.0）。
    progress: f32,
    /// indeterminate 旋转相位（弧度）。
    rotation: f32,
    /// 是否 indeterminate。
    indeterminate: bool,
}

impl CircularProgressIndicator {
    /// 创建 determinate 指示器。
    pub fn new() -> Self {
        Self {
            view: View::new(),
            progress: 0.0,
            rotation: 0.0,
            indeterminate: false,
        }
    }

    /// 切换 indeterminate 模式。
    pub fn set_indeterminate(&mut self, ind: bool) {
        self.indeterminate = ind;
        self.view.request_layout();
    }

    /// 设置 determinate 进度。
    pub fn set_progress(&mut self, p: f32) {
        self.progress = p.clamp(0.0, 1.0);
        self.view.request_layout();
    }

    /// 推进 indeterminate 旋转（帧回调调用）。
    ///
    /// # 单位
    /// `rotation` 以“圈”为单位（0.0~1.0 回绕），避免长时间动画浮点累积漂移。
    pub fn advance_rotation(&mut self, delta_turns: f32) {
        self.rotation = (self.rotation + delta_turns).rem_euclid(1.0);
        self.view.request_layout();
    }

    /// 测量：48dp。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        let size = dp(48.0);
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
    }

    /// 绘制：圆弧描边（determinate = 进度弧，indeterminate = 90° 弧旋转）。
    pub fn draw(&self, scene: &mut Scene) {
        let t = theme::theme();
        let size = self.view.height().min(self.view.width()).min(dp(48.0));
        let stroke = dp(4.0);
        let r = (size - stroke) / 2.0;
        let cx = self.view.left() + self.view.width() / 2.0;
        let cy = self.view.top() + self.view.height() / 2.0;
        // 圆环（inactive 底轨）。
        let mut ring = crate::gfx::path::Path::circle(f64::from(cx), f64::from(cy), f64::from(r));
        scene.commands.push(crate::gfx::scene::Command::StrokePath {
            path: ring.clone(),
            paint: crate::gfx::paint::Paint::default()
                .color(t.roles.surface_container_highest)
                .stroke_width(stroke),
        });
        // active 弧。
        let (start, sweep) = if self.indeterminate {
            // rotation 以圈存储，绘制时转弧度。
            (
                f64::from(self.rotation) * std::f64::consts::TAU,
                std::f64::consts::FRAC_PI_2,
            )
        } else {
            (
                -std::f64::consts::FRAC_PI_2,
                f64::from(self.progress) * std::f64::consts::TAU,
            )
        };
        if sweep > 0.0 {
            ring = crate::gfx::path::Path::arc(
                f64::from(cx),
                f64::from(cy),
                f64::from(r),
                start,
                start + sweep,
            );
            scene.commands.push(crate::gfx::scene::Command::StrokePath {
                path: ring,
                paint: crate::gfx::paint::Paint::default()
                    .color(t.roles.primary)
                    .stroke_width(stroke),
            });
        }
    }
}

impl Default for CircularProgressIndicator {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for CircularProgressIndicator {
    fn view(&self) -> &View {
        &self.view
    }
    fn view_mut(&mut self) -> &mut View {
        &mut self.view
    }
    fn measure(&mut self, ws: MeasureSpec, hs: MeasureSpec) {
        CircularProgressIndicator::measure(self, ws, hs)
    }
    fn layout(&mut self, l: f32, t: f32, r: f32, b: f32) {
        CircularProgressIndicator::layout(self, l, t, r, b)
    }
    fn draw(&self, scene: &mut Scene) {
        CircularProgressIndicator::draw(self, scene)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// determinate 进度截断与绘制不 panic。
    #[test]
    fn linear_progress_clamps_and_draws() {
        let mut p = LinearProgressIndicator::new();
        p.set_progress(2.0);
        assert_eq!(p.progress, 1.0);
        p.measure(
            MeasureSpec::make(crate::ui::measure_spec::Mode::Exactly, 240),
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 100),
        );
        p.layout(0.0, 0.0, 240.0, 4.0);
        let mut scene = Scene::new(240, 4);
        p.draw(&mut scene);
    }

    /// indeterminate 相位回绕。
    #[test]
    fn circular_phase_wraps() {
        let mut p = CircularProgressIndicator::new();
        p.set_indeterminate(true);
        p.advance_rotation(0.75);
        p.advance_rotation(0.5);
        assert!((p.rotation - 0.25).abs() < 1e-6);
    }
}
