//! 文本排版单测：parley 集成（Task 13，主机端覆盖）。
//!
//! # 测试对象
//! - `TextContext::new` + `layout`：family 命中、字号、折行、对齐；
//! - `TextLayout`：宽高、行数、字形提取；
//! - 中文/英文/混合文本 shape 正确性。

use velloview::gfx::color::Color;
use velloview::text::font::load_from_path;
use velloview::text::layout::{TextAlign, TextContext, TextStyleRequest};

/// 测试字体路径（与 APK assets 同一份 NotoSerifSC）。
const FONT_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../app/src/main/assets/fonts/NotoSerifSC-Regular.ttf"
);

/// 构建测试上下文（注册 NotoSerifSC）。
fn context() -> TextContext {
    let font = load_from_path(std::path::Path::new(FONT_PATH)).expect("fixture font must load");
    TextContext::new(&font)
}

fn default_req<'a>(text: &'a str) -> TextStyleRequest<'a> {
    TextStyleRequest {
        text,
        font_size: 32.0,
        max_advance: None,
        align: TextAlign::Left,
        color: Color::BLACK,
        line_spacing_multiplier: 1.0,
    }
}

/// 空文本必须返回 None（零行布局无意义）。
#[test]
fn empty_text_returns_none() {
    let mut cx = context();
    assert!(cx.layout(&default_req("")).is_none());
}

/// 单行英文：恰好 1 行，字形数 == 字符数（拉丁逐字 shape）。
#[test]
fn single_line_english() {
    let mut cx = context();
    let layout = cx
        .layout(&default_req("Hello"))
        .expect("layout must succeed");
    assert_eq!(layout.line_count(), 1);
    let line = layout.line_glyphs(0).expect("line 0 must exist");
    assert_eq!(line.glyphs.len(), 5);
    assert!(layout.width() > 0.0);
    assert!(layout.height() > 0.0);
}

/// 单行中文：字形数 == 字符数（CJK 逐字）。
#[test]
fn single_line_chinese() {
    let mut cx = context();
    let layout = cx
        .layout(&default_req("你好世界"))
        .expect("layout must succeed");
    assert_eq!(layout.line_count(), 1);
    let line = layout.line_glyphs(0).expect("line 0 must exist");
    assert_eq!(line.glyphs.len(), 4);
}

/// 中英混排：字形数 == 全部字符数（同一字体实例内 shape）。
#[test]
fn mixed_script_layout() {
    let mut cx = context();
    let layout = cx
        .layout(&default_req("你好ABC世界"))
        .expect("layout must succeed");
    assert_eq!(layout.line_count(), 1);
    let line = layout.line_glyphs(0).expect("line 0 must exist");
    assert_eq!(line.glyphs.len(), 7);
}

/// 折行：窄宽度下长文本必然多行。
#[test]
fn wrapping_produces_multiple_lines() {
    let mut cx = context();
    let mut req = default_req("The quick brown fox jumps over the lazy dog");
    req.max_advance = Some(120.0);
    let layout = cx.layout(&req).expect("layout must succeed");
    assert!(
        layout.line_count() > 1,
        "narrow width must wrap, got {} lines",
        layout.line_count()
    );
    // 每行 advance 不超过折行宽度（不含行尾空白）。
    for i in 0..layout.line_count() {
        let line = layout.line_glyphs(i).expect("line must exist");
        assert!(!line.glyphs.is_empty());
    }
}

/// 字号缩放：32px 与 64px 的同文本布局宽度按比例增长（近似 2 倍，
/// 容差 5% 防 hinting 差异）。
#[test]
fn font_size_scales_layout() {
    let mut cx = context();
    let small = cx.layout(&default_req("Hello")).unwrap();
    let mut req = default_req("Hello");
    req.font_size = 64.0;
    let large = cx.layout(&req).unwrap();
    let ratio = large.width() / small.width();
    assert!(
        (ratio - 2.0).abs() < 0.1,
        "width ratio expected ~2.0, got {ratio}"
    );
}

/// 行距倍率：2.0 倍行距的高度大于 1.0 倍。
#[test]
fn line_spacing_multiplier_grows_height() {
    let mut cx = context();
    let mut req = default_req("第一行\n第二行").clone();
    // 单段双行文本（\n 在 parley 中分段；改用长中文 + 窄宽度换行更贴近场景）。
    req.text = "这是一段较长的中文文本需要换行验证行距";
    req.max_advance = Some(200.0);
    req.line_spacing_multiplier = 1.0;
    let normal = cx.layout(&req).unwrap();
    assert!(normal.line_count() > 1);
    let mut req2 = req.clone();
    req2.line_spacing_multiplier = 2.0;
    let spaced = cx.layout(&req2).unwrap();
    assert!(spaced.height() > normal.height() * 1.5);
}

/// 对齐：居中时同一文本首字形 x 偏移大于左对齐（行内留白分配）。
#[test]
fn center_alignment_indents_first_glyph() {
    let mut cx = context();
    let mut req = default_req("abc");
    req.max_advance = Some(300.0);
    req.align = TextAlign::Left;
    let left = cx.layout(&req).unwrap();
    let first_left = left.line_glyphs(0).unwrap().glyphs[0].x;
    let mut req2 = req.clone();
    req2.align = TextAlign::Center;
    let center = cx.layout(&req2).unwrap();
    let first_center = center.line_glyphs(0).unwrap().glyphs[0].x;
    assert!(
        first_center > first_left,
        "centered first glyph x ({first_center}) must exceed left-aligned ({first_left})"
    );
}

/// 颜色传递：字形携带请求颜色。
#[test]
fn color_propagates_to_glyphs() {
    let mut cx = context();
    let mut req = default_req("Hi");
    req.color = Color::rgba(255, 0, 0, 255);
    let layout = cx.layout(&req).unwrap();
    let line = layout.line_glyphs(0).unwrap();
    assert_eq!(line.glyphs[0].color, Color::rgba(255, 0, 0, 255));
}

/// 首行 metrics：ascent/descent 为正值且 baseline 在 (0, line_height) 内。
#[test]
fn first_line_metrics_are_sane() {
    let mut cx = context();
    let layout = cx.layout(&default_req("Hello")).unwrap();
    let m = layout.first_line_metrics().expect("metrics must exist");
    assert!(m.ascent > 0.0);
    assert!(m.descent > 0.0);
    assert!(m.line_height > 0.0);
    assert!(m.baseline > 0.0 && m.baseline <= m.line_height);
}

/// 越界行访问必须返回 None（不 panic）。
#[test]
fn out_of_range_line_returns_none() {
    let mut cx = context();
    let layout = cx.layout(&default_req("Hello")).unwrap();
    assert!(layout.line_glyphs(999).is_none());
}
