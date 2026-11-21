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
use vello_cpu::{Pixmap, RenderContext, Resources};

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
    // 未配对裁剪层数（帧末统一弹出）。
    let mut clips_open: u32 = 0;

    // 清屏：整幅覆盖（透明清屏色 = 透明黑，与 Android 硬件缓冲语义一致）。
    ctx.set_paint(to_alpha_color(scene.clear_color));
    ctx.fill_rect(&KurboRect::from_points(
        (0.0, 0.0),
        (f64::from(w), f64::from(h)),
    ));

    for cmd in &scene.commands {
        // 逐命令日志仅在 Trace 级开启：默认 Info 下零开销、零 logcat 写入。
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
                draw_image_cmd(&mut ctx, &mut resources, image, *x, *y);
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
                    .glyph_run(&mut resources, &font)
                    .font_size(*font_size)
                    .hint(true)
                    .fill_glyphs(iter);
            }
        }
    }

    // 平衡 layer 栈（录制期未 restore 的裁剪在帧末统一弹出，
    // vello_cpu 要求 flush 前栈空）。
    while clips_open > 0 {
        ctx.pop_layer();
        clips_open -= 1;
    }

    ctx.flush();
    log::debug!("render: flushed");
    ctx.render(&mut pixmap, &mut resources);
    log::debug!("render: rendered to pixmap");
    Some(pixmap)
}

/// 图片绘制：Image → vello_common ImageSource（Pixmap 共享）。
///
/// # 边界
/// V1 不做采样缩放（1:1 搬运）；大图越界部分被裁剪层丢弃。
fn draw_image_cmd(
    ctx: &mut RenderContext,
    resources: &mut Resources,
    image: &Image,
    x: f64,
    y: f64,
) {
    let w = image.width().min(u16::MAX as u32) as u16;
    let h = image.height().min(u16::MAX as u32) as u16;
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
    // ImageBrush（vello_common::paint::Image = ImageBrush<ImageSource>）。
    let brush = vello_cpu::Image {
        image: vello_cpu::ImageSource::Pixmap(std::sync::Arc::new(pm)),
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

/// gfx Rect → kurbo Rect。
fn to_kurbo_rect(rect: &Rect) -> KurboRect {
    KurboRect::from_points((rect.x, rect.y), (rect.x + rect.w, rect.y + rect.h))
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
}
