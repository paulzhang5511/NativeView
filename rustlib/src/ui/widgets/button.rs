//! 按钮控件：Button（PRD §5.1.7，Task 16）。
//!
//! # 范围
//! V1 无状态按钮：TextView + 圆角底色背景的静态组合；
//! 点击按压反馈由 Task 17/18 输入链接入（press 状态机）。

use super::text_view::TextView;
use crate::gfx::color::Color;
use crate::gfx::scene::Scene;
use crate::ui::measure_spec::MeasureSpec;
/// 按钮。
pub struct Button {
    /// 文本控件（承载排版与文本绘制）。
    pub text_view: TextView,
    /// 圆角半径（px）。
    corner_radius: f32,
    /// 按钮背景色。
    background: Color,
    /// 按压态（按下时背景加深；同 Android `state_pressed`）。
    pressed: bool,
}

impl Button {
    /// 创建（默认 4dp 圆角、深灰背景）。
    ///
    /// # 尺寸语义
    /// 圆角是视觉尺寸，必须随设备密度缩放（4dp 在 640dpi 为 16px）；
    /// 写成裸 px 会在高密度设备上退化为近乎直角。
    pub fn new() -> Self {
        let mut text_view = TextView::new();
        // 按钮文本默认白色（深底）。
        text_view.set_text_color(Color::WHITE);
        Self {
            text_view,
            corner_radius: crate::ui::dimen::dp(4.0),
            // Material 深灰（非纯黑，保证与文本对比度）。
            background: Color {
                r: 51,
                g: 51,
                b: 51,
                a: 255,
            },
            pressed: false,
        }
    }

    /// 设置文本。
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text_view.set_text(text);
    }

    /// 当前文本。
    pub fn text(&self) -> &str {
        self.text_view.text()
    }

    /// 设置背景色。
    pub fn set_background_color(&mut self, color: Color) {
        self.background = color;
        self.text_view.view.request_layout();
    }

    /// 测量（委托文本测量 + 语义扩边：按钮默认水平 padding）。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        self.text_view.measure(width_spec, height_spec);
    }

    /// 布局。
    pub fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.text_view.layout(left, top, right, bottom);
    }

    /// 按压态。
    pub fn is_pressed(&self) -> bool {
        self.pressed
    }

    /// 设置按压态（仅状态位；重绘由输入层消费 Touch 后统一触发）。
    pub fn set_pressed(&mut self, pressed: bool) {
        self.pressed = pressed;
    }

    /// 实际绘制背景色：按压态整体压暗约 18%（与 Material ripple 静态等价）。
    fn effective_background(&self) -> Color {
        if !self.pressed {
            return self.background;
        }
        Color {
            r: (u32::from(self.background.r) * 82 / 100) as u8,
            g: (u32::from(self.background.g) * 82 / 100) as u8,
            b: (u32::from(self.background.b) * 82 / 100) as u8,
            a: self.background.a,
        }
    }

    /// 绘制：圆角矩形背景 + 文本。
    pub fn draw(&self, scene: &mut Scene) {
        let v = &self.text_view.view;
        scene.commands.push(crate::gfx::scene::Command::FillPath {
            path: crate::gfx::path::Path::rounded_rect(
                f64::from(v.left()),
                f64::from(v.top()),
                f64::from(v.width()),
                f64::from(v.height()),
                f64::from(self.corner_radius),
            ),
            paint: crate::gfx::paint::Paint::default().color(self.effective_background()),
        });
        self.text_view.draw(scene);
    }
}

impl Default for Button {
    fn default() -> Self {
        Self::new()
    }
}

/// Button 的 Widget 实现（叶子）。
impl crate::ui::widgets::widget::Widget for Button {
    fn view(&self) -> &crate::ui::view::View {
        &self.text_view.view
    }
    fn view_mut(&mut self) -> &mut crate::ui::view::View {
        &mut self.text_view.view
    }
    fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        Button::measure(self, width_spec, height_spec)
    }
    fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        Button::layout(self, left, top, right, bottom)
    }
    fn draw(&self, scene: &mut Scene) {
        Button::draw(self, scene)
    }

    /// 触摸：DOWN 置按压态、UP/CANCEL 复位。
    ///
    /// # 返回
    /// 手势序列内的所有事件都消费（按下即视为本按钮接管），
    /// 避免父容器在同序列中再次分发。
    fn dispatch_touch(&mut self, event: &crate::ui::input::MotionEvent) -> bool {
        match event.action() {
            crate::ui::input::Action::Down => {
                self.pressed = true;
                true
            }
            crate::ui::input::Action::Move => self.pressed,
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
    use crate::ui::input::{Action, MotionEvent, PointerCoords};

    fn event(action: i32) -> MotionEvent {
        MotionEvent {
            packed_action: action,
            down_time: 0,
            event_time: 0,
            pointers: vec![PointerCoords {
                pointer_id: 0,
                x: 0.0,
                y: 0.0,
                pressure: 0.5,
            }],
            historical: vec![],
        }
    }

    /// 按压态：DOWN 置位（背景压暗）、UP 复位。
    #[test]
    fn press_state_toggles_on_touch() {
        use crate::ui::widgets::widget::Widget;
        let mut b = Button::new();
        b.set_background_color(Color::rgb(0x40, 0x40, 0x40));
        let normal = b.effective_background();
        assert!(b.dispatch_touch(&event(Action::DOWN)));
        assert!(b.is_pressed());
        assert!(
            b.effective_background().r < normal.r,
            "pressed background must be darker"
        );
        assert!(b.dispatch_touch(&event(Action::UP)));
        assert!(!b.is_pressed());
        assert_eq!(b.effective_background(), normal);
    }
}
