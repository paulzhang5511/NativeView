//! GPU 后端公共初始化：wgpu Instance/adapter/surface（仅 Vulkan）。
//!
//! # 设计
//! - 两个 GPU 后端（vello_gpu / vello）共享同一套 wgpu 初始化流程，
//!   仅渲染阶段不同，故抽取公共模块避免重复；
//! - **仅启用 Vulkan 后端**（PRD §5.3.3）：`gles` 会引入 `ndk-sys`，禁止；
//! - Android 无默认异步运行时，用 `pollster` 阻塞等待 wgpu future。
//!
//! # 失败语义
//! 所有初始化函数失败一律返回 `Err(())`，由选择层（Task 8）降级，禁止 panic。

#[cfg(target_os = "android")]
use raw_window_handle::{AndroidDisplayHandle, AndroidNdkWindowHandle};

use super::Scene;

/// 「CPU 光栅 → 纹理 → blit 上屏」的持久化呈现器（V1 GPU 后端共享路径）。
///
/// # V1 简化说明
/// gfx 命令先经 `SoftwareRaster` 软件光栅化为 Pixmap，再上传为纹理 blit 到
/// surface——非命令级 GPU 管线（vello 计算着色器直渲归 V2）；V1 优先保证
/// 三后端共用同一 Scene 载体与正确像素输出。
///
/// # 为什么每一件都要持久化
/// 早期实现每帧新建 `RenderContext`/`Resources`/`Texture`/`TextureBlitter`：
/// - `Resources` 里是**字形 atlas 与前程缓存**，重建等于每帧重光栅全部字形；
/// - 新建 `wgpu::Texture` 是驱动侧分配 + 整帧 10MB 上传；
/// - `TextureBlitter::new` 每帧编译/blit 管线对象。
/// 真机实测这三项让「静态页」也要 16ms/帧，持久化后同一帧只剩光栅与一次上传。
#[cfg(target_os = "android")]
pub struct PixmapPresenter {
    /// 跨帧复用的软件光栅器（含字形缓存）。
    raster: Option<crate::gfx::render::SoftwareRaster>,
    /// 跨帧复用的 CPU 像素缓冲（脏区重绘时其余像素保持上一帧）。
    pixmap: Option<crate::gfx::render::Pixmap>,
    /// 上传目标纹理（尺寸变化时重建）。
    texture: Option<wgpu::Texture>,
    /// 纹理视图（与 texture 同生命周期，避免每帧 create_view）。
    view: Option<wgpu::TextureView>,
    /// blit 管线对象（与 surface format 绑定，只建一次）。
    blitter: Option<wgpu::util::TextureBlitter>,
    /// 当前纹理/像素缓冲尺寸。
    size: (u32, u32),
}

#[cfg(target_os = "android")]
impl PixmapPresenter {
    /// 空呈现器（首帧懒分配）。
    pub fn new() -> Self {
        Self {
            raster: None,
            pixmap: None,
            texture: None,
            view: None,
            blitter: None,
            size: (0, 0),
        }
    }

    /// 尺寸变化：丢弃纹理与视图（像素缓冲由光栅器按需重建）。
    fn invalidate(&mut self) {
        self.texture = None;
        self.view = None;
        self.size = (0, 0);
    }

