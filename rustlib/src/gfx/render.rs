//! 软件渲染器：`gfx::Scene` → vello_cpu 的翻译层。
//!
//! # 设计
//! - 与后端解耦：本模块只做"命令 → vello_cpu 调用"的翻译，
//!   产物是 `Pixmap`（RGBA8888 紧凑缓冲）；
//! - 三后端共享：cpu 后端直接搬运 Pixmap；GPU 后端仅真机编译，
//!   主机 `cargo test` 走本模块（黄金图测试的渲染引擎）；
//! - 渐变映射：ColorStop（非预乘 RGBA8888）→ peniko ColorStop（Srgb f32），
//!   alpha 由 vello_cpu 内部处理预乘。

use std::collections::VecDeque;

use vello_cpu::color::AlphaColor;
use vello_cpu::color::Srgb;
use vello_cpu::kurbo::{Affine, BezPath, Point, Rect as KurboRect, Stroke as KurboStroke};
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
    let (w, h) = scene_dims(scene)?;
    let mut raster = SoftwareRaster::new(w, h);
    let mut pixmap = Pixmap::new(w, h);
    raster.render(scene, &mut pixmap).then_some(pixmap)
}

/// 场景尺寸合法化（0 或超 u16 上限 → None）。
fn scene_dims(scene: &Scene) -> Option<(u16, u16)> {
    let (w, h) = (scene.width, scene.height);
    if w == 0 || h == 0 || w > u16::MAX as u32 || h > u16::MAX as u32 {
        return None;
    }
    Some((w as u16, h as u16))
}

/// 跨帧复用的软件光栅器。
///
/// # 为什么要复用
/// `RenderContext` 与 `Resources` 里各自持有可复用状态：前者是 tile 调度器与
/// 临时几何缓冲，后者是**字形 atlas 与字形 prep 缓存**。逐帧重建会让每一帧都
/// 重新光栅化全部字形轮廓（文字密集的页面单这一项就能吃掉 10ms+）。
///
/// # 不变量
/// - 尺寸只经 [`Self::resize`] 变更；变更后 `ctx` 与目标 `Pixmap` 必须同尺寸；
/// - [`Self::render`] / [`Self::render_rect`] 入口自带 `ctx.reset()`，
///   调用方无需关心上一帧残留的裁剪栈与变换。
pub struct SoftwareRaster {
    ctx: RenderContext,
    resources: Resources,
    width: u16,
    height: u16,
}

