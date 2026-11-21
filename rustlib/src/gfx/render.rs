//! 软件渲染器：`gfx::Scene` → vello_cpu 的翻译层。
//!
//! # 设计
//! - 与后端解耦：本模块只做"命令 → vello_cpu 调用"的翻译，
//!   产物是 `Pixmap`（RGBA8888 紧凑缓冲）；
//! - 三后端共享：cpu 后端直接搬运 Pixmap；GPU 后端仅真机编译，
//!   主机 `cargo test` 走本模块（黄金图测试的渲染引擎）；
//! - 渐变映射：ColorStop（非预乘 RGBA8888）→ peniko ColorStop（Srgb f32），
//!   alpha 由 vello_cpu 内部处理预乘。

use vello_cpu::color::AlphaColor;
use vello_cpu::color::Srgb;
use vello_cpu::kurbo::{BezPath, Point, Rect as KurboRect, Stroke as KurboStroke};
use vello_cpu::peniko::{ColorStop as PenikoStop, Fill, Gradient};
use vello_cpu::{RasterizerSettings, RenderContext, Resources, TargetInit};

/// 对外重导出（backends 模块引用 Pixmap 类型）。
pub use vello_cpu::Pixmap;

use super::color::Color;
use super::image::Image;
use super::paint::{Paint, StrokeCap, StrokeJoin};
use super::path::{FillType, Path, PathElement};
use super::scene::{Command, Rect, Scene};
use super::shader::{Shader, TileMode};

/// 把一帧 `Scene` 渲染为 RGBA8888 紧凑缓冲。
///
/// # 不变量
/// 尺寸超 u16::MAX 视为非法（返回 `None`，不 panic）；
/// 返回缓冲长度恒为 `width * height * 4`。
pub fn render_scene(scene: &Scene) -> Option<Pixmap> {
    let (w, h) = (scene.width, scene.height);
    if w == 0 || h == 0 || w > u16::MAX as u32 || h > u16::MAX as u32 {
        return None;
    }
    let (w, h) = (w as u16, h as u16);
    let mut ctx = RenderContext::new(w, h);
    let mut resources = Resources::new();
    let mut pixmap = Pixmap::new(w, h);

    // 清屏：整幅覆盖（透明清屏色 = 透明黑，与 Android 硬件缓冲语义一致）。
    ctx.set_paint(to_alpha_color(scene.clear_color));
    ctx.fill_rect(&KurboRect::from_points(
        (0.0, 0.0),
        (f64::from(w), f64::from(h)),
    ));

    replay(scene, &mut ctx, &mut resources, None);

    ctx.render(&mut pixmap, &mut resources);
    log::debug!("render: rendered to pixmap");
    Some(pixmap)
}

/// 图片绘制：Image → vello_common ImageSource（Pixmap 共享）。
///
/// # 边界
/// V1 不做采样缩放（1:1 搬运）；大图越界部分被裁剪层丢弃。
///
/// # 缓存
/// 预乘 Pixmap 按 Image 像素指针缓存（thread-local），避免每帧重复
/// RGBA→PremulRgba8 转换（大图 1080×1920 = 8MB 逐像素运算）。
fn draw_image_cmd(
    ctx: &mut RenderContext,
    resources: &mut Resources,
    image: &Image,
    x: f64,
    y: f64,
) {
    let w = image.width().min(u16::MAX as u32) as u16;
    let h = image.height().min(u16::MAX as u32) as u16;

    // 缓存键：像素 Arc 指针（同一 Image 实例跨帧共享同一 Arc）。
    let key = image.pixels().as_ptr() as usize;
    thread_local! {
        static PM_CACHE: std::cell::RefCell<std::collections::HashMap<usize, std::sync::Arc<Pixmap>>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
    }
    let pm: std::sync::Arc<Pixmap> = PM_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(cached) = cache.get(&key) {
            std::sync::Arc::clone(cached)
        } else {
            // 紧凑 RGBA → 预乘 PremulRgba8 Pixmap（vello_cpu 内部位图格式）。
            let mut pm = Pixmap::new(w, h);
            let pixels = image.pixels();
            let dst = pm.data_mut();
            for i in 0..(w as usize * h as usize) {
                let (r, g, b, a) = (
                    pixels[i * 4],
                    pixels[i * 4 + 1],
                    pixels[i * 4 + 2],
                    pixels[i * 4 + 3],
                );
                let premul = vello_cpu::color::PremulRgba8 {
                    r: ((r as u16 * a as u16) / 255) as u8,
                    g: ((g as u16 * a as u16) / 255) as u8,
                    b: ((b as u16 * a as u16) / 255) as u8,
                    a,
                };
                dst[i] = premul;
            }
            let arc = std::sync::Arc::new(pm);
            cache.insert(key, std::sync::Arc::clone(&arc));
            arc
        }
    });

    // ImageBrush（vello_common::paint::Image = ImageBrush<ImageSource>）。
    let brush = vello_cpu::Image {
        image: vello_cpu::ImageSource::Pixmap(pm),
        sampler: vello_cpu::peniko::ImageSampler::default(),
    };
    let _ = resources;
    ctx.set_paint(brush);
    // 图片作为"画笔"填充目标矩形：1:1 无缩放。
    let rect = KurboRect::from_points((x, y), (x + f64::from(w), y + f64::from(h)));
    ctx.fill_rect(&rect);
}