    /// 渲染并上屏一帧。
    ///
    /// # 脏区语义（`Scene.damage`）
    /// - `Some(空)`：整帧跳过（不 acquire、不 present），静止页零 vsync 占用；
    /// - `Some(rect)`：只光栅该块、只上传该块，其余像素沿用上一帧纹理；
    /// - `None`：整帧光栅 + 整帧上传。
    pub fn render(&mut self, base: &mut GpuBase, mut scene: Scene) -> Result<(), ()> {
        use crate::platform::perf::{record_sub_us, SUB_PRESENT, SUB_RASTER, SUB_UPLOAD};
        let (w, h) = (base.width, base.height);
        scene.width = w;
        scene.height = h;

        // 尺寸变化：光栅器视口与纹理都要重建（脏区差分不再成立）。
        if self.size != (w, h) {
            self.invalidate();
            self.size = (w, h);
        }

        // 空脏区 = 本帧无变化：直接返回，连 acquire 都不做。
        let damage = match scene.damage {
            Some(d) if d.w <= 0.0 || d.h <= 0.0 => return Ok(()),
            other => other,
        };

        crate::platform::perf::reset_sub(crate::platform::perf::SUB_IMAGE);
        let t_raster = std::time::Instant::now();
        let raster = self
            .raster
            .get_or_insert_with(|| crate::gfx::render::SoftwareRaster::new(w as u16, h as u16));
        let pixmap = self
            .pixmap
            .get_or_insert_with(|| crate::gfx::render::Pixmap::new(w as u16, h as u16));
        // 只有「已有完整上一帧」时才走局部重绘，否则必须整帧。
        // `Some(true)` = 已局部重绘；`Some(false)` = 脏区无像素；`None` = 需整帧。
        let partial = match damage {
            Some(d) if self.texture.is_some() => raster.render_rect(&scene, pixmap, &d),
            _ => None,
        };
        let ready = match partial {
            Some(_) => true,
            None => raster.render(&scene, pixmap),
        };
        if let Some(d) = damage.filter(|_| ready) {
            if matches!(partial, Some(_)) {
                crate::platform::perf::add_raster_area(
                    d.w.max(0.0) as u64 * d.h.max(0.0) as u64,
                    false,
                );
            }
        }
        record_sub_us(SUB_RASTER, t_raster.elapsed());
        if matches!(partial, None) {
            crate::platform::perf::add_raster_area(0, true);
        }
        if !ready {
            return Err(());
        }
        crate::platform::perf::note_frame_mode(matches!(partial, Some(true)));

        let format = base.config.format;
        // 局部重绘成功时只上传脏块；整帧渲染或纹理尚未建立时整帧上传。
        let needs_upload_full = !matches!(partial, Some(true)) || self.texture.is_none();
        let t_upload = std::time::Instant::now();
        if needs_upload_full {
            let texture = base.ctx.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("pixmap-upload"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            self.view = Some(texture.create_view(&Default::default()));
            self.texture = Some(texture);
            let pixels = pixmap.data_as_u8_slice();
            base.ctx.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: self.texture.as_ref().ok_or(())?,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(w * 4),
                    rows_per_image: Some(h),
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
        } else if let Some(d) = damage {
            // 局部上传：只写脏矩形（行跨距仍是整帧宽度）。
            let x0 = d.x.max(0.0).floor() as u32;
            let y0 = d.y.max(0.0).floor() as u32;
            let x1 = (d.x + d.w).min(f64::from(w)).ceil() as u32;
            let y1 = (d.y + d.h).min(f64::from(h)).ceil() as u32;
            if x1 > x0 && y1 > y0 {
                let pitch = (w * 4) as usize;
                let off = (y0 as usize) * pitch + (x0 as usize) * 4;
                let pixels = pixmap.data_as_u8_slice();
                base.ctx.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: self.texture.as_ref().ok_or(())?,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x: x0, y: y0, z: 0 },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &pixels[off..],
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(w * 4),
                        rows_per_image: Some(h),
                    },
                    wgpu::Extent3d {
                        width: x1 - x0,
                        height: y1 - y0,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }
        record_sub_us(SUB_UPLOAD, t_upload.elapsed());

        let t_acq = std::time::Instant::now();
        let frame = base.acquire_frame()?;
        // 等 vsync 的时长单独记：它不是本帧的计算成本（见 `SUB_ACQUIRE` 注释）。
        record_sub_us(
            crate::platform::perf::SUB_ACQUIRE,
            t_acq.elapsed(),
        );
        let t_present = std::time::Instant::now();
        let surface_view = frame.texture.create_view(&Default::default());
        let blitter = match self.blitter.as_ref() {
            Some(b) => b,
            None => {
                let b = wgpu::util::TextureBlitter::new(&base.ctx.device, format);
                self.blitter = Some(b);
                self.blitter.as_ref().ok_or(())?
            }
        };
        let mut encoder = base
            .ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        blitter.copy(
            &base.ctx.device,
            &mut encoder,
            self.view.as_ref().ok_or(())?,
            &surface_view,
        );
        base.ctx.queue.submit([encoder.finish()]);
        base.ctx.queue.present(frame);
        record_sub_us(SUB_PRESENT, t_present.elapsed());
        Ok(())
    }
}

/// GPU 后端共享的 wgpu 资源集合。
///
/// # 生命周期
/// surface 借用 `ANativeWindow*`（经 raw-window-handle 包裹），
/// 后端 drop 时 surface 先于窗口引用释放（glue 保证窗口 drop 顺序在后）。
#[cfg(target_os = "android")]
pub struct GpuContext {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_info: String,
}

/// GPU 后端公共基座：封装 wgpu 初始化与 resize。
///
/// 子类（vello_gpu / vello 后端）通过组合持有本类型，
/// 复用 surface 获取 / 帧呈现逻辑，仅实现各自的渲染命令。
#[cfg(target_os = "android")]
pub struct GpuBase {
    pub ctx: GpuContext,
    pub config: wgpu::SurfaceConfiguration,
    /// 缓存宽度（resize 检测）。
    pub width: u32,
    /// 缓存高度（resize 检测）。
    pub height: u32,
}

#[cfg(target_os = "android")]
impl GpuBase {
    /// 初始化：Instance（仅 Vulkan）→ surface（ANativeWindow）→ adapter → device/queue。
    ///
    /// # 参数
    /// - `window`：借用 `ANativeWindow*`，借用期内必须有效（glue 保证）；
    /// - `width`/`height`：初始尺寸，来自 `ANativeWindow_getWidth/Height`。
    ///
    /// # 失败语义
    /// 任一步失败（无 Vulkan 适配器 / surface 不支持 / 请求设备失败）返回 `Err(())`，
    /// 调用方记录日志并降级。
    pub fn new(window_ptr: *mut std::ffi::c_void, width: u32, height: u32) -> Result<Self, ()> {
        use std::ptr::NonNull;

        log::info!("gpu init: creating instance (vulkan only)");

        // SAFETY: window_ptr 来自 glue 已 acquire 的 ANativeWindow*，
        // 借用期内（后端存活期间）窗口有效；'static 借用由后端 drop 先于窗口 release 保证。
        let raw_window =
            AndroidNdkWindowHandle::new(NonNull::new(window_ptr).ok_or(())?.cast()).into();
        let raw_display = AndroidDisplayHandle::new().into();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            flags: wgpu::InstanceFlags::default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            backend_options: wgpu::BackendOptions::default(),
            display: None,
        });

