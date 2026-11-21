//! 文本渲染：`TextLayout` → vello_cpu 管线（PRD §5.1.4）。
//!
//! # 设计
//! - 与 `gfx::render` 同构：把排版结果翻译为 vello_cpu 调用；
//! - 逐 run 绘制：`RenderContext::glyph_run` 按字体+字号+变换批量光栅化，
//!   glifo 内部持有图集缓存（命中计数由 vello_cpu Resources 管理）；
//! - 坐标系：字形坐标为基线系（y 向下为正），外层 translate 到绘制原点。

use std::sync::Arc;

use log::debug;
use vello_cpu::RenderContext;

use super::layout::{PositionedGlyph, TextLayout};

/// 把整个布局绘制到 vello_cpu 上下文。
///
/// # 参数
/// `x`/`y`：布局原点（左上角）在画布上的位置；
/// `scale`：布局字号 → 实际绘制字号的缩放（V1 恒 1.0，预留给 dp→px）。
pub fn draw_layout(
    ctx: &mut RenderContext,
    resources: &mut vello_cpu::Resources,
    layout: &TextLayout,
    x: f64,
    y: f64,
    scale: f32,
) {
    // 保存/恢复 由调用方（gfx::render 的 layer 机制）负责，这里只平移。
    ctx.set_transform(
        vello_cpu::kurbo::Affine::translate((x, y))
            * vello_cpu::kurbo::Affine::scale(f64::from(scale)),
    );
    for (i, line) in layout.all_line_glyphs().iter().enumerate() {
        let _ = i;
        draw_line_glyphs(
            ctx,
            resources,
            line.glyphs.as_slice(),
            layout.font_size() * scale,
        );
    }
    ctx.set_transform(vello_cpu::kurbo::Affine::IDENTITY);
}

/// 绘制一行字形：逐 glyph（同字体场景下批量进 glyph_run 更优，
/// 但 V1 run 结构不跨字体，按行聚合已足够）。
///
/// # 性能
/// glifo `GlyphRunBuilder` 要求 glyphs 迭代器按绘制序给出；
/// 同行字形共享同一字体实例（V1 单字体），一次 `fill_glyphs` 批量渲染。
///
/// # 失败语义
/// 字体未注入（None）时直接返回，跳过本行——渲染降级不 panic。
fn draw_line_glyphs(
    ctx: &mut RenderContext,
    resources: &mut vello_cpu::Resources,
    glyphs: &[PositionedGlyph],
    font_size: f32,
) {
    if glyphs.is_empty() || font_size <= 0.0 {
        return;
    }
    debug!(
        "draw_text: {} glyphs at size {:.1}px",
        glyphs.len(),
        font_size
    );
    // 首个字形的颜色作为 run 色（V1 行内恒单色；多色分 run 由 layout 层
    // 拆分——当前 TextBox 单样式，不触发）。
    let color = glyphs[0].color;
    ctx.set_paint(
        vello_cpu::color::AlphaColor::<vello_cpu::color::Srgb>::from_rgba8(
            color.r, color.g, color.b, color.a,
        ),
    );
    // 字体数据：V1 全局单字体（NotoSerifSC），字形 id 直接对应。
    // 未注入时降级跳过（真机黑屏教训：禁止渲染线程 panic）。
    let Some(font) = font() else {
        log::warn!(
            "draw_text: font not injected, skipping {} glyphs",
            glyphs.len()
        );
        return;
    };
    let iter = glyphs.iter().map(|g| glifo::Glyph {
        id: g.id,
        x: g.x,
        y: g.y,
    });
    let _ = ctx
        .glyph_run(resources, &font)
        .font_size(font_size)
        .hint(true)
        .fill_glyphs(iter);
}

/// 全局字体数据（进程单例）：glifo 需要 `peniko::FontData` 实例。
///
/// # 来源优先级
/// 1. Android 真机：`text::font` 进程缓存（onCreate 预加载，mmap 零拷贝）；
/// 2. 主机测试：`set_font_bytes` 注入的堆缓冲（黄金图测试替身）。
///
/// # 失败语义
/// 两处均未注入时返回 `None`（调用方降级：跳过字形绘制），禁止 panic——
/// 渲染线程 panic 会导致主循环静默死亡（真机已踩坑：黑屏无日志）。
///
/// # 线程安全
/// `FontData` 内部 `Blob` 引用计数共享；注入后只读，并发访问安全。
fn font() -> Option<vello_cpu::peniko::FontData> {
    let bytes: Arc<dyn AsRef<[u8]> + Send + Sync> = match crate::text::font::cached_font() {
        Some(font) => Arc::new(font.shared()),
        None => Arc::new(Arc::clone(FONT_BYTES.get()?)),
    };
    Some(vello_cpu::peniko::FontData::new(
        vello_cpu::peniko::Blob::new(bytes),
        0,
    ))
}

/// 场景渲染路径的字体访问（gfx::render 消费）。
pub fn scene_font() -> Option<vello_cpu::peniko::FontData> {
    font()
}

/// 进程级字体字节（`text::font::load` 完成后填充；主机测试经
/// `set_test_font` 注入）。
///
/// 存储 `Arc<[u8]>`：`font()` 每次调用都取一份句柄，若存裸 `Vec`
/// 每次 `clone()` 都会整份复制 ~14MB（列表页 200 行 × 每行一次
/// = ~2.8GB 瞬时分配）；Arc 克隆仅引用计数。
static FONT_BYTES: std::sync::OnceLock<Arc<[u8]>> = std::sync::OnceLock::new();
/// 注入字体字节（宿主装配或测试替身）。
///
/// # 约束
/// 只允许调用一次（首次生效）；字体切换不在 V1 范围。
pub fn set_font_bytes(bytes: Vec<u8>) {
    let _ = FONT_BYTES.set(Arc::from(bytes.into_boxed_slice()));
}

/// 把排版结果翻译为 `Scene` 命令（View 系绘制入口）。
///
/// 与 `draw_layout`（直接 vello_cpu 路径）互为补充：Widget 体系经
/// Scene 中间表示，便于黄金图测试与后端无关。逐行一命令（同字体同色）。
pub fn emit_layout(scene: &mut crate::gfx::scene::Scene, layout: &TextLayout, x: f64, y: f64) {
    for line in layout.all_line_glyphs() {
        if line.glyphs.is_empty() {
            continue;
        }
        let color = line.glyphs[0].color;
        let glyphs = line
            .glyphs
            .iter()
            .map(|g| crate::text::layout::SceneGlyph {
                id: g.id,
                x: x as f32 + g.x,
                y: y as f32 + g.y,
            })
            .collect();
        scene.commands.push(crate::gfx::scene::Command::DrawGlyphs {
            glyphs,
            font_size: layout.font_size(),
            color,
        });
    }
}
