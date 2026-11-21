//! 文本测量：`Paint` 文本参数 → 排版请求桥接 + Android FontMetrics 语义
//! （Task 13，PRD §5.1.4）。
//!
//! # 设计
//! - Android `Paint.measureText` 对应一次完整 parley 排版（无更廉价路径；
//!   缓存复用由上层 View 记账，V1 不做全局测量缓存）；
//! - 字号/对齐/行距等参数从 `gfx::Paint` 的文本扩展字段读取，
//!   避免 UI 层直接触达 parley 类型。

use crate::gfx::color::Color;
use crate::gfx::paint::Paint;

use super::layout::{LineMetricsInfo, TextAlign, TextContext, TextLayout, TextStyleRequest};

/// `Paint` 的文本扩展参数（Android `Paint` 文本子集）。
///
/// # 设计
/// 独立于 `gfx::Paint` 主结构（文本是 L2 关注点，主结构保持 L1 纯图形），
/// TextView 等 UI 层组装时合并进 `TextStyleRequest`。
#[derive(Clone, Debug)]
pub struct TextPaint {
    /// 字号（px，Android `paint.textSize` 语义）。
    pub font_size: f32,
    /// 对齐。
    pub align: TextAlign,
    /// 文字颜色（默认取 `Paint::color`，由 `text_paint_from` 合并）。
    pub color: Color,
    /// 行距倍率（Android `lineSpacingMult` 近似：parley FontSizeRelative）。
    pub line_spacing_multiplier: f32,
}

impl Default for TextPaint {
    /// Android 默认：12sp→px 由上层 dp 换算；此处 16.0 与 parley 默认一致。
    fn default() -> Self {
        Self {
            font_size: 16.0,
            align: TextAlign::Left,
            color: Color::BLACK,
            line_spacing_multiplier: 1.0,
        }
    }
}

/// 从图形 `Paint` 提取文本参数（颜色合并语义：文字色 = Paint 主色）。
pub fn text_paint_from(paint: &Paint) -> TextPaint {
    TextPaint {
        color: paint.color,
        ..TextPaint::default()
    }
}

/// 文本测量器：持有 `TextContext`，提供 Android 风格测量入口。
pub struct TextMeasurer {
    /// parley 排版上下文（与渲染共享字体注册）。
    context: TextContext,
}

impl TextMeasurer {
    /// 创建测量器（与渲染侧共享同一份 `FontData` 注册源）。
    pub fn new(context: TextContext) -> Self {
        Self { context }
    }

    /// Android `Paint.measureText` 语义：排版后取宽度。
    ///
    /// 空文本宽度为 0（与 Android 一致）。
    pub fn measure_text(&mut self, text: &str, tp: &TextPaint) -> f32 {
        match self.layout(text, None, tp) {
            Some(l) => l.width(),
            None => 0.0,
        }
    }

    /// Android `Paint.getFontMetrics` 语义：首行 metrics。
    pub fn font_metrics(&mut self, text: &str, tp: &TextPaint) -> Option<LineMetricsInfo> {
        self.layout(text, None, tp)?.first_line_metrics()
    }

    /// 排版（measure 与渲染共用入口；max_advance 为折行宽度）。
    pub fn layout(
        &mut self,
        text: &str,
        max_advance: Option<f32>,
        tp: &TextPaint,
    ) -> Option<TextLayout> {
        let req = TextStyleRequest {
            text,
            font_size: tp.font_size,
            max_advance,
            align: tp.align,
            color: tp.color,
            line_spacing_multiplier: tp.line_spacing_multiplier,
        };
        self.context.layout(&req)
    }
}