        // SAFETY: 见上方 raw_window 注释；display 为空对象，无借用风险。
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(raw_display),
                raw_window_handle: raw_window,
            })
        }
        .map_err(|_| ())?;

        log::info!("gpu init: surface created");

        // 阻塞等待适配器（Android 无异步运行时）。
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::<
            '_,
            '_,
        > {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
            apply_limit_buckets: false,
        }))
        .map_err(|_| ())?;

        log::info!("gpu init: adapter acquired");

        // 适配器信息用于降级日志（SoC 白名单探测在 Task 8 接入）。
        let info = adapter.get_info();
        let adapter_info = format!("{} ({})", info.name, info.driver);
        log::info!("gpu init: adapter info: {}", adapter_info);

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("nativeview-gpu"),
            required_features: wgpu::Features::default(),
            required_limits: wgpu::Limits::default(),
            ..Default::default()
        }))
        .map_err(|_| ())?;

        log::info!("gpu init: device acquired");

        // wgpu 默认对未捕获错误直接 panic 且消息走 stderr（logcat 不可见），
        // 改为记录日志后由调用方按 Err(()) 降级，避免主循环线程静默死亡。
        let error_handler: std::sync::Arc<dyn wgpu::UncapturedErrorHandler> =
            std::sync::Arc::new(|err| {
                log::error!("wgpu uncaptured error: {}", err);
            });
        device.on_uncaptured_error(error_handler);

        let capabilities = surface.get_capabilities(&adapter);
        // 优先 RGBA 非线性格式：vello_gpu 输出的像素即 sRGB 编码后的值
        // （与 cpu 后端逐像素一致），若选 sRGB 视图会导致硬件二次编码偏亮。
        let format = capabilities
            .formats
            .iter()
            .find(|f| {
                matches!(
                    f,
                    wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Bgra8Unorm
                )
            })
            .copied()
            .ok_or(())?;

        // Android Vulkan surface 仅支持 Inherit，Opaque 会触发 Validation Error，
        // 故按驱动声明的能力选择；Opaque 优先（内容不透明），否则回退首个支持项。
        let alpha_mode = capabilities
            .alpha_modes
            .iter()
            .find(|m| **m == wgpu::CompositeAlphaMode::Opaque)
            .copied()
            .unwrap_or(capabilities.alpha_modes[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            color_space: wgpu::SurfaceColorSpace::Auto,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
        };
        surface.configure(&device, &config);
        log::info!("gpu init: surface configured, format={:?}", config.format);

        Ok(Self {
            ctx: GpuContext {
                surface,
                device,
                queue,
                adapter_info,
            },
            config,
            width,
            height,
        })
    }

    /// 获取下一帧的 surface 纹理。
    ///
    /// # 失败语义
    /// `Outdated`/`Lost` 返回 `Err`（上层触发重建或降级）；
    /// `Timeout`/`Occluded` 同样返回 `Err`（跳帧即可，本帧放弃）。
    pub fn acquire_frame(&mut self) -> Result<wgpu::SurfaceTexture, ()> {
        match self.ctx.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) => Ok(t),
            wgpu::CurrentSurfaceTexture::Suboptimal(t) => Ok(t),
            _ => Err(()),
        }
    }

    /// 尺寸变化时重新配置 surface（构造独立 render target 的后端自行重建内部资源）。
    pub fn resize(&mut self, width: u32, height: u32) {
        if self.width == width && self.height == height {
            return;
        }
        self.width = width;
        self.height = height;
        self.config.width = width;
        self.config.height = height;
        self.ctx.surface.configure(&self.ctx.device, &self.config);
    }
}

/// GPU 后端共用的"渲染一帧"骨架：acquire → 渲染回调 → submit → present。
///
/// 子后端通过 `render_into` 闭包注入各自的渲染命令，避免重复 acquire/present 样板。
#[cfg(target_os = "android")]
pub fn render_frame_with<F>(base: &mut GpuBase, render_into: F) -> Result<(), ()>
where
    F: FnOnce(&wgpu::Device, &wgpu::Queue, &wgpu::SurfaceTexture) -> Result<(), ()>,
{
    let frame = base.acquire_frame()?;
    render_into(&base.ctx.device, &base.ctx.queue, &frame)?;
    base.ctx.queue.present(frame);
    log::trace!("gpu blit: presented");
    Ok(())
}
