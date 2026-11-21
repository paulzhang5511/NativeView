//! 编辑框控件：EditText（SPEC-ime.md §2「控件」）。
//!
//! # 设计动机
//! V1 不做富文本编辑（`Spannable` 输入侧不支持），因此 `EditText` 只是
//! `TextView` 的扩展：文本排版/测量/绘制全部复用，增量只有三件事：
//! 1. **可点击**：点击即请求输入焦点（委托 [`crate::ui::ime::manager`]）；
//! 2. **预编辑区间**：IME 回推的 composing 区（拼音未上屏部分）以系统样式
//!    下划线渲染，确认后清除；
//! 3. **`EditorInfo` 配置**：按控件语义下发 `inputType`/`imeOptions`。
//!
//! # 不变量
//! - composing 区间始终以**字符**下标表达且满足 `start <= end <= 字符数`；
//!   越界按协议错误整体丢弃（不渲染半截下划线）；
//! - 控件本身不持有 IME 连接，也不做任何 JNI 调用——焦点副作用统一经
//!   焦点管理器排队，由平台层在帧回调内消费。

use crate::gfx::color::Color;
use crate::gfx::scene::Scene;
use crate::text::layout::{TextContext, TextLayout};
use crate::ui::ime::manager;
use crate::ui::ime::text_input::{ImeOptions, InputType};
use crate::ui::measure_spec::MeasureSpec;
use crate::ui::view::View;
use crate::ui::widgets::text_view::{Ellipsize, TextAlignment, TextView};
use crate::ui::widgets::widget::Widget;

/// 预编辑下划线颜色（系统 composing 样式近似：浅蓝）。
const COMPOSING_UNDERLINE_COLOR: Color = Color::rgba(0x20, 0x82, 0xF8, 0xFF);

/// 下划线距基线偏移（px，正值向下）。
const UNDERLINE_BASELINE_OFFSET: f32 = 2.0;

/// 下划线粗细（px）。
const UNDERLINE_THICKNESS: f32 = 2.0;

/// 单行编辑框。
pub struct EditText {
    /// 内嵌文本控件（排版/绘制复用）。
    text_view: TextView,
    /// 输入类型（下拉软键盘类型）。
    input_type: InputType,
    /// 回车键语义。
    ime_options: ImeOptions,
    /// 当前 composing 区间（字符下标，左闭右开）。
    composing: Option<(usize, usize)>,
    /// 是否已持有输入焦点（本地视图，焦点管理器为准）。
    focused: bool,
}

impl EditText {
    /// 创建空编辑框（默认文本类型 + Done 语义）。
    pub fn new() -> Self {
        let mut text_view = TextView::new();
        // 编辑框默认可点击（点击请求焦点）。
        text_view.view.set_clickable(true);
        Self {
            text_view,
            input_type: InputType::Text,
            ime_options: ImeOptions::Done,
            composing: None,
            focused: false,
        }
    }

    /// 注入排版上下文（必经入口，未注入时文本不排版）。
    pub fn set_context(&mut self, cx: TextContext) {
        self.text_view.set_context(cx);
    }