/// gfx Color → vello_cpu AlphaColor（Srgb，非预乘）。
fn to_alpha_color(c: Color) -> AlphaColor<Srgb> {
    AlphaColor::from_rgba8(c.r, c.g, c.b, c.a)
}
/// gfx Paint → PaintType（vello_common Brush：纯色或渐变）。
fn to_brush(paint: &Paint) -> vello_cpu::PaintType {
    match &paint.shader {
        None => vello_cpu::PaintType::Solid(to_alpha_color(paint.color)),
        Some(shader) => vello_cpu::PaintType::Gradient(to_gradient(shader)),
    }
}

/// gfx Shader → peniko Gradient。
fn to_gradient(shader: &Shader) -> Gradient {
    match shader {
        Shader::LinearGradient {
            x0,
            y0,
            x1,
            y1,
            stops,
            tile,
        } => {
            let mut g = Gradient::new_linear(Point::new(*x0, *y0), Point::new(*x1, *y1));
            g = apply_stops(g, stops);
            g.with_extend(to_extend(*tile))
        }
        Shader::RadialGradient {
            cx,
            cy,
            r,
            stops,
            tile,
        } => {
            let mut g = Gradient::new_radial(Point::new(*cx, *cy), *r as f32);
            g = apply_stops(g, stops);
            g.with_extend(to_extend(*tile))
        }
        Shader::SweepGradient { cx, cy, stops } => {
            let mut g = Gradient::new_sweep(Point::new(*cx, *cy), 0.0, std::f32::consts::TAU);
            g = apply_stops(g, stops);
            g
        }
    }
}

/// 停靠点表转换（非预乘 u8 → Srgb f32）。
fn apply_stops(mut g: Gradient, stops: &[super::shader::ColorStop]) -> Gradient {
    let stops: Vec<PenikoStop> = stops
        .iter()
        .map(|s| PenikoStop {
            offset: s.position,
            color: to_alpha_color(s.color).into(),
        })
        .collect();
    g = g.with_stops(stops.as_slice());
    g
}

/// TileMode → peniko Extend。
fn to_extend(tile: TileMode) -> vello_cpu::peniko::Extend {
    match tile {
        TileMode::Clamp => vello_cpu::peniko::Extend::Pad,
        TileMode::Repeat => vello_cpu::peniko::Extend::Repeat,
        TileMode::Mirror => vello_cpu::peniko::Extend::Reflect,
    }
}

