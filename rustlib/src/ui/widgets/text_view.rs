//! 文本控件：TextView（PRD §5.1.7，Task 16）。
//!
//! # 设计
//! - 持有 `text::TextMeasurer` 按需排版（measure 用测量、draw 用绘制，
//!   同一 TextLayout 缓存到字段，避免一帧双排版）；
//! - Ellipsize V1 仅 END（PRD 子集）。

use crate::gfx::scene::Scene;
use crate::text::layout::{TextContext, TextLayout};
use crate::text::metrics::{TextMeasurer, TextPaint};
use crate::ui::measure_spec::{MeasureSpec, Mode};
use crate::ui::view::View;
/// 文本对齐（映射到 text::layout::TextAlign）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlignment {
    /// 左。
    #[default]
    Left,
    /// 居中。
    Center,
    /// 右。
    Right,
}

/// 文本省略策略（V1 仅 END）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ellipsize {
    /// 不省略（溢出裁剪）。
    #[default]
    None,
    /// 尾部省略号（Android `TruncateAt.END`）。
    End,
}

/// 文本视图。
pub struct TextView {
    /// 自身 View 状态。
    pub view: View,
    /// 文本内容。
    text: String,
    /// 文本参数。
    text_paint: TextPaint,
    /// 对齐。
    alignment: TextAlignment,
    /// 省略策略。
    ellipsize: Ellipsize,
    /// 排版缓存（measure 填充，draw 消费）。
    layout: Option<TextLayout>,
    /// 省略后的实际绘制文本（measure 填充）。
    display_text: String,
    /// 排版测量器（持有 `TextContext`，由 `set_context` 注入后可用）。
    measurer: Option<TextMeasurer>,
}

impl TextView {
    /// 创建（排版上下文由 `set_context` 注入）。
    pub fn new() -> Self {
        Self {
            view: View::new(),
            text: String::new(),
            text_paint: TextPaint::default(),
            alignment: TextAlignment::Left,
            ellipsize: Ellipsize::None,
            layout: None,
            display_text: String::new(),
            measurer: None,
        }
    }

    /// 注入排版上下文（进程单例语义，宿主装配时调用）。
    pub fn set_context(&mut self, cx: TextContext) {
        self.measurer = Some(TextMeasurer::new(cx));
    }