    /// 设置文本内容。
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text_view.set_text(text);
    }

    /// 当前文本。
    pub fn text(&self) -> &str {
        self.text_view.text()
    }

    /// 设置字号。
    pub fn set_font_size(&mut self, size: f32) {
        self.text_view.set_font_size(size);
    }

    /// 按 sp 设置字号（随用户字体缩放；编辑文本应优先使用）。
    pub fn set_text_size_sp(&mut self, size_sp: f32) {
        self.text_view.set_text_size_sp(size_sp);
    }

    /// 设置文字颜色。
    pub fn set_text_color(&mut self, color: Color) {
        self.text_view.set_text_color(color);
    }

    /// 设置对齐。
    pub fn set_text_alignment(&mut self, a: TextAlignment) {
        self.text_view.set_text_alignment(a);
    }

    /// 设置省略策略。
    pub fn set_ellipsize(&mut self, e: Ellipsize) {
        self.text_view.set_ellipsize(e);
    }

    /// 设置输入类型（影响系统键盘面板）。
    pub fn set_input_type(&mut self, t: InputType) {
        self.input_type = t;
    }

    /// 当前输入类型。
    pub fn input_type(&self) -> InputType {
        self.input_type
    }

    /// 设置回车键语义。
    pub fn set_ime_options(&mut self, o: ImeOptions) {
        self.ime_options = o;
    }

    /// 当前回车键语义。
    pub fn ime_options(&self) -> ImeOptions {
        self.ime_options
    }

    /// 是否持有输入焦点。
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// 设置焦点态（由平台层同步系统焦点变化时调用）。
    pub fn set_focused(&mut self, focused: bool) {
        if self.focused == focused {
            return;
        }
        self.focused = focused;
        if !focused {
            // 失焦必须清除预编辑态，否则重聚焦会残留半截下划线。
            self.composing = None;
        }
        self.text_view.view.request_layout();
    }

    /// IME 回推文本与 composing 区间（平台层转发）。
    ///
    /// # 参数
    /// `text`：全文（Kotlin `TextWatcher` 的完整内容，非增量）；
    /// `composing`：预编辑区间（字符下标）；`None` = 无预编辑。
    ///
    /// # 返回
    /// true = 渲染状态变化需要重绘。
    pub fn on_ime_text(&mut self, text: &str, composing: Option<(usize, usize)>) -> bool {
        let char_count = text.chars().count();
        // 区间越界按协议错误丢弃（保留上一帧合法状态）。
        let composing = match composing {
            Some((start, end)) if start <= end && end <= char_count && start != end => {
                Some((start, end))
            }
            _ => None,
        };
        let text_changed = self.text_view.text() != text;
        let composing_changed = self.composing != composing;
        if text_changed {
            self.text_view.set_text(text);
        }
        if composing_changed {
            self.composing = composing;
            self.text_view.view.request_layout();
        }
        text_changed || composing_changed
    }

    /// 当前 composing 区间（字符下标）。
    pub fn composing(&self) -> Option<(usize, usize)> {
        self.composing
    }

    /// 点击：请求输入焦点（经焦点管理器排队，帧回调内下发平台）。
    fn request_focus(&mut self) {
        self.focused = true;
        self.text_view.view.request_layout();
        manager::request_focus(self.input_type, self.ime_options, self.text().to_string());
    }

    /// 测量（委托内嵌 TextView）。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        self.text_view.measure(width_spec, height_spec);
    }

    /// 布局（委托内嵌 TextView）。
    pub fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.text_view.layout(left, top, right, bottom);
    }

    /// 绘制：背景 + 文本 + composing 下划线 + 插入光标。
    pub fn draw(&self, scene: &mut Scene) {
        self.text_view.draw(scene);
        let layout = self.layout_ref();
        if let (Some((start, end)), Some(layout)) = (self.composing, layout) {
            self.emit_composing_underline(scene, layout, start, end);
        }
        if self.focused {
            if let Some(layout) = layout {
                self.emit_caret(scene, layout);
            }
        }
    }

    /// 插入光标（V1 仅支持光标在文本末尾，无选区模型）。
    ///
    /// # 坐标
    /// x 取最后一行内容宽度（行尾空白不计），y 取 baseline 上方一个字号。
    fn emit_caret(&self, scene: &mut Scene, layout: &TextLayout) {
        let origin_x = f64::from(self.text_view.view.left() + self.text_view.view.padding_left());
        let origin_y = f64::from(self.text_view.view.top() + self.text_view.view.padding_top());
        let baseline_local = match layout.all_line_glyphs().last() {
            Some(line) => line.baseline,
            None => return,
        };
        let caret_x = origin_x + f64::from(layout.width()) + 1.0;
        let baseline = origin_y + f64::from(baseline_local);
        let size = f64::from(layout.font_size());
        // 2dp 宽，高一个字号（高密度设备上仍清晰可见）。
        let w = f64::from(crate::ui::dimen::dp(2.0)).max(1.0);
        scene.commands.push(crate::gfx::scene::Command::FillRect {
            rect: crate::gfx::scene::Rect {
                x: caret_x,
                y: baseline - size,
                w,
                h: size,
            },
            paint: crate::gfx::paint::Paint::default()
                .color(crate::gfx::color::Color::rgb(0x20, 0x82, 0xF8)),
        });
    }

    /// 取排版缓存（draw 期只读）。
    fn layout_ref(&self) -> Option<&TextLayout> {
        self.text_view.layout_cache()
    }

    /// 输出 composing 下划线（逐行覆盖预编辑区间）。
    ///
    /// # 坐标
    /// 字形 x 为基线系相对布局原点的偏移，故下划线 x 需加视图左/上内边距
    /// 与视图原点，y 取该行 baseline 下移常量。
    fn emit_composing_underline(
        &self,
        scene: &mut Scene,
        layout: &TextLayout,
        start: usize,
        end: usize,
    ) {
        let origin_x = f64::from(self.text_view.view.left() + self.text_view.view.padding_left());
        let origin_y = f64::from(self.text_view.view.top() + self.text_view.view.padding_top());
        // 按字形累计字符下标（V1 假设 1 字形 ≈ 1 字符，与 parley 排版一致）。
        let mut char_index = 0usize;
        for line in layout.all_line_glyphs() {
            let line_start_char = char_index;
            let glyph_count = line.glyphs.len();
            char_index += glyph_count;
            let line_end_char = char_index;
            // 本行与 [start, end) 的交集。
            let seg_start = start.max(line_start_char);
            let seg_end = end.min(line_end_char);
            if seg_start >= seg_end {
                continue;
            }
            let local_start = seg_start - line_start_char;
            let local_end = seg_end - line_start_char;
            let Some(first) = line.glyphs.get(local_start) else {
                continue;
            };
            let Some(last) = line.glyphs.get(local_end - 1) else {
                continue;
            };
            // 宽度取首末字形 x 之差加一个字号宽（末字形自身宽度近似）。
            let x0 = origin_x + f64::from(first.x);
            let x1 = origin_x + f64::from(last.x) + f64::from(layout.font_size());
            let y = origin_y + f64::from(line.baseline) + f64::from(UNDERLINE_BASELINE_OFFSET);
            scene.commands.push(crate::gfx::scene::Command::FillRect {
                rect: crate::gfx::scene::Rect {
                    x: x0,
                    y,
                    w: (x1 - x0).max(0.0),
                    h: f64::from(UNDERLINE_THICKNESS),
                },
                paint: crate::gfx::paint::Paint::default().color(COMPOSING_UNDERLINE_COLOR),
            });
        }
    }
}

