//! M3 Snackbar（Material 3）。
//!
//! 底部浮出：`inverse_surface` 底 4dp 圆角，action 文字 `inverse_primary`；
//! 4s 自动消失由宿主 choreographer 驱动（组件本身无时钟）。

use crate::gfx::scene::Scene;
use crate::text::layout::TextContext;
use crate::ui::dimen::dp;
use crate::ui::measure_spec::MeasureSpec;
use crate::ui::theme;
use crate::ui::view::View;
use crate::ui::widgets::text_view::TextView;
use crate::ui::widgets::widget::Widget;

/// M3 Snackbar。
pub struct Snackbar {
    /// 基础视图。
    pub view: View,
    /// 消息文本。
    pub text_view: TextView,
    /// action 文本（可选）。
    pub action_view: TextView,
    /// 是否有 action。
    has_action: bool,
    /// 剩余展示时间（秒；<=0 隐藏）。
    pub remaining: f32,
}

impl Snackbar {
    /// M3 展示时长（秒）。
    pub const DURATION: f32 = 4.0;

    /// 创建隐藏状态的 snackbar。
    pub fn new() -> Self {
        let mut text_view = TextView::new();
        text_view.set_text_style(theme::typography::SCALE.body_medium);
        text_view.set_text_color(theme::theme().roles.inverse_on_surface);
        let mut action_view = TextView::new();
        action_view.set_text_style(theme::typography::SCALE.label_large);
        action_view.set_text_color(theme::theme().roles.inverse_primary);
        Self {
            view: View::new(),
            text_view,
            action_view,
            has_action: false,
            remaining: 0.0,
        }
    }

    /// 显示（重置倒计时）。
    pub fn show(&mut self) {
        self.remaining = Self::DURATION;
    }

    /// 立即隐藏。
    pub fn dismiss(&mut self) {
        self.remaining = 0.0;
    }

    /// 是否可见。
    pub fn is_showing(&self) -> bool {
        self.remaining > 0.0
    }

    /// 帧推进（每帧由 choreographer 调用）。
    pub fn advance(&mut self, dt: f32) {
        if self.remaining > 0.0 {
            self.remaining = (self.remaining - dt).max(0.0);
        }
    }

    /// 设置消息文本。
    pub fn set_message(&mut self, s: impl Into<String>) {
        self.text_view.set_text(s);
        self.view.request_layout();
    }

    /// 设置 action 文本（空串清除 action）。
    pub fn set_action(&mut self, s: &str) {
        self.has_action = !s.is_empty();
        self.action_view.set_text(s);
        self.view.request_layout();
    }

    /// 注入排版上下文（消息与 action 各一份，因 TextContext 不支持 Clone）。
    pub fn set_contexts(&mut self, msg_cx: TextContext, action_cx: TextContext) {
        self.text_view.set_context(msg_cx);
        self.action_view.set_context(action_cx);
    }

    /// 测量：文本包裹 + 8dp 内边距（action 占 48dp 宽槽）。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        self.text_view.measure(width_spec, height_spec);
        self.action_view.measure(width_spec, height_spec);
        let tw = self.text_view.view.measured_width();
        let th = self.text_view.view.measured_height();
        let aw = if self.has_action { dp(48.0) } else { 0.0 };
        let w = tw + dp(16.0) + aw;
        let h = (th.max(dp(48.0))) + dp(16.0);
        self.view.set_measured_dimension(w, h);
    }

    /// 布局。
    pub fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.view.layout(left, top, right, bottom);
        // 消息左对齐 16dp，action 右对齐。
        let th = self.text_view.view.measured_height();
        let ty = top + (bottom - top - th) / 2.0;
        self.text_view.layout(left + dp(16.0), ty, right, ty + th);
        if self.has_action {
            let aw = self.action_view.view.measured_width();
            let ah = self.action_view.view.measured_height();
            let ax = right - dp(16.0) - aw;
            let ay = top + (bottom - top - ah) / 2.0;
            self.action_view.layout(ax, ay, ax + aw, ay + ah);
        }
    }

    /// 绘制：inverse 底 4dp 圆角 + 文本（隐藏时跳过）。
    pub fn draw(&self, scene: &mut Scene) {
        if !self.is_showing() {
            return;
        }
        let t = theme::theme();
        scene.commands.push(crate::gfx::scene::Command::FillPath {
            path: crate::gfx::path::Path::rounded_rect(
                f64::from(self.view.left()),
                f64::from(self.view.top()),
                f64::from(self.view.width()),
                f64::from(self.view.height()),
                f64::from(dp(4.0)),
            ),
            paint: crate::gfx::paint::Paint::default().color(t.roles.inverse_surface),
        });
        self.text_view.draw(scene);
        if self.has_action {
            self.action_view.draw(scene);
        }
    }
}

impl Default for Snackbar {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Snackbar {
    fn view(&self) -> &View {
        &self.view
    }
    fn view_mut(&mut self) -> &mut View {
        &mut self.view
    }
    fn measure(&mut self, ws: MeasureSpec, hs: MeasureSpec) {
        Snackbar::measure(self, ws, hs)
    }
    fn layout(&mut self, l: f32, t: f32, r: f32, b: f32) {
        Snackbar::layout(self, l, t, r, b)
    }
    fn draw(&self, scene: &mut Scene) {
        Snackbar::draw(self, scene)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 4 秒倒计时归零后隐藏。
    #[test]
    fn auto_dismiss_after_4s() {
        let mut s = Snackbar::new();
        assert!(!s.is_showing());
        s.show();
        assert!(s.is_showing());
        s.advance(3.9);
        assert!(s.is_showing());
        s.advance(0.2);
        assert!(!s.is_showing());
    }

    /// 隐藏时不产出绘制命令。
    #[test]
    fn hidden_draws_nothing() {
        let mut s = Snackbar::new();
        s.set_message("hi");
        s.measure(
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 300),
            MeasureSpec::make(crate::ui::measure_spec::Mode::AtMost, 100),
        );
        s.layout(0.0, 0.0, s.view.measured_width(), s.view.measured_height());
        let mut scene = Scene::new(300, 100);
        s.draw(&mut scene);
        assert!(scene.commands.is_empty());
        s.show();
        s.draw(&mut scene);
        assert!(!scene.commands.is_empty());
    }
}
