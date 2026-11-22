//! 文本排版：parley 封装（PRD §5.1.4）。
//!
//! # 设计
//! - `TextContext` 持有 parley 的 `FontContext`/`LayoutContext`（内部含
//!   字形缓存与样式表，跨布局复用以命中缓存）；
//! - `TextLayout` 为排版结果值类型：行/字形游标只读暴露；
//! - 字体经 `fontique::Collection::register_fonts` 注入（V1 单字体：
//!   assets NotoSerifSC，Android 主机同一入口）；
//! - 对齐（左/中/右）由 parley alignment 在 `build` 后应用。

use std::sync::Arc;

use parley::fontique::Blob;
use parley::layout::{Alignment, AlignmentOptions, Layout, PositionedLayoutItem};
use parley::style::{FontFamily, FontFamilyName, LineHeight, StyleProperty, TextStyle};
use parley::{FontContext, LayoutContext};

use super::font::FontData;

/// `&'static [u8]` 的 `AsRef` 包装：让 mmap 字体切片可零拷贝注入 `Blob`。
struct StaticSlice(&'static [u8]);

impl AsRef<[u8]> for StaticSlice {
    fn as_ref(&self) -> &[u8] {
        self.0
    }
}

/// `Arc<[u8]>` 的 `AsRef` 包装：堆字体缓冲零拷贝转发（不 `to_vec` 复制）。
struct HeapSlice(Arc<[u8]>);

impl AsRef<[u8]> for HeapSlice {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// 排版上下文：跨布局复用的字体缓存与样式表（非 Send；单线程 owning）。
pub struct TextContext {
    /// parley 字体上下文（skrifa 解析 + 字形缓存宿主）。
    font_cx: FontContext,
    /// parley 排版上下文（样式表 + 断行游标缓存）。
    layout_cx: LayoutContext<ColorBrush>,
    /// 排版结果缓存：同一段文字 + 同样式重复请求时直接复用。
    ///
    /// # 为什么必须有
    /// parley 的 `build + break_all_lines + align` 单次 0.2–0.5ms（要断行、
    /// 取字形、算 cluster）。一个页面十几段文字、每帧都排一次，实测能吃掉
    /// 4–5ms —— 帧预算才 16.7ms。文案与样式都没变时这部分工作本可为零。
    ///
    /// # 失效策略
    /// 键包含文字、字号、限宽、对齐与颜色，任一变化自然落到新条目；
    /// 容量上限按「最久未用」淘汰，防止动态数值文案（字数/百分比）无限增长。
    cache: std::cell::RefCell<LayoutCache>,
}

/// 排版缓存的最大条目数（一屏文字约 20–40 段，留一倍余量给动画中间文案）。
const LAYOUT_CACHE_CAP: usize = 96;

/// 排版缓存的键：文字 + 字号 + 限宽 + 对齐 + 颜色。
#[derive(Clone, PartialEq, Eq, Hash)]
struct LayoutKey {
    text: String,
    size_bits: u32,
    max_adv_bits: u32,
    align: u8,
    rgba: [u8; 4],
}

/// 排版缓存本体（顺序表实现：容量极小，线性查找比哈希更省事且无额外分配）。
struct LayoutCache {
    entries: std::collections::VecDeque<(LayoutKey, std::rc::Rc<TextLayout>)>,
}

impl LayoutCache {
    fn new() -> Self {
        Self {
            entries: std::collections::VecDeque::new(),
        }
    }

    fn get(&mut self, key: &LayoutKey) -> Option<std::rc::Rc<TextLayout>> {
        let pos = self.entries.iter().position(|(k, _)| k == key)?;
        let hit = self.entries.remove(pos)?;
        let out = std::rc::Rc::clone(&hit.1);
        self.entries.push_back(hit);
        Some(out)
    }

    fn put(&mut self, key: LayoutKey, layout: std::rc::Rc<TextLayout>) {
        if self.entries.len() >= LAYOUT_CACHE_CAP {
            self.entries.pop_front();
        }
        self.entries.push_back((key, layout));
    }
}

/// parley Brush 实现：纯色（V1 无渐变文字）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ColorBrush {
    /// sRGBA 非预乘，u8 通道（与 gfx::Color 一致）。
    pub rgba: [u8; 4],
}

/// 文本对齐（Android `Paint.Align` 超集：V1 支持 L/C/R）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlign {
    /// 左对齐（默认，LTR 下与 START 等价）。
    #[default]
    Left,
    /// 居中。
    Center,
    /// 右对齐。
    Right,
}

