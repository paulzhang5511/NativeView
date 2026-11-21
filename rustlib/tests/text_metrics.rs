//! 文本测量单测：TextMeasurer / TextPaint（Task 13）。

use velloview::gfx::color::Color;
use velloview::gfx::paint::Paint;
use velloview::text::font::load_from_path;
use velloview::text::layout::{TextAlign, TextContext};
use velloview::text::metrics::{TextMeasurer, TextPaint, text_paint_from};

/// 测试字体路径（与 APK assets 同一份 NotoSerifSC）。
const FONT_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../app/src/main/assets/fonts/NotoSerifSC-Regular.ttf"
);

/// 构建测量器（注册 NotoSerifSC）。
fn measurer() -> TextMeasurer {
    let font = load_from_path(std::path::Path::new(FONT_PATH)).expect("fixture font must load");
    TextMeasurer::new(TextContext::new(&font))
}

/// measureText 语义：宽度为正、同文本确定性一致、不同文本可区分。
#[test]
fn measure_text_is_deterministic() {
    let mut m = measurer();
    let tp = TextPaint::default();
    let a = m.measure_text("Hello", &tp);
    let b = m.measure_text("Hello", &tp);
    assert_eq!(a, b);
    assert!(a > 0.0);
    let c = m.measure_text("He", &tp);
    assert!(c < a, "shorter text must measure narrower");
}

/// measureText 空文本为 0（与 Android 一致，不 panic）。
#[test]
fn measure_empty_text_is_zero() {
    let mut m = measurer();
    assert_eq!(m.measure_text("", &TextPaint::default()), 0.0);
}

/// getFontMetrics 语义：ascent/descent 正值、baseline 在行内。
#[test]
fn font_metrics_positive_ascent_descent() {
    let mut m = measurer();
    let metrics = m
        .font_metrics("Ag", &TextPaint::default())
        .expect("metrics must exist");
    assert!(metrics.ascent > 0.0);
    assert!(metrics.descent > 0.0);
    assert!(metrics.baseline > 0.0);
}

/// 字号越大度量越宽。
#[test]
fn larger_font_measures_wider() {
    let mut m = measurer();
    let small = TextPaint {
        font_size: 16.0,
        ..TextPaint::default()
    };
    let large = TextPaint {
        font_size: 64.0,
        ..TextPaint::default()
    };
    assert!(m.measure_text("Same", &large) > m.measure_text("Same", &small));
}

/// text_paint_from：文字色合并自 Paint 主色。
#[test]
fn text_paint_from_merges_paint_color() {
    let mut paint = Paint::default();
    paint.color = Color::rgba(10, 20, 30, 40);
    let tp = text_paint_from(&paint);
    assert_eq!(tp.color, Color::rgba(10, 20, 30, 40));
    // 非文本字段保持 TextPaint 默认。
    assert_eq!(tp.align, TextAlign::Left);
    assert_eq!(tp.font_size, 16.0);
}

/// 布局入口：折行宽度生效（窄宽度多行）。
#[test]
fn layout_with_wrap_produces_lines() {
    let mut m = measurer();
    let layout = m
        .layout(
            "The quick brown fox jumps over the lazy dog",
            Some(100.0),
            &TextPaint::default(),
        )
        .expect("layout must exist");
    assert!(layout.line_count() > 1);
}