impl SoftwareRaster {
    /// 按视口尺寸构造（尺寸非法时钳到 1×1，调用方应先用 `scene_dims` 校验）。
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            ctx: RenderContext::new(width.max(1), height.max(1)),
            resources: Resources::new(),
            width: width.max(1),
            height: height.max(1),
        }
    }

    /// 当前光栅视口尺寸。
    pub fn size(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    /// 视口尺寸变化（窗口旋转/分屏）。尺寸不变时不触碰内部状态。
    pub fn resize(&mut self, width: u16, height: u16) {
        let (width, height) = (width.max(1), height.max(1));
        if width == self.width && height == self.height {
            return;
        }
        self.width = width;
        self.height = height;
        self.ctx.reset_and_resize(width, height);
    }

    /// 整帧渲染到 `out`（尺寸必须与视口一致）。
    ///
    /// # 返回
    /// `false` = 尺寸非法（场景 0 边或超 u16 上限）。
    pub fn render(&mut self, scene: &Scene, out: &mut Pixmap) -> bool {
        let Some((w, h)) = scene_dims(scene) else {
            return false;
        };
        if (w, h) != self.size() {
            self.resize(w, h);
        }
        self.ctx.reset();
        // 清屏：整幅覆盖（透明清屏色 = 透明黑，与 Android 硬件缓冲语义一致）。
        self.ctx.set_paint(to_alpha_color(scene.clear_color));
        self.ctx.fill_rect(&KurboRect::from_points(
            (0.0, 0.0),
            (f64::from(w), f64::from(h)),
        ));
        replay(scene, &mut self.ctx, &mut self.resources, None);
        self.ctx.render(out, &mut self.resources);
        true
    }

    /// 只重绘脏区到 `out`（其余像素保持上一帧内容）。
    ///
    /// # 返回
    /// `Some(true)` = 已重绘；`Some(false)` = 脏区与视口无交集（调用方可跳过上传）；
    /// `None` = 场景尺寸非法。
    ///
    /// # 为什么脏区也用全帧坐标系
    /// `RasterizerSettings.offset` 只把光栅结果平移进小目标，场景坐标不变——
    /// 缩小上下文会改变光栅器坐标空间，破坏与整帧渲染的逐像素一致性。
    pub fn render_rect(&mut self, scene: &Scene, out: &mut Pixmap, damage: &Rect) -> Option<bool> {
        let (w, h) = scene_dims(scene)?;
        if (w, h) != self.size() {
            self.resize(w, h);
        }
        let x0 = damage.x.max(0.0).floor() as u32;
        let y0 = damage.y.max(0.0).floor() as u32;
        let x1 = (damage.x + damage.w).min(f64::from(w)).ceil() as u32;
        let y1 = (damage.y + damage.h).min(f64::from(h)).ceil() as u32;
        if x0 >= x1 || y0 >= y1 {
            return Some(false);
        }
        let (dw, dh) = ((x1 - x0) as u16, (y1 - y0) as u16);
        if dw >= w && dh >= h {
            return Some(self.render(scene, out));
        }
        self.ctx.reset();
        self.ctx.set_paint(to_alpha_color(scene.clear_color));
        self.ctx.fill_rect(&KurboRect::from_points(
            (0.0, 0.0),
            (f64::from(w), f64::from(h)),
        ));
        replay(scene, &mut self.ctx, &mut self.resources, Some(damage));
        let mut tmp = Pixmap::new(w, dh);
        let settings = RasterizerSettings {
            offset: (-(x0 as i32), -(y0 as i32)),
            target_init: TargetInit::SrcOver,
            ..Default::default()
        };
        self.ctx.render_with(&mut tmp, &mut self.resources, settings);
        // 拷回脏区行段：tmp 列 0 已对应场景 x0（负偏移完成平移）。
        let src = tmp.data_as_u8_slice();
        let dst = out.data_as_u8_slice_mut();
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
}

/// 图片绘制：Image → vello_common ImageSource（Pixmap 共享）。
///
/// # 边界
/// V1 不做采样缩放（1:1 搬运）；大图越界部分被裁剪层丢弃。
///
/// # 缓存
/// 预乘 Pixmap 按 Image 像素指针缓存（thread-local），避免每帧重复
/// RGBA→PremulRgba8 转换（大图 1080×1920 = 8MB 逐像素运算）。
/// 图像预乘缓存的字节预算。
///
/// # 为什么必须有上限
/// 缓存键是像素指针：落笔每次都会产生新 `Arc`，若只增不删，长笔一笔一点
/// 就往里塞一份 4MB 级缓冲，几分钟就会把设备内存吃光（低端机直接 OOM 掉）。
/// 这里按「总字节预算 + 最久未用淘汰」封顶。
const PM_CACHE_BUDGET: u64 = 40 * 1024 * 1024;

/// 预乘缓存条目（缓存的是供 vello_cpu 直接采样的 `Pixmap`）。
struct PremulCache {
    /// 键 = (像素缓冲首地址, 就地改写代际)。
    map: std::collections::HashMap<(usize, u32), (std::sync::Arc<Pixmap>, u64)>,
    /// 最近使用顺序（尾部最新）。命中时挪到尾部，超预算从头部淘汰。
    order: VecDeque<(usize, u32)>,
    /// 每个像素指针最近一次建好的代际：代际恰好 +1 且图片给出了补丁窗口时，
    /// 可以在这张位图上就地补一块，而不是整幅重算。
    latest: std::collections::HashMap<usize, u32>,
    bytes: u64,
}

impl PremulCache {
    fn new() -> Self {
        Self {
            map: std::collections::HashMap::new(),
            order: VecDeque::new(),
            latest: std::collections::HashMap::new(),
            bytes: 0,
        }
    }

    fn touch(&mut self, key: (usize, u32)) {
        if let Some(pos) = self.order.iter().position(|k| *k == key) {
            self.order.remove(pos);
        }
        self.order.push_back(key);
    }

    /// 摘掉某个键（用于「旧的代际」被就地升级成新代际）。
    fn take(&mut self, key: (usize, u32)) -> Option<std::sync::Arc<Pixmap>> {
        let (pm, b) = self.map.remove(&key)?;
        self.bytes = self.bytes.saturating_sub(b);
        if let Some(pos) = self.order.iter().position(|k| *k == key) {
            self.order.remove(pos);
        }
        Some(pm)
    }

    fn insert(&mut self, key: (usize, u32), pm: std::sync::Arc<Pixmap>, bytes: u64) {
        self.bytes = self.bytes.saturating_add(bytes);
        self.map.insert(key, (pm, bytes));
        self.latest.insert(key.0, key.1);
        self.touch(key);
        while self.bytes > PM_CACHE_BUDGET {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some((_, b)) = self.map.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(b);
            }
        }
    }
}