/// 排版请求参数（`TextLayout::build` 入参）。
#[derive(Clone)]
pub struct TextStyleRequest<'a> {
    /// 文本内容（UTF-8，中英混排由 parley complex-scripts 处理）。
    pub text: &'a str,
    /// 字号（px；Android `paint.textSize` 语义）。
    pub font_size: f32,
    /// 折行宽度上限（`None` = 不折行；Android `breakStrategy` V1 仅简单折行）。
    pub max_advance: Option<f32>,
    /// 对齐（作用于所有行）。
    pub align: TextAlign,
    /// 文字颜色。
    pub color: crate::gfx::color::Color,
    /// 行距倍率（1.0 = 字体默认 leading；Android `lineSpacingMult` 语义近似，
    /// V1 经 font_size 缩放 approximated）。
    pub line_spacing_multiplier: f32,
}

impl<'a> Default for TextStyleRequest<'a> {
    fn default() -> Self {
        Self {
            text: "",
            font_size: 16.0,
            max_advance: None,
            align: TextAlign::default(),
            color: crate::gfx::color::Color::BLACK,
            line_spacing_multiplier: 1.0,
        }
    }
}

impl TextContext {
    /// 创建上下文并注册字体（进程生命周期单例语义由调用方保证）。
    ///
    /// # 注册语义
    /// fontique 按字体内嵌的 family 名注册（NotoSerifSC 的 family 为
    /// "Noto Serif SC"）；查询走 generic serif → 注册族匹配。
    pub fn new(font: &FontData) -> Self {
        let mut font_cx = FontContext::new();
        // 注册字体数据：Blob 引用共享（零拷贝，与 FontData 同一份内存）。
        // 堆缓冲直接 Arc 转发；mmap 静态切片包一层借用包装（同样零拷贝），
        // 避免 200 行列表页各自复制 14MB 字体导致 ~2.9GB RSS。
        let bytes: Arc<dyn AsRef<[u8]> + Send + Sync> = match font.shared() {
            super::font::FontBytes::Heap(arc) => Arc::new(HeapSlice(arc)),
            super::font::FontBytes::Mmap(slice) => Arc::new(StaticSlice(slice)),
        };
        font_cx.collection.register_fonts(Blob::new(bytes), None);
        Self {
            font_cx,
            layout_cx: LayoutContext::new(),
            cache: std::cell::RefCell::new(LayoutCache::new()),
        }
    }

    /// 排版一段文本。
    ///
    /// # 返回
    /// `None` 仅当文本为空（零行布局无意义）。
    pub fn layout(&mut self, req: &TextStyleRequest<'_>) -> Option<std::rc::Rc<TextLayout>> {
        if req.text.is_empty() {
            return None;
        }
        let key = LayoutKey {
            text: req.text.to_string(),
            size_bits: req.font_size.to_bits(),
            max_adv_bits: req.max_advance.unwrap_or(f32::NAN).to_bits(),
            align: match req.align {
                TextAlign::Left => 0,
                TextAlign::Center => 1,
                TextAlign::Right => 2,
            },
            rgba: [req.color.r, req.color.g, req.color.b, req.color.a],
        };
        if let Some(hit) = self.cache.borrow_mut().get(&key) {
            return Some(hit);
        }
        let brush = ColorBrush {
            rgba: [req.color.r, req.color.g, req.color.b, req.color.a],
        };
        // TextStyle：命名 family（NotoSerifSC 已注册命中）+ 字号 + 颜色 + 行距。
        let mut style: TextStyle<ColorBrush> = TextStyle::default();
        style.font_family = FontFamily::Single(FontFamilyName::Named("Noto Serif SC".into()));
        style.font_size = req.font_size;
        style.brush = brush;
        style.line_height = LineHeight::FontSizeRelative(req.line_spacing_multiplier);
        let mut builder = self
            .layout_cx
            .ranged_builder(&mut self.font_cx, req.text, 1.0, true);
        builder.push_default(StyleProperty::FontSize(style.font_size));
        builder.push_default(StyleProperty::FontStyle(style.font_style));
        builder.push_default(StyleProperty::FontFamily(style.font_family));
        builder.push_default(StyleProperty::Brush(brush));
        builder.push_default(StyleProperty::LineHeight(style.line_height));
        let mut layout: Layout<ColorBrush> = builder.build(req.text);
        // 折行：给定宽度时全量断行。
        layout.break_all_lines(req.max_advance);
        // 对齐：每行按对齐选项重定位（Default = 溢出不强制对齐）。
        layout.align(
            match req.align {
                TextAlign::Left => Alignment::Left,
                TextAlign::Center => Alignment::Center,
                TextAlign::Right => Alignment::Right,
            },
            AlignmentOptions::default(),
        );
        let out = std::rc::Rc::new(TextLayout {
            layout,
            font_size: req.font_size,
        });
        self.cache.borrow_mut().put(key, std::rc::Rc::clone(&out));
        Some(out)
    }
}