/// 只重绘脏区（PRD §12.R9 damage 重绘）。
///
/// # 算法
/// 光栅化仍以整幅场景坐标进行（与 `render_scene` 完全一致的命令重放），
/// 但输出到与视口同宽的临时 Pixmap，并经 `RasterizerSettings.offset`
/// 负偏移只保留脏区行；随后把脏区行段拷回持久 Pixmap，脏区外像素
/// 保持上一帧内容。这避免了 `ctx.render` 默认 `TargetInit::Clear`
/// 抹掉整个目标的陷阱。
///
/// # 返回
/// `Some(true)` = 已重绘脏区；`Some(false)` = 脏区与视口无交集，跳过；
/// `None` = 尺寸非法。
pub fn render_damage(scene: &Scene, pixmap: &mut Pixmap, damage: &Rect) -> Option<bool> {
    let (w, h) = (scene.width, scene.height);
    if w == 0 || h == 0 || w > u16::MAX as u32 || h > u16::MAX as u32 {
        return None;
    }
    // 脏区与视口求交：空交集 = 无变化。
    let x0 = damage.x.max(0.0).floor() as u32;
    let y0 = damage.y.max(0.0).floor() as u32;
    let x1 = (damage.x + damage.w).min(f64::from(w)).ceil() as u32;
    let y1 = (damage.y + damage.h).min(f64::from(h)).ceil() as u32;
    if x0 >= x1 || y0 >= y1 {
        return Some(false);
    }
    let (w, h) = (w as u16, h as u16);
    let (dw, dh) = ((x1 - x0) as u16, (y1 - y0) as u16);

    // 上下文尺寸 = 全帧（场景坐标系）：光栅化器在场景坐标下工作，
    // 经 RasterizerSettings.offset 平移后仅脏区部分落入目标缓冲。
    // 缩小上下文会改变光栅化器坐标空间，破坏与全帧渲染的逐像素一致性。
    let mut ctx = RenderContext::new(w, h);
    let mut resources = Resources::new();

    // 清屏：与 render_scene 相同的清屏命令（在场景坐标系中整幅覆盖），
    // 保证脏区像素与全帧渲染逐像素一致（含半透明清屏色语义）。
    ctx.set_paint(to_alpha_color(scene.clear_color));
    ctx.fill_rect(&KurboRect::from_points(
        (0.0, 0.0),
        (f64::from(w), f64::from(h)),
    ));

    replay(scene, &mut ctx, &mut resources, Some(damage));

    // 临时缓冲与持久 Pixmap 同宽（避免逐行重映射），高度只需覆盖脏区；
    // SrcOver + 手动清零（Pixmap::new 已是透明黑）= 与 render_scene 的
    // Clear(transparent) 背景语义一致。
    let mut tmp = Pixmap::new(w, dh);
    let settings = RasterizerSettings {
        offset: (-(x0 as i32), -(y0 as i32)),
        target_init: TargetInit::SrcOver,
        ..Default::default()
    };
    ctx.render_with(&mut tmp, &mut resources, settings);

    // 拷回脏区行段：tmp 列 0 已对应场景 x0（负偏移已完成平移），
    // 直接从列 0 起拷 dw 列。
    let src = tmp.data_as_u8_slice();
    let dst = pixmap.data_as_u8_slice_mut();
    let pitch = (w as usize) * 4;
    let seg_bytes = (dw as usize) * 4;
    let seg_off = (x0 as usize) * 4;
    for row in 0..dh as usize {
        let sy = row * pitch;
        let dy = ((y0 as usize) + row) * pitch + seg_off;
        dst[dy..dy + seg_bytes].copy_from_slice(&src[sy..sy + seg_bytes]);
    }
    Some(true)
}

/// 命令与矩形的相交测试（damage 裁剪用）。
///
/// # 保守策略
/// 仅对可精确界定 bounds 的命令（FillRect / DrawImage / DrawGlyphs）做裁剪；
/// 路径/裁剪栈命令保守保留（clip 栈状态与路径几何无法廉价界定影响范围）。
fn cmd_intersects(cmd: &Command, rect: &Rect) -> bool {
    match cmd {
        Command::FillRect { rect: r, .. } => rects_intersect(r, rect),
        Command::DrawImage { image, x, y } => {
            let cmd_rect = Rect {
                x: *x,
                y: *y,
                w: image.width() as f64,
                h: image.height() as f64,
            };
            rects_intersect(&cmd_rect, rect)
        }
        Command::DrawGlyphs {
            glyphs, font_size, ..
        } => {
            if glyphs.is_empty() {
                return true;
            }
            let fs = *font_size as f64;
            let margin = fs * 0.5;
            let mut x0 = f64::INFINITY;
            let mut y0 = f64::INFINITY;
            let mut x1 = f64::NEG_INFINITY;
            let mut y1 = f64::NEG_INFINITY;
            for g in glyphs {
                let gx = g.x as f64;
                let gy = g.y as f64;
                x0 = x0.min(gx);
                y0 = y0.min(gy);
                x1 = x1.max(gx + fs);
                y1 = y1.max(gy + fs);
            }
            let cmd_rect = Rect {
                x: x0 - margin,
                y: y0 - margin,
                w: (x1 - x0) + 2.0 * margin,
                h: (y1 - y0) + 2.0 * margin,
            };
            rects_intersect(&cmd_rect, rect)
        }
        _ => true,
    }
}

