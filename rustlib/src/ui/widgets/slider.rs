//! M3 滑块（Material 3 Expressive）。
//!
//! # 设计
//! - 44dp 触控行高，4dp 轨，拇指 4dp 圆点（M3 新式滑块）；
//! - active 轨 primary、inactive 轨 `surface_container_highest`；
//! - 拖动范围 0.0-1.0（归一化，宿主自行映射业务值）。

use crate::gfx::scene::Scene;
use crate::ui::dimen::dp;
use crate::ui::measure_spec::MeasureSpec;
use crate::ui::theme;
use crate::ui::view::View;
use crate::ui::widgets::widget::Widget;

/// M3 滑块。
pub struct Slider {
    /// 基础视图。
    pub view: View,
    /// 当前进度（0.0-1.0）。
    progress: f32,
    /// 按压/拖动态。
    pressed: bool,
}

impl Slider {
    /// 创建（初始进度 0）。
    pub fn new() -> Self {
        let mut view = View::new();
        view.set_clickable(true);
        view.set_a11y_role(crate::ui::a11y::A11yRole::Slider);
        Self {
            view,
            progress: 0.0,
            pressed: false,
        }
    }

    /// 当前进度。
    pub fn progress(&self) -> f32 {
        self.progress
    }

    /// 设置进度（越界截断）。
    pub fn set_progress(&mut self, p: f32) {
        self.progress = p.clamp(0.0, 1.0);
        self.view.request_layout();
    }

    /// 测量：全宽 × 44dp 触控行。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        let w = match width_spec.mode() {
            crate::ui::measure_spec::Mode::Exactly => width_spec.size() as f32,
            _ => dp(200.0),
        };
        let h = match height_spec.mode() {
            crate::ui::measure_spec::Mode::Exactly => height_spec.size() as f32,
            _ => dp(44.0),
        };
        self.view.set_measured_dimension(w.max(0.0), h.max(0.0));
    }

    /// 布局。
    pub fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.view.layout(left, top, right, bottom);
    }

    /// 由触摸 x 计算进度。
    fn update_progress_from_x(&mut self, x: f32) {
        let w = self.view.width();
        if w <= 0.0 {
            return;
        }
        let local = (x - self.view.left()).clamp(0.0, w);
        self.set_progress(local / w);
    }

    /// 绘制：active/inactive 轨 + 拇指点。
    pub fn draw(&self, scene: &mut Scene) {
        let t = theme::theme();
        let track_h = dp(4.0);
        let y = self.view.top() + (self.view.height() - track_h) / 2.0;
        let w = self.view.width();
        let active_w = w * self.progress;
        let thumb_r = dp(2.0);
        let thumb_x = if w > 0.0 {
            self.view.left() + active_w.min(w - thumb_r)
        } else {
            self.view.left() + thumb_r
        };
        let thumb_cy = y + track_h / 2.0;

        // inactive 轨（全宽）。
        scene.commands.push(crate::gfx::scene::Command::FillRect {
            rect: crate::gfx::scene::Rect {
                x: f64::from(self.view.left()),
                y: f64::from(y),
                w: f64::from(w),
                h: f64::from(track_h),
            },
            paint: crate::gfx::paint::Paint::default().color(t.roles.surface_container_highest),
        });
        // active 轨。
        if active_w > 0.0 {
            scene.commands.push(crate::gfx::scene::Command::FillRect {
                rect: crate::gfx::scene::Rect {
                    x: f64::from(self.view.left()),
                    y: f64::from(y),
                    w: f64::from(active_w),
                    h: f64::from(track_h),
                },
                paint: crate::gfx::paint::Paint::default().color(t.roles.primary),
            });
        }
        // 拇指圆点。
        scene.commands.push(crate::gfx::scene::Command::FillPath {
            path: crate::gfx::path::Path::circle(
                f64::from(thumb_x),
                f64::from(thumb_cy),
                f64::from(thumb_r),
            ),
            paint: crate::gfx::paint::Paint::default().color(t.roles.primary),
        });
    }
}

impl Default for Slider {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Slider {
    fn view(&self) -> &View {
        &self.view
    }
    fn view_mut(&mut self) -> &mut View {
        &mut self.view
    }
    fn measure(&mut self, ws: MeasureSpec, hs: MeasureSpec) {
        Slider::measure(self, ws, hs)
    }
    fn layout(&mut self, l: f32, t: f32, r: f32, b: f32) {
        Slider::layout(self, l, t, r, b)
    }
    fn draw(&self, scene: &mut Scene) {
        Slider::draw(self, scene)
    }
    /// 触摸：DOWN/MOVE 拖动更新进度。
    fn dispatch_touch(&mut self, event: &crate::ui::input::MotionEvent) -> bool {
        match event.action() {
            crate::ui::input::Action::Down => {
                self.pressed = true;
                if let Some(p) = event.pointers.first() {
                    self.update_progress_from_x(p.x);
                }
                true
            }
            crate::ui::input::Action::Move => {
                if self.pressed {
                    if let Some(p) = event.pointers.first() {
                        self.update_progress_from_x(p.x);
                    }
                }
                self.pressed
            }
            crate::ui::input::Action::Up | crate::ui::input::Action::Cancel => {
                self.pressed = false;
                true
            }
            _ => false,
        }
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 进度越界截断。
    #[test]
    fn progress_clamps() {
        let mut s = Slider::new();
        s.set_progress(1.5);
        assert_eq!(s.progress(), 1.0);
        s.set_progress(-0.5);
        assert_eq!(s.progress(), 0.0);
    }

    /// 拖动更新进度。
    #[test]
    fn drag_updates_progress() {
        let mut s = Slider::new();
        s.measure(
            MeasureSpec::make(crate::ui::measure_spec::Mode::Exactly, 100),
            MeasureSpec::make(crate::ui::measure_spec::Mode::Exactly, 44),
        );
        s.layout(0.0, 0.0, 100.0, 44.0);
        let ev = crate::ui::input::MotionEvent {
            packed_action: crate::ui::input::Action::DOWN,
            down_time: 0,
            event_time: 0,
            pointers: vec![crate::ui::input::PointerCoords {
                pointer_id: 0,
                x: 50.0,
                y: 22.0,
                pressure: 0.5,
            }],
            historical: vec![],
        };
        use crate::ui::widgets::widget::Widget;
        assert!(s.dispatch_touch(&ev));
        assert!((s.progress() - 0.5).abs() < 1e-6);
    }
}