/// 排版结果：只读行/字形访问 + 测量。
pub struct TextLayout {
    /// parley 排版产物（含全部行/字形/样式数据）。
    layout: Layout<ColorBrush>,
    /// 排版字号（metrics 换算用）。
    font_size: f32,
}

/// 单个定位字形（渲染层消费的最小单元）。
#[derive(Clone, Copy, Debug)]
pub struct PositionedGlyph {
    /// 字形 id（skrifa glyph_id）。
    pub id: u32,
    /// 基线系 x 坐标（px）。
    pub x: f32,
    /// 基线系 y 坐标（px，含基线偏移）。
    pub y: f32,
    /// 颜色（run 级）。
    pub color: crate::gfx::color::Color,
}

/// 场景字形：渲染层消费的最小单元（id + 绘制位置 + 颜色）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneGlyph {
    /// 字形 id（skrifa glyph_id）。
    pub id: u32,
    /// 基线系 x 坐标（px）。
    pub x: f32,
    /// 基线系 y 坐标（px）。
    pub y: f32,
}

impl From<&PositionedGlyph> for SceneGlyph {
    fn from(g: &PositionedGlyph) -> Self {
        Self {
            id: g.id,
            x: g.x,
            y: g.y,
        }
    }
}

/// 单行字形集合（同一行内所有 run 展平）。
pub struct LineGlyphs {
    /// 行内全部已定位字形。
    pub glyphs: Vec<PositionedGlyph>,
    /// 该行的 baseline y（px，相对布局原点）。
    pub baseline: f32,
}

impl TextLayout {
    /// 布局总宽（px，不含行尾空白）。
    ///
    /// # 语义
    /// 折行后按「最长行的内容宽度」计算（parley `Layout::width` 在
    /// `break_all_lines` 后不保证重算，直接取会高估未折行的宽度）；
    /// 无字形（空白文本）时为 0。
    pub fn width(&self) -> f32 {
        let mut max = 0.0f32;
        for line in self.all_line_glyphs() {
            for g in &line.glyphs {
                max = max.max(g.x);
            }
        }
        max
    }

    /// 布局总高（px，所有行之和）。
    pub fn height(&self) -> f32 {
        self.layout.height()
    }

    /// 行数。
    pub fn line_count(&self) -> usize {
        self.layout.len()
    }

    /// 收集第 `line_index` 行的全部已定位字形（渲染层一次消费）。
    pub fn line_glyphs(&self, line_index: usize) -> Option<LineGlyphs> {
        let line = self.layout.get(line_index)?;
        let mut glyphs = Vec::new();
        for item in line.items() {
            if let PositionedLayoutItem::GlyphRun(run) = item {
                let [r, g, b, a] = run.style().brush.rgba;
                let color = crate::gfx::color::Color::rgba(r, g, b, a);
                for pg in run.positioned_glyphs() {
                    glyphs.push(PositionedGlyph {
                        id: pg.id,
                        x: pg.x,
                        y: pg.y,
                        color,
                    });
                }
            }
            // InlineBox V1 未使用（EditText 光标等由控件层自行绘制）。
        }
        let baseline = line.metrics().baseline + line.metrics().offset;
        Some(LineGlyphs { glyphs, baseline })
    }