/// 两矩形相交测试（AABB）。
fn rects_intersect(a: &Rect, b: &Rect) -> bool {
    a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y
}

/// 命令重放（render_scene 与 render_damage 共享命令翻译路径）。
///
/// # 不变量
/// 只调用 ctx 的绘制/裁剪方法，不触碰 render/render_with——
/// 两个入口的差异仅在于输出目标设置。
///
/// # 参数
/// `damage`：脏区（场景坐标）；`Some` 时跳过与脏区不相交的命令（裁剪优化），
/// `None` 时全量重放（全帧渲染）。
fn replay(
    scene: &Scene,
    ctx: &mut RenderContext,
    resources: &mut Resources,
    damage: Option<&Rect>,
) {
    let mut clips_open: u32 = 0;
    for cmd in &scene.commands {
        // Damage 裁剪：跳过与脏区不相交的命令（仅对可精确界定 bounds 的命令生效）。
        if let Some(dmg) = damage {
            if !cmd_intersects(cmd, dmg) {
                continue;
            }
        }
        log::trace!("render: cmd start: {}", cmd.name());
        match cmd {
            Command::FillRect { rect, paint } => {
                ctx.set_paint(to_brush(paint));
                ctx.fill_rect(&to_kurbo_rect(rect));
            }
            Command::FillPath { path, paint } => {
                ctx.set_paint(to_brush(paint));
                ctx.set_fill_rule(to_fill(path.fill_type()));
                ctx.fill_path(&to_bez_path(path));
            }
            Command::StrokePath { path, paint } => {
                ctx.set_paint(to_brush(paint));
                ctx.set_stroke(to_stroke(paint));
                ctx.stroke_path(&to_bez_path(path));
            }
            Command::ClipRect { rect } => {
                ctx.push_clip_layer(&to_bez_path(&Path::rect(rect.x, rect.y, rect.w, rect.h)));
                clips_open += 1;
            }
            Command::ClipPath { path } => {
                ctx.push_clip_layer(&to_bez_path(path));
                clips_open += 1;
            }
            Command::PopClips { count } => {
                for _ in 0..*count {
                    ctx.pop_layer();
                }
                clips_open -= (*count).min(clips_open);
            }
            Command::DrawImage { image, x, y } => {
                draw_image_cmd(ctx, resources, image, *x, *y);
            }
            Command::DrawGlyphs {
                glyphs,
                font_size,
                color,
            } => {
                ctx.set_paint(to_alpha_color(*color));
                // 字体未注入时降级跳过（渲染线程禁止 panic，真机黑屏教训）。
                let Some(font) = crate::text::render::scene_font() else {
                    log::debug!(
                        "render: font not injected, skipping {} glyphs",
                        glyphs.len()
                    );
                    continue;
                };
                let iter = glyphs.iter().map(|g| glifo::Glyph {
                    id: g.id,
                    x: g.x,
                    y: g.y,
                });
                let _ = ctx
                    .glyph_run(resources, &font)
                    .font_size(*font_size)
                    .hint(true)
                    .fill_glyphs(iter);
            }
        }
    }
    // 平衡 layer 栈（录制期未 restore 的裁剪在帧末统一弹出）。
    while clips_open > 0 {
        ctx.pop_layer();
        clips_open -= 1;
    }
    ctx.flush();
}