/// 测试用：局部补算命中次数（仅供单测断言「增量路径确实走了」）。
#[cfg(test)]
static INCR_HITS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// 把 `rect` 窗口内的像素就地转成预乘（其余部分保持原样）。
///
/// # 语义
/// 与整幅转换逐像素一致；窗口按行带并行，行数很少时（常见的一小段笔迹）
/// 会退化为单线程，避免为几十行付线程创建开销。
fn premul_rows_in(pm: &mut Pixmap, image: &Image, rect: (u32, u32, u32, u32)) {
    let w = pm.width() as usize;
    let h = pm.height() as usize;
    let x0 = (rect.0 as usize).min(w);
    let x1 = (rect.2 as usize).min(w).max(x0);
    let y0 = (rect.1 as usize).min(h);
    let y1 = (rect.3 as usize).min(h).max(y0);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let pixels = image.pixels();
    let buf = pm.data_mut();
    let rows = y1 - y0;
    crate::platform::perf::add_premul_pixels((rows * (x1 - x0)) as u64);
    let band = &mut buf[y0 * w..y1 * w];
    let min_rows = 24;
    crate::gfx::parallel::par_rows(band, rows, min_rows, |band, first_row| {
        for (i, out) in band.iter_mut().enumerate() {
            let row = first_row + i / w;
            let col = i % w;
            if col < x0 || col >= x1 {
                continue;
            }
            let idx = (y0 + row) * w + col;
            let (r, g, b, a) = (
                pixels[idx * 4],
                pixels[idx * 4 + 1],
                pixels[idx * 4 + 2],
                pixels[idx * 4 + 3],
            );
            *out = vello_cpu::color::PremulRgba8 {
                r: ((r as u16 * a as u16) / 255) as u8,
                g: ((g as u16 * a as u16) / 255) as u8,
                b: ((b as u16 * a as u16) / 255) as u8,
                a,
            };
        }
    });
}