    /// 设置文本。
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.layout = None;
        self.view.request_layout();
    }

    /// 当前文本。
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 排版缓存只读访问（供扩展控件复用已排版结果，避免二次排版）。
    pub fn layout_cache(&self) -> Option<&TextLayout> {
        self.layout.as_ref()
    }

    /// 设置字号。
    pub fn set_font_size(&mut self, size: f32) {
        self.text_paint.font_size = size;
        self.layout = None;
        self.view.request_layout();
    }

    /// 按 sp 设置字号（Android `setTextSize(COMPLEX_UNIT_SP, ...)` 语义）。
    ///
    /// # 为什么优先用 sp
    /// sp 随用户系统字体大小缩放（无障碍要求），dp/px 不随。
    /// 界面文字应统一走此入口，仅在需要「不随字体缩放」的图标文字等
    /// 场景才用 [`Self::set_font_size`] 传 px。
    pub fn set_text_size_sp(&mut self, size_sp: f32) {
        self.set_font_size(crate::ui::dimen::sp(size_sp));
    }

    /// 当前字号（px）。
    pub fn font_size(&self) -> f32 {
        self.text_paint.font_size
    }

    /// 设置文字颜色。
    pub fn set_text_color(&mut self, color: crate::gfx::color::Color) {
        self.text_paint.color = color;
        self.layout = None;
        self.view.request_layout();
    }

    /// 设置对齐。
    pub fn set_text_alignment(&mut self, a: TextAlignment) {
        self.alignment = a;
        self.text_paint.align = match a {
            TextAlignment::Left => crate::text::layout::TextAlign::Left,
            TextAlignment::Center => crate::text::layout::TextAlign::Center,
            TextAlignment::Right => crate::text::layout::TextAlign::Right,
        };
        self.layout = None;
        self.view.request_layout();
    }

    /// 读取对齐。
    pub fn text_alignment(&self) -> TextAlignment {
        self.alignment
    }

    /// 行距倍率。
    pub fn set_line_spacing_multiplier(&mut self, m: f32) {
        self.text_paint.line_spacing_multiplier = m;
        self.layout = None;
        self.view.request_layout();
    }

    /// 设置省略策略。
    pub fn set_ellipsize(&mut self, e: Ellipsize) {
        self.ellipsize = e;
        self.layout = None;
        self.view.request_layout();
    }

    /// 读取省略策略。
    pub fn ellipsize(&self) -> Ellipsize {
        self.ellipsize
    }

    /// 测量：WRAP 时取排版尺寸 + padding；END 省略时截断到单行。
    pub fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        let max_advance = match width_spec.mode() {
            Mode::Unspecified => None,
            _ => Some(
                width_spec.size() as f32 - self.view.padding_left() - self.view.padding_right(),
            ),
        };
        if let Some(measurer) = self.measurer.as_mut() {
            self.display_text = self.text.clone();
            self.layout = measurer.layout(&self.text, max_advance, &self.text_paint);
            // END 省略：内容超出可用宽度时二分截断并追加省略号，
            // 重新排版为单行（Android TruncateAt.END 语义子集）。
            if self.ellipsize == Ellipsize::End {
                if let (Some(avail), Some(cur)) =
                    (max_advance, self.layout.as_ref().map(|l| l.width()))
                {
                    if cur > avail && !self.text.is_empty() {
                        if let Some(truncated) = Self::truncate_with_ellipsis(
                            &self.text,
                            &self.text_paint,
                            measurer,
                            avail,
                        ) {
                            self.display_text = truncated;
                            self.layout =
                                measurer.layout(&self.display_text, Some(avail), &self.text_paint);
                        }
                    }
                }
            }
        }
        let (w, h) = match &self.layout {
            Some(l) => (
                l.width() + self.view.padding_left() + self.view.padding_right(),
                l.height() + self.view.padding_top() + self.view.padding_bottom(),
            ),
            None => (0.0, 0.0),
        };
        let final_w = match width_spec.mode() {
            Mode::Exactly => width_spec.size() as f32,
            _ => w,
        };
        let final_h = match height_spec.mode() {
            Mode::Exactly => height_spec.size() as f32,
            _ => h,
        };
        self.view.set_measured_dimension(final_w, final_h);
    }

    /// 布局。
    pub fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.view.layout(left, top, right, bottom);
    }

    /// 二分截断：找最长前缀 + 省略号适配可用宽度。
    fn truncate_with_ellipsis(
        text: &str,
        paint: &TextPaint,
        measurer: &mut TextMeasurer,
        avail: f32,
    ) -> Option<String> {
        // 完整文本加省略号即可容纳（罕见：省略号宽度 < 溢出量）。
        let with_ellipsis = format!("{text}\u{2026}");
        if let Some(l) = measurer.layout(&with_ellipsis, Some(avail), paint) {
            if l.width() <= avail {
                return Some(with_ellipsis);
            }
        }
        // 字符（非字节）级二分：找首个适配的字符前缀。
        let chars: Vec<char> = text.chars().collect();
        let mut lo = 0usize;
        let mut hi = chars.len();
        let mut best: Option<String> = None;
        while lo < hi {
            let mid = (lo + hi) / 2;
            let candidate: String = chars[..mid].iter().collect();
            let candidate = format!("{candidate}\u{2026}");
            let fits = measurer
                .layout(&candidate, Some(avail), paint)
                .map(|l| l.width() <= avail)
                .unwrap_or(false);
            if fits {
                best = Some(candidate);
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        best
    }

    /// 绘制：背景 + 文本（经 Scene 中间表示，颜色进 DrawGlyphs 命令）。
    pub fn draw(&self, scene: &mut Scene) {
        self.view.draw(scene);
        if let Some(layout) = &self.layout {
            let x = f64::from(self.view.left() + self.view.padding_left());
            let y = f64::from(self.view.top() + self.view.padding_top());
            crate::text::render::emit_layout(scene, layout, x, y);
        }
    }
}

impl Default for TextView {
    fn default() -> Self {
        Self::new()
    }
}

/// TextView 的 Widget 实现（叶子：默认手势推进即可，无子树）。
impl crate::ui::widgets::widget::Widget for TextView {
    fn view(&self) -> &View {
        &self.view
    }
    fn view_mut(&mut self) -> &mut View {
        &mut self.view
    }
    fn measure(&mut self, width_spec: MeasureSpec, height_spec: MeasureSpec) {
        TextView::measure(self, width_spec, height_spec)
    }
    fn layout(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        TextView::layout(self, left, top, right, bottom)
    }
    fn draw(&self, scene: &mut Scene) {
        TextView::draw(self, scene)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