/// gfx Rect → kurbo Rect。
fn to_kurbo_rect(rect: &Rect) -> KurboRect {
    KurboRect::from_points((rect.x, rect.y), (rect.x + rect.w, rect.y + rect.h))
}

/// 计算两帧之间的保守脏区（PRD §12.R9 damage 重绘）。
///
/// # 算法
/// 1. 清屏色变化或尺寸变化 → 整帧；
/// 2. 命令序列完全一致 → 零脏区（静止帧，调用方可跳过重绘）；
/// 3. 命令数量一致时逐条对比：
///    - [`Command::FillRect`] / [`Command::ClipRect`]：新旧 bounds 并集；
///    - [`Command::DrawImage`]：新旧位置并集；
///    - 其余命令（路径/字形/裁剪栈操作）：保守整帧（clip 栈状态与
///      非轴对齐几何无法精确界定影响范围）；
/// 4. 命令数量不一致 → 整帧（树结构变化，影响面不可局部化）。
///
/// # 返回
/// `None` = 无变化（跳过本帧上屏）；`Some(rect)` = 需要重绘的区域
/// （可能超界，调用方负责与视口求交）。
pub fn diff_damage(prev: &Scene, next: &Scene) -> Option<Rect> {
    if prev.width != next.width
        || prev.height != next.height
        || prev.clear_color != next.clear_color
    {
        return Some(Rect {
            x: 0.0,
            y: 0.0,
            w: f64::from(next.width),
            h: f64::from(next.height),
        });
    }
    if prev.commands == next.commands {
        return None;
    }
    if prev.commands.len() != next.commands.len() {
        return Some(full_rect(next));
    }
    let mut damage: Option<Rect> = None;
    for (p, n) in prev.commands.iter().zip(next.commands.iter()) {
        let changed_bounds: Option<Rect> = match (p, n) {
            (Command::FillRect { rect: a, .. }, Command::FillRect { rect: b, .. }) => {
                Some(union(a, b))
            }
            (Command::ClipRect { rect: a }, Command::ClipRect { rect: b }) => Some(union(a, b)),
            (
                Command::DrawImage { image, x, y },
                Command::DrawImage {
                    image: image2,
                    x: x2,
                    y: y2,
                },
            ) => {
                // 像素内容变化（同尺寸不同图）→ 整帧（保守，像素比较
                // 结果无法用 bounds 局部化覆盖区域）。
                if image != image2 {
                    return Some(full_rect(next));
                }
                let a = Rect {
                    x: *x,
                    y: *y,
                    w: image.width() as f64,
                    h: image.height() as f64,
                };
                let b = Rect {
                    x: *x2,
                    y: *y2,
                    w: image2.width() as f64,
                    h: image2.height() as f64,
                };
                Some(union(&a, &b))
            }
            // 同类型其余命令：paint/几何变化无法精确 bounds（路径控制点、
            // 字形边界），保守整帧。
            (a, b) if a.name() == b.name() => return Some(full_rect(next)),
            // 类型不同的同位命令：影响面不可比，整帧。
            _ => return Some(full_rect(next)),
        };
        damage = Some(match damage {
            None => changed_bounds.unwrap(),
            Some(d) => union(&d, &changed_bounds.unwrap()),
        });
    }
    damage
}

/// 整帧矩形。
fn full_rect(scene: &Scene) -> Rect {
    Rect {
        x: 0.0,
        y: 0.0,
        w: f64::from(scene.width),
        h: f64::from(scene.height),
    }
}

/// 两矩形并集（包围盒）。
fn union(a: &Rect, b: &Rect) -> Rect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = (a.x + a.w).max(b.x + b.w);
    let y1 = (a.y + a.h).max(b.y + b.h);
    Rect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    }
}

/// gfx Path → kurbo BezPath。
fn to_bez_path(path: &Path) -> BezPath {
    let mut bez = BezPath::new();
    for el in path.elements() {
        match el {
            PathElement::MoveTo { x, y } => bez.move_to(Point::new(*x, *y)),
            PathElement::LineTo { x, y } => bez.line_to(Point::new(*x, *y)),
            PathElement::QuadTo { cx, cy, x, y } => {
                bez.quad_to(Point::new(*cx, *cy), Point::new(*x, *y))
            }
            PathElement::CubicTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
            } => bez.curve_to(
                Point::new(*c1x, *c1y),
                Point::new(*c2x, *c2y),
                Point::new(*x, *y),
            ),
            PathElement::Close => bez.close_path(),
        }
    }
    bez
}