/// 取（必要时生成）`image` 的预乘 Pixmap。
///
/// # 线程模型
/// 缓存是 thread_local：每个渲染线程各自持有一份（当前只有主渲染线程），
/// 命中不走锁，未命中才做一次像素转换（大图按行带并行）。
fn premul_cache_acquire(
    key: (usize, u32),
    w: u16,
    h: u16,
    image: &Image,
) -> std::sync::Arc<Pixmap> {
    let (ptr, rev) = key;
    thread_local! {
        static PM_CACHE: std::cell::RefCell<PremulCache> =
            std::cell::RefCell::new(PremulCache::new());
    }
    PM_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some((cached, _)) = cache.map.get(&key) {
            let pm = std::sync::Arc::clone(cached);
            cache.touch(key);
            return pm;
        }
        // 同一指针、代际恰好 +1、且图片声明了补丁窗口 → 在上一张位图上就地补一块。
        if let Some(rect) = image.patch() {
            let prev_rev = rev.wrapping_sub(1);
            if cache.latest.get(&ptr) == Some(&prev_rev) {
                if let Some(arc) = cache.take((ptr, prev_rev)) {
                    // 上一帧 Scene 已释放时是唯一持有者，`make_mut` 不产生拷贝。
                    let mut arc = arc;
                    let pm = std::sync::Arc::make_mut(&mut arc);
                    if pm.width() == w && pm.height() == h {
                        let bytes = u64::from(w) * u64::from(h) * 4;
                        premul_rows_in(pm, image, rect);
                        #[cfg(test)]
                        INCR_HITS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        cache.insert(key, std::sync::Arc::clone(&arc), bytes);
                        return arc;
                    }
                }
            }
        }
        // 紧凑 RGBA → 预乘 PremulRgba8 Pixmap（vello_cpu 内部位图格式）。
        let mut pm = Pixmap::new(w, h);
        let pixels = image.pixels();
        let px_wide = w as usize;
        crate::gfx::parallel::par_rows(pm.data_mut(), h as usize, 64, |band, first_row| {
            for (i, out) in band.iter_mut().enumerate() {
                let idx = first_row * px_wide + i;
                let (r, g, b, a) = (
                    pixels[idx * 4],
                    pixels[idx * 4 + 1],
                    pixels[idx * 4 + 2],
                    pixels[idx * 4 + 3],
                );
                *out = vello_cpu::color::PremulRgba8 {
                    r: ((r as u16 * a as u16) / 255) as u8,
                    g: ((g as u16 * a as u16) / 255) as u8,
                    b: ((b as u16 * a as u16) / 255) as u8,
                    a,
                };
            }
        });
        crate::platform::perf::add_premul_pixels(u64::from(w) * u64::from(h));
        let bytes = u64::from(w) * u64::from(h) * 4;
        let arc = std::sync::Arc::new(pm);
        cache.insert(key, std::sync::Arc::clone(&arc), bytes);
        arc
    })
}