impl Default for EditText {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for EditText {
    fn view(&self) -> &View {
        &self.text_view.view
    }
    fn view_mut(&mut self) -> &mut View {
        &mut self.text_view.view
    }
    fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        EditText::measure(self, width_spec, height_spec)
    }
    fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        EditText::layout(self, left, top, right, bottom)
    }
    fn draw(&self, scene: &mut Scene) {
        EditText::draw(self, scene)
    }
    /// 触摸：点击即请求焦点（编辑框不参与拖拽/滚动手势）。
    fn dispatch_touch(&mut self, event: &crate::ui::input::MotionEvent) -> bool {
        if event.action() != crate::ui::input::Action::Up {
            // DOWN 也消费，避免系统把点击判给父级滚动容器。
            return true;
        }
        self.request_focus();
        true
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::measure_spec::Mode;

    /// 默认配置：文本类型 + Done。
    #[test]
    fn defaults() {
        let e = EditText::new();
        assert_eq!(e.input_type(), InputType::Text);
        assert_eq!(e.ime_options(), ImeOptions::Done);
        assert!(!e.is_focused());
        assert_eq!(e.composing(), None);
        assert!(e.view().is_clickable());
    }

    /// 配置可变更。
    #[test]
    fn config_setters() {
        let mut e = EditText::new();
        e.set_input_type(InputType::Number);
        e.set_ime_options(ImeOptions::Search);
        assert_eq!(e.input_type(), InputType::Number);
        assert_eq!(e.ime_options(), ImeOptions::Search);
    }

    /// IME 回推：合法 composing 区间被接受。
    #[test]
    fn accepts_valid_composing() {
        let mut e = EditText::new();
        assert!(e.on_ime_text("ni", Some((0, 2))));
        assert_eq!(e.text(), "ni");
        assert_eq!(e.composing(), Some((0, 2)));
    }

    /// 空区间（start == end）等价于无预编辑。
    #[test]
    fn empty_composing_is_none() {
        let mut e = EditText::new();
        e.on_ime_text("你好", Some((1, 1)));
        assert_eq!(e.composing(), None);
    }

    /// 越界区间被丢弃（end 超字符数）。
    #[test]
    fn rejects_out_of_range() {
        let mut e = EditText::new();
        e.on_ime_text("ab", Some((0, 5)));
        assert_eq!(e.composing(), None);
        // start > end 同样非法。
        e.on_ime_text("abcd", Some((3, 1)));
        assert_eq!(e.composing(), None);
    }

    /// 多字节字符按字符计数（中文不按字节判越界）。
    #[test]
    fn multibyte_length_uses_chars() {
        let mut e = EditText::new();
        // "你好" 6 字节但 2 字符。
        assert!(e.on_ime_text("你好", Some((0, 2))));
        assert_eq!(e.composing(), Some((0, 2)));
    }

    /// composing 清除后渲染状态变化。
    #[test]
    fn clearing_composing_reports_change() {
        let mut e = EditText::new();
        e.on_ime_text("ni", Some((0, 2)));
        assert!(e.on_ime_text("ni", None), "composing cleared must dirty");
        assert_eq!(e.composing(), None);
    }

    /// 文本未变且 composing 未变：不产生多余重绘。
    #[test]
    fn no_change_no_dirty() {
        let mut e = EditText::new();
        e.on_ime_text("abc", Some((0, 1)));
        assert!(!e.on_ime_text("abc", Some((0, 1))));
    }

    /// 失焦清除预编辑态。
    #[test]
    fn blur_clears_composing() {
        let mut e = EditText::new();
        e.set_focused(true);
        e.on_ime_text("ni", Some((0, 2)));
        assert_eq!(e.composing(), Some((0, 2)));
        e.set_focused(false);
        assert_eq!(e.composing(), None);
        assert!(!e.is_focused());
    }

    /// 无字体降级：不 panic，测量为 0。
    #[test]
    fn no_font_degrades_gracefully() {
        let mut e = EditText::new();
        let ws = MeasureSpec::make(Mode::AtMost, 200);
        let hs = MeasureSpec::make(Mode::AtMost, 100);
        e.measure(ws, hs);
        e.layout(0.0, 0.0, 200.0, 40.0);
        let mut scene = Scene::new(200, 100);
        e.draw(&mut scene);
        assert_eq!(e.view().measured_width(), 0.0);
    }
}