/// FillType → vello_cpu Fill（winding/even-odd）。
fn to_fill(fill: FillType) -> Fill {
    match fill {
        FillType::Winding => Fill::NonZero,
        FillType::EvenOdd => Fill::EvenOdd,
    }
}

/// Paint 描边参数 → kurbo Stroke。
fn to_stroke(paint: &Paint) -> KurboStroke {
    let mut stroke = KurboStroke::new(f64::from(paint.stroke_width));
    stroke = match paint.stroke_cap {
        StrokeCap::Butt => stroke.with_caps(vello_cpu::kurbo::Cap::Butt),
        StrokeCap::Round => stroke.with_caps(vello_cpu::kurbo::Cap::Round),
        StrokeCap::Square => stroke.with_caps(vello_cpu::kurbo::Cap::Square),
    };
    stroke = match paint.stroke_join {
        StrokeJoin::Miter => stroke.with_join(vello_cpu::kurbo::Join::Miter),
        StrokeJoin::Bevel => stroke.with_join(vello_cpu::kurbo::Join::Bevel),
        StrokeJoin::Round => stroke.with_join(vello_cpu::kurbo::Join::Round),
    };
    stroke
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::canvas::Canvas;
    use crate::gfx::paint::PaintStyle;

    /// 基本渲染：清屏 + 红色矩形落位（中心像素红色，外圈清屏色）。
    #[test]
    fn render_basic_rect() {
        let mut c = Canvas::new(10, 10);
        c.set_clear_color(Color::WHITE);
        c.draw_rect(
            Rect {
                x: 2.0,
                y: 2.0,
                w: 5.0,
                h: 5.0,
            },
            &Paint::default().color(Color::RED),
        );
        let scene = c.finish();
        let pixmap = render_scene(&scene).unwrap();
        assert_eq!(pixmap.width(), 10);
        assert_eq!(pixmap.height(), 10);
        let data = pixmap.data();
        // 矩形中心 (4,4) 为红色（预乘后同值）。
        let center = data[(4 * 10 + 4) as usize];
        assert_eq!(
            center,
            vello_cpu::color::PremulRgba8::from_u8_array([255, 0, 0, 255])
        );
        // 外圈 (0,0) 为白色清屏。
        let corner = data[0];
        assert_eq!(
            corner,
            vello_cpu::color::PremulRgba8::from_u8_array([255, 255, 255, 255])
        );
    }

    /// 非法尺寸返回 None（不 panic）。
    #[test]
    fn render_rejects_invalid_size() {
        let mut s = Scene::new(u16::MAX as u32 + 1, 10);
        s.commands.push(Command::FillRect {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
            },
            paint: Paint::default(),
        });
        assert!(render_scene(&s).is_none());
    }

    /// 裁剪生效：clip 外的绘制被丢弃。
    #[test]
    fn render_clip_bounds_drawing() {
        let mut c = Canvas::new(10, 10);
        c.set_clear_color(Color::WHITE);
        c.save();
        c.clip_rect(Rect {
            x: 0.0,
            y: 0.0,
            w: 5.0,
            h: 5.0,
        });
        c.draw_rect(
            Rect {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
            },
            &Paint::default().color(Color::RED),
        );
        c.restore();
        let pixmap = render_scene(&c.finish()).unwrap();
        let data = pixmap.data();
        // (2,2) 在 clip 内：红。
        assert_eq!(
            data[2 * 10 + 2],
            vello_cpu::color::PremulRgba8::from_u8_array([255, 0, 0, 255])
        );
        // (7,7) 在 clip 外：白（清屏色）。
        assert_eq!(
            data[7 * 10 + 7],
            vello_cpu::color::PremulRgba8::from_u8_array([255, 255, 255, 255])
        );
    }

    /// 线性渐变渲染：起点色/终点色/中点近似。
    #[test]
    fn render_linear_gradient() {
        let shader = Shader::linear(
            0.0,
            0.0,
            10.0,
            0.0,
            &[Color::RED, Color::BLUE],
            None,
            TileMode::Clamp,
        );
        let mut c = Canvas::new(10, 1);
        c.set_clear_color(Color::WHITE);
        c.draw_rect(
            Rect {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 1.0,
            },
            &Paint::default().shader(Some(shader)),
        );
        let pixmap = render_scene(&c.finish()).unwrap();
        let data = pixmap.data();
        // 渐变以像素中心采样：x=0.5/10 处接近红（G/B 有少量蓝渗入），
        // x=9.5/10 处接近蓝（R/G 有少量红渗入）。
        let p0 = data[0];
        assert!(p0.r >= 240, "start should be red-ish: {p0:?}");
        assert_eq!(p0.g, 0);
        let p9 = data[9];
        assert!(p9.b >= 240, "end should be blue-ish: {p9:?}");
        assert!(p9.r <= 30, "end should have little red: {p9:?}");
    }

    /// 路径描边：线条中心像素命中。
    #[test]
    fn render_stroked_path() {
        let mut path = Path::new();
        path.move_to(1.0, 5.0);
        path.line_to(8.0, 5.0);
        let mut c = Canvas::new(10, 10);
        c.set_clear_color(Color::WHITE);
        c.draw_path(
            &path,
            &Paint::default()
                .color(Color::BLACK)
                .style(PaintStyle::Stroke)
                .stroke_width(2.0),
        );
        let pixmap = render_scene(&c.finish()).unwrap();
        let data = pixmap.data();
        // 线中心 (4,5) 黑。
        assert_eq!(
            data[5 * 10 + 4],
            vello_cpu::color::PremulRgba8::from_u8_array([0, 0, 0, 255])
        );
        // 远离线 (4,0) 白。
        assert_eq!(
            data[4],
            vello_cpu::color::PremulRgba8::from_u8_array([255, 255, 255, 255])
        );
    }

    /// 图片绘制：1x1 红图落在目标点。
    #[test]
    fn render_image() {
        // 生成 1x1 红 PNG 再解码（复用 image crate 的标准路径）。
        let img = image::RgbaImage::from_raw(1, 1, vec![0xFF, 0, 0, 0xFF]).unwrap();
        let mut buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        let image = Image::decode(buf.get_ref()).unwrap();

        let mut c = Canvas::new(4, 4);
        c.set_clear_color(Color::WHITE);
        c.draw_image(&image, 2.0, 2.0);
        let pixmap = render_scene(&c.finish()).unwrap();
        let data = pixmap.data();
        assert_eq!(
            data[2 * 4 + 2],
            vello_cpu::color::PremulRgba8::from_u8_array([255, 0, 0, 255])
        );
        assert_eq!(
            data[0],
            vello_cpu::color::PremulRgba8::from_u8_array([255, 255, 255, 255])
        );
    }

    /// diff_damage：静止帧 → None（可跳过重绘）。
    #[test]
    fn damage_no_change() {
        let c = |x: f64| {
            let mut c = Canvas::new(10, 10);
            c.set_clear_color(Color::WHITE);
            c.draw_rect(
                Rect {
                    x,
                    y: 0.0,
                    w: 2.0,
                    h: 2.0,
                },
                &Paint::default().color(Color::RED),
            );
            c.finish()
        };
        assert!(diff_damage(&c(1.0), &c(1.0)).is_none());
    }

    /// diff_damage：单 FillRect 位移 → 新旧 bounds 并集。
    #[test]
    fn damage_rect_move_union() {
        let make = |x: f64| {
            let mut c = Canvas::new(10, 10);
            c.set_clear_color(Color::WHITE);
            c.draw_rect(
                Rect {
                    x,
                    y: 0.0,
                    w: 2.0,
                    h: 2.0,
                },
                &Paint::default().color(Color::RED),
            );
            c.finish()
        };
        let d = diff_damage(&make(1.0), &make(4.0)).unwrap();
        // 并集 = [1,0] ~ [6,2]。
        assert_eq!((d.x, d.y, d.w, d.h), (1.0, 0.0, 5.0, 2.0));
    }

    /// diff_damage：清屏色变化 → 整帧。
    #[test]
    fn damage_clear_color_change() {
        let mut c1 = Canvas::new(10, 10);
        c1.set_clear_color(Color::WHITE);
        let mut c2 = Canvas::new(10, 10);
        c2.set_clear_color(Color::BLACK);
        let d = diff_damage(&c1.finish(), &c2.finish()).unwrap();
        assert_eq!((d.x, d.y, d.w, d.h), (0.0, 0.0, 10.0, 10.0));
    }

    /// diff_damage：命令数量不一致 → 整帧。
    #[test]
    fn damage_command_count_change() {
        let one = {
            let mut c = Canvas::new(10, 10);
            c.draw_rect(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 2.0,
                    h: 2.0,
                },
                &Paint::default().color(Color::RED),
            );
            c.finish()
        };
        let two = {
            let mut c = Canvas::new(10, 10);
            c.draw_rect(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 2.0,
                    h: 2.0,
                },
                &Paint::default().color(Color::RED),
            );
            c.draw_rect(
                Rect {
                    x: 5.0,
                    y: 5.0,
                    w: 2.0,
                    h: 2.0,
                },
                &Paint::default().color(Color::BLUE),
            );
            c.finish()
        };
        let d = diff_damage(&one, &two).unwrap();
        assert_eq!((d.x, d.y, d.w, d.h), (0.0, 0.0, 10.0, 10.0));
    }

    /// render_damage：脏区重绘与全帧渲染逐像素一致（核心正确性）。
    ///
    /// 场景：白底 + 两个红矩形。先全帧渲染，再只对脏区重绘（模拟
    /// 第一个矩形移动），验证脏区内外像素与全帧结果一致。
    #[test]
    fn damage_render_matches_full_render() {
        let mut c1 = Canvas::new(10, 10);
        c1.set_clear_color(Color::WHITE);
        c1.draw_rect(
            Rect {
                x: 1.0,
                y: 1.0,
                w: 2.0,
                h: 2.0,
            },
            &Paint::default().color(Color::RED),
        );
        c1.draw_rect(
            Rect {
                x: 6.0,
                y: 6.0,
                w: 2.0,
                h: 2.0,
            },
            &Paint::default().color(Color::BLUE),
        );
        let s1 = c1.finish();

        let mut c2 = Canvas::new(10, 10);
        c2.set_clear_color(Color::WHITE);
        c2.draw_rect(
            Rect {
                x: 2.0,
                y: 1.0,
                w: 2.0,
                h: 2.0,
            },
            &Paint::default().color(Color::RED),
        );
        c2.draw_rect(
            Rect {
                x: 6.0,
                y: 6.0,
                w: 2.0,
                h: 2.0,
            },
            &Paint::default().color(Color::BLUE),
        );
        let s2 = c2.finish();

        // 全帧基准。
        let full = render_scene(&s2).unwrap();

        // damage 路径：以全帧 s1 为底，按 diff 脏区局部重绘。
        let mut base = render_scene(&s1).unwrap();
        let d = diff_damage(&s1, &s2).unwrap();
        assert!(render_damage(&s2, &mut base, &d).unwrap());

        // 逐像素一致（局部重绘 = 全帧渲染，覆盖 clear 语义）。
        for i in 0..100 {
            assert_eq!(
                base.data()[i],
                full.data()[i],
                "pixel {i} mismatch: damage render differs from full render"
            );
        }
    }

    /// render_damage：脏区与视口无交集 → false（跳过）。
    #[test]
    fn damage_out_of_viewport() {
        let mut c = Canvas::new(10, 10);
        c.set_clear_color(Color::WHITE);
        let s = c.finish();
        let mut pixmap = Pixmap::new(10, 10);
        assert!(
            !render_damage(
                &s,
                &mut pixmap,
                &Rect {
                    x: 20.0,
                    y: 20.0,
                    w: 5.0,
                    h: 5.0
                }
            )
            .unwrap()
        );
    }
}