fn draw_image_cmd(
    ctx: &mut RenderContext,
    resources: &mut Resources,
    image: &Image,
    x: f64,
    y: f64,
    scale: f64,
) {
    let w = image.width().min(u16::MAX as u32) as u16;
    let h = image.height().min(u16::MAX as u32) as u16;

    // 缓存键：像素指针 + 就地改写代际。指针相同但内容被就地补丁过时必须
    // 视为未命中，否则画板增量预览会一直画着旧帧的预乘位图。
    let ptr = image.pixels().as_ptr();
    let key = (ptr as usize, crate::gfx::image::mutation_rev(ptr));
    let t_pm = std::time::Instant::now();
    let pm: std::sync::Arc<Pixmap> = premul_cache_acquire(key, w, h, image);
    // 预乘转换是「图像绘制」里唯一的大头（每帧命中不到缓存时按像素重算）。
    // 记在累加型子阶段里：一帧可能画多张图。
    crate::platform::perf::add_sub_us(crate::platform::perf::SUB_IMAGE, t_pm.elapsed());

    // ImageBrush（vello_common::paint::Image = ImageBrush<ImageSource>）。
    let brush = vello_cpu::Image {
        image: vello_cpu::ImageSource::Pixmap(pm),
        sampler: vello_cpu::peniko::ImageSampler::default(),
    };
    let _ = resources;
    ctx.set_paint(brush);
    // 图片作为"画笔"填充矩形：brush 采样以画布原点为基准，因此先把坐标系
    // 平移到 (x, y) 再填 (0,0,w,h)，否则图像会按绝对坐标错位采样
    // （大图（近原点）看不出，小图（远原点）会整块采到图外变空白）。
    let prev = *ctx.transform();
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    ctx.set_transform(prev * Affine::translate((x, y)) * Affine::scale(scale));
    let rect = KurboRect::from_points((0.0, 0.0), (f64::from(w), f64::from(h)));
    ctx.fill_rect(&rect);
    ctx.set_transform(prev);
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
        Command::DrawImage { image, x, y, scale } => {
            let cmd_rect = Rect {
                x: *x,
                y: *y,
                w: image.width() as f64 * *scale,
                h: image.height() as f64 * *scale,
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
            Command::DrawImage { image, x, y, scale } => {
                draw_image_cmd(ctx, resources, image, *x, *y, *scale);
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
                Command::DrawImage { image, x, y, scale },
                Command::DrawImage {
                    image: image2,
                    x: x2,
                    y: y2,
                    scale: scale2,
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
                    w: image.width() as f64 * *scale,
                    h: image.height() as f64 * *scale,
                };
                let b = Rect {
                    x: *x2,
                    y: *y2,
                    w: image2.width() as f64 * *scale2,
                    h: image2.height() as f64 * *scale2,
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

    /// 只补一块的预乘结果必须与「整幅重算」逐字节一致。
    #[test]
    fn incremental_premul_matches_full_rebuild() {
        let (w, h) = (64u32, 64u32);
        let mut img = Image::from_rgba8(w, h, vec![0x20u8; (w * h * 4) as usize]);
        let mut c0 = Canvas::new(w, h);
        c0.draw_image(&img, 0.0, 0.0);
        let mut s0 = c0.finish();
        s0.clear_color = Color::rgb(0, 0, 0);
        let mut raster = SoftwareRaster::new(w as u16, h as u16);
        let mut out_first = Pixmap::new(w as u16, h as u16);
        assert!(raster.render(&s0, &mut out_first));
        drop(s0);

        // 就地补一块（左上 8..24）：只有这块该变。
        let rect = (8u32, 8u32, 24u32, 24u32);
        let px = img.pixels_mut().expect("unique");
        for y in rect.1..rect.3 {
            for x in rect.0..rect.2 {
                let o = ((y * w + x) * 4) as usize;
                px[o..o + 4].copy_from_slice(&[0, 0, 255, 255]);
            }
        }
        img.note_patch(rect);

        let mut c1 = Canvas::new(w, h);
        c1.draw_image(&img, 0.0, 0.0);
        let mut s1 = c1.finish();
        s1.clear_color = Color::rgb(0, 0, 0);
        let mut out_inc = Pixmap::new(w as u16, h as u16);
        assert!(raster.render(&s1, &mut out_inc));
        drop(s1);

        assert!(
            INCR_HITS.load(std::sync::atomic::Ordering::Relaxed) >= 1,
            "the incremental path must be exercised"
        );

        // 参照：同样的像素内容放在一个全新 Image 上（新指针 → 强制整幅重算）。
        let fresh = Image::from_rgba8(w, h, img.pixels().to_vec());
        let mut c2 = Canvas::new(w, h);
        c2.draw_image(&fresh, 0.0, 0.0);
        let mut s2 = c2.finish();
        s2.clear_color = Color::rgb(0, 0, 0);
        let mut out_full = Pixmap::new(w as u16, h as u16);
        assert!(raster.render(&s2, &mut out_full));

        assert_eq!(
            out_inc.data_as_u8_slice(),
            out_full.data_as_u8_slice(),
            "incremental premultiply must match a full rebuild"
        );
    }

    /// 就地改写图片像素后，第二次渲染必须看到新内容。
    ///
    /// 回归：预乘位图缓存只按像素指针做键，落笔期「同一 Arc 就地补丁」会一直
    /// 命中旧缓存——画板上表现为"只有第一笔落下时的样子"。
    #[test]
    fn in_place_patch_invalidates_premultiplied_cache() {
        let mut img = Image::from_rgba8(64, 64, vec![0u8; 64 * 64 * 4]);
        let mut c = Canvas::new(64, 64);
        c.draw_image(&img, 0.0, 0.0);
        let mut scene = c.finish();
        scene.clear_color = Color::rgb(0, 0, 0);
        let mut raster = SoftwareRaster::new(64, 64);
        let mut out = Pixmap::new(64, 64);
        assert!(raster.render(&scene, &mut out));
        let before = out.data_as_u8_slice()[..16].to_vec();
        assert_eq!(before[0], 0, "empty source must leave the clear colour");

        // 就地补丁：先确保没有别处仍持有同一像素 Arc（Scene 里的命令会持一份）。
        drop(scene);
        let px = img.pixels_mut().expect("unique ownership");
        for v in px.chunks_mut(4) {
            v.copy_from_slice(&[255, 0, 0, 255]);
        }
        let mut c2 = Canvas::new(64, 64);
        c2.draw_image(&img, 0.0, 0.0);
        let mut scene2 = c2.finish();
        scene2.clear_color = Color::rgb(0, 0, 0);
        let mut out2 = Pixmap::new(64, 64);
        assert!(raster.render(&scene2, &mut out2));
        let after = out2.data_as_u8_slice()[..16].to_vec();
        assert_eq!(after[3], 255, "patched pixels must reach the rasterizer");
        assert_eq!(after[0], 255, "patched colour must be the new one");
    }

    /// 图像绘制在偏移位置：目标坐标必须精确落位，不能按画布原点采样。
    ///
    /// 回归：`ImageBrush` 采样以画布原点为基准，旧实现直接 fill 目标矩形，
    /// 导致 (x,y) 远离原点的图（导出页缩略图）整块采到图外变空白。
    #[test]
    fn draw_image_lands_at_offset() {
        // 8x8 纯红不透明图，画到 (40, 50)。
        let mut pixels = vec![0u8; 8 * 8 * 4];
        for px in pixels.chunks_exact_mut(4) {
            px.copy_from_slice(&[255, 0, 0, 255]);
        }
        let image = super::super::image::Image::from_rgba8(8, 8, pixels);
        let mut c = Canvas::new(64, 64);
        c.set_clear_color(Color::WHITE);
        c.draw_image(&image, 40.0, 50.0);
        let pixmap = render_scene(&c.finish()).unwrap();
        let data = pixmap.data();
        let stride = pixmap.width() as usize;
        let at = |x: usize, y: usize| data[y * stride + x];
        // 偏移处（中心 43,53）必须是红色，原点附近必须仍是清屏色。
        let p = at(43, 53);
        assert_eq!(
            (p.r, p.g, p.b, p.a),
            (255, 0, 0, 255),
            "image not at offset"
        );
        let p = at(4, 4);
        assert_eq!(
            (p.r, p.g, p.b, p.a),
            (255, 255, 255, 255),
            "image leaked to origin"
        );
    }

    #[test]
    fn draw_image_scaled_fills_scaled_area() {
        // 放大查看时必须真的铺开：8x8 图按 3 倍画到 (20,24)，覆盖 24x24 区域。
        let mut pixels = vec![0u8; 8 * 8 * 4];
        for px in pixels.chunks_exact_mut(4) {
            px.copy_from_slice(&[0, 128, 255, 255]);
        }
        let image = super::super::image::Image::from_rgba8(8, 8, pixels);
        let mut c = Canvas::new(64, 64);
        c.set_clear_color(Color::WHITE);
        c.draw_image_scaled(&image, 20.0, 24.0, 3.0);
        let pixmap = render_scene(&c.finish()).unwrap();
        let data = pixmap.data();
        let stride = pixmap.width() as usize;
        let at = |x: usize, y: usize| data[y * stride + x];
        // 放大后右下角仍应被覆盖（原尺寸只能覆盖到 28,32）。
        let p = at(40, 44);
        assert_eq!(
            (p.r, p.g, p.b, p.a),
            (0, 128, 255, 255),
            "scaled image did not fill the scaled rect"
        );
        // 区域之外不得渗出。
        let p = at(46, 30);
        assert_eq!(
            (p.r, p.g, p.b, p.a),
            (255, 255, 255, 255),
            "scaled image leaked outside"
        );
    }

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
