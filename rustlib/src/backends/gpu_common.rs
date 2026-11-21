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

/// Pixmap 上传 + blit 渲染（V1 GPU 后端共享路径）。
///
/// # V1 简化说明
/// gfx 命令先经 `gfx::render::render_scene` 软件光栅化为 Pixmap，再整帧
/// 上传为纹理 blit 到 surface——非命令级 GPU 管线（vello 计算着色器
/// 直渲归 V2）；V1 优先保证三后端共用同一 Scene 载体与正确像素输出。
#[cfg(target_os = "android")]
pub fn render_pixmap_frame(base: &mut GpuBase, scene: Scene) -> Result<(), ()> {
    let (w, h) = (base.width, base.height);
    let mut scene = scene;
    scene.width = w;
    scene.height = h;
    let Some(pixmap) = crate::gfx::render::render_scene(&scene) else {
        return Err(());
    };
    log::trace!("gpu blit: pixmap rasterized {}x{}", w, h);
    let pixels: &[u8] = pixmap.data_as_u8_slice();
    let format = base.config.format;

    render_frame_with(base, move |device, queue, frame| {
        log::trace!("gpu blit: frame acquired");
        let texture = device.create_texture(&wgpu::TextureDescriptor {
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
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
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
        let texture_view = texture.create_view(&Default::default());
        let surface_view = frame.texture.create_view(&Default::default());
        let blitter = wgpu::util::TextureBlitter::new(device, format);
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        blitter.copy(device, &mut encoder, &texture_view, &surface_view);
        queue.submit([encoder.finish()]);
        log::trace!("gpu blit: submitted");
        Ok(())
    })
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