    /// 全部行迭代（渲染层逐行绘制入口）。
    pub fn all_line_glyphs(&self) -> Vec<LineGlyphs> {
        (0..self.line_count())
            .filter_map(|i| self.line_glyphs(i))
            .collect()
    }

    /// 排版字号。
    pub fn font_size(&self) -> f32 {
        self.font_size
    }

    /// 首行 metrics（Android `Paint.getFontMetrics` 语义近似）。
    pub fn first_line_metrics(&self) -> Option<LineMetricsInfo> {
        let line = self.layout.get(0)?;
        let m = line.metrics();
        Some(LineMetricsInfo {
            ascent: m.ascent,
            descent: m.descent,
            leading: m.leading,
            line_height: m.line_height,
            baseline: m.baseline + m.offset,
        })
    }
}

/// Android `FontMetrics` 子集（TextView 高度协商用）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineMetricsInfo {
    /// 顶线到基线的距离（正值，向上）。
    pub ascent: f32,
    /// 基线到底线的距离（正值，向下）。
    pub descent: f32,
    /// 行间额外间距（字体 leading）。
    pub leading: f32,
    /// 行全高。
    pub line_height: f32,
    /// 基线相对行顶的 y 偏移。
    pub baseline: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 排版缓存必须「同文案同样式命中、不同文案/字号不命中」。
    ///
    /// # 为什么这条要有单测
    /// 缓存命中与否直接决定每帧 4–5ms 的开销；一旦键漏了某个样式维度
    /// （字号/限宽/颜色），会出现「改字号但版式不更新」的诡异 bug。
    #[test]
    fn layout_cache_hits_on_identical_request() {
        let font = crate::text::font::load_from_path(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../app/src/main/assets/fonts/NotoSerifSC-Regular.ttf"
        )))
        .expect("fixture font must load");
        let mut cx = TextContext::new(&font);
        let c = crate::gfx::color::Color {
            r: 0x20,
            g: 0x20,
            b: 0x28,
            a: 255,
        };
        let mut req = TextStyleRequest {
            text: "鸢尾花 · 01",
            font_size: 16.0,
            max_advance: Some(300.0),
            align: TextAlign::Left,
            color: c,
            line_spacing_multiplier: 1.25,
        };
        let a = cx.layout(&req).expect("layout");
        let b = cx.layout(&req).expect("layout");
        assert!(std::rc::Rc::ptr_eq(&a, &b), "同一请求必须命中同一条缓存");
        assert_eq!(a.width(), b.width());
        // 字号变化 → 必须是新条目（否则版式不会随字号更新）。
        req.font_size = 18.0;
        let d = cx.layout(&req).expect("layout");
        assert!(!std::rc::Rc::ptr_eq(&a, &d));
    }

    /// 缓存容量必须封顶（动态数值文案不能无限堆积内存）。
    #[test]
    fn layout_cache_is_bounded() {
        let font = crate::text::font::load_from_path(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../app/src/main/assets/fonts/NotoSerifSC-Regular.ttf"
        )))
        .expect("fixture font must load");
        let mut cx = TextContext::new(&font);
        for i in 0..(LAYOUT_CACHE_CAP + 10) {
            let text = format!("{i}%");
            let req = TextStyleRequest {
                text: &text,
                font_size: 12.0,
                max_advance: Some(100.0),
                align: TextAlign::Left,
                color: crate::gfx::color::Color::TRANSPARENT,
                line_spacing_multiplier: 1.2,
            };
            let _ = cx.layout(&req);
        }
        // 底层缓存同样封顶（避免只测到 TextContext 的薄封装）。
        assert!(cx.cache.borrow().entries.len() <= LAYOUT_CACHE_CAP);
    }

    /// 空文案直接返回 None（不占缓存、不排版）。
    #[test]
    fn empty_text_returns_none() {
        let font = crate::text::font::load_from_path(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../app/src/main/assets/fonts/NotoSerifSC-Regular.ttf"
        )))
        .expect("fixture font must load");
        let mut cx = TextContext::new(&font);
        let req = TextStyleRequest {
            text: "",
            font_size: 12.0,
            max_advance: None,
            align: TextAlign::Left,
            color: crate::gfx::color::Color::TRANSPARENT,
            line_spacing_multiplier: 1.2,
        };
        assert!(cx.layout(&req).is_none());
    }
}
