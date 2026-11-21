//! 渲染后端矩阵（PRD §5.3）。
//!
//! # 分层铁律
//! - `backends` 只依赖 `platform` 与各渲染库，**不得向上依赖 `ui`**；
//! - 所有后端实现统一的 [`Backend`] trait，上层（gfx/ui）只面向该 trait；
//! - 后端初始化失败一律返回 `Err`，由选择层（Task 8）降级，禁止 panic。
//!
//! # 场景载体（Task 20 起）
//! 后端消费 `gfx::scene::Scene`（命令列表）——cpu 后端经
//! `gfx::render::render_scene` 软件光栅化后按 stride 搬运；
//! GPU 后端先软件光栅化到 Pixmap 再整帧上传 blit（V1 简化路径：
//! 完整命令级 GPU 管线归 V2）。

pub mod capability;
pub mod cpu;
#[cfg(all(target_os = "android", feature = "vello_gpu"))]
pub mod gpu_common;
pub mod select;
#[cfg(all(target_os = "android", feature = "vello"))]
pub mod vello;
#[cfg(all(target_os = "android", feature = "vello_gpu"))]
pub mod vello_gpu;

#[cfg(target_os = "android")]
use crate::gfx::scene::Scene;

/// 渲染后端抽象（PRD §5.3.4）。
///
/// # 借用约束
/// - `render_frame` 借用窗口，后端存活期间窗口必须有效；
/// - 后端不持有窗口所有权：glue 在 `onNativeWindowDestroyed` 前先 drop 后端。
pub trait Backend {
    /// 渲染一帧并上屏。
    ///
    /// # 失败语义
    /// 返回 `Err` 表示本帧失败（surface 失效/上下文丢失），调用方记录日志；
    /// 连续失败达到阈值时由选择层触发降级（Task 8）。
    #[cfg(target_os = "android")]
    fn render_frame(&mut self, scene: &Scene) -> Result<(), ()>;

    /// 窗口尺寸变化（surface 重配）。
    ///
    /// # 默认实现
    /// 无 surface 状态的后端（cpu）无需处理；GPU 后端覆写并重建 swapchain。
    #[cfg(target_os = "android")]
    fn resize(&mut self, _width: u32, _height: u32) {}
}

/// 后端类型标识（日志与降级链用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    Cpu,
    VelloGpu,
    Vello,
}

impl BackendKind {
    /// 日志名（固定英文，PRD §5.3.3 降级日志格式）。
    pub fn name(self) -> &'static str {
        match self {
            BackendKind::Cpu => "cpu",
            BackendKind::VelloGpu => "vello_gpu",
            BackendKind::Vello => "vello",
        }
    }
}

/// 泛化的后端实例（类型擦除：供 glue 持有"当前后端"）。
///
/// # 借用约束
/// 与具体后端一致：借用窗口，glue 在窗口销毁前先 drop。
#[cfg(target_os = "android")]
pub enum AnyBackend {
    #[cfg(feature = "cpu")]
    Cpu(crate::backends::cpu::CpuBackend),
    #[cfg(feature = "vello_gpu")]
    VelloGpu(crate::backends::vello_gpu::VelloGpuBackend),
    #[cfg(feature = "vello")]
    Vello(crate::backends::vello::VelloBackend),
}

#[cfg(target_os = "android")]
impl AnyBackend {
    /// 尺寸变化分派到具体后端（GPU 后端重配 surface）。
    pub fn resize(&mut self, width: u32, height: u32) {
        match self {
            #[cfg(feature = "cpu")]
            AnyBackend::Cpu(_) => {}
            #[cfg(feature = "vello_gpu")]
            AnyBackend::VelloGpu(b) => b.resize(width, height),
            #[cfg(feature = "vello")]
            AnyBackend::Vello(b) => b.resize(width, height),
        }
    }
}

#[cfg(target_os = "android")]
impl Backend for AnyBackend {
    fn render_frame(&mut self, scene: &Scene) -> Result<(), ()> {
        match self {
            #[cfg(feature = "cpu")]
            AnyBackend::Cpu(b) => b.render_frame(scene),
            #[cfg(feature = "vello_gpu")]
            AnyBackend::VelloGpu(b) => b.render_frame(scene),
            #[cfg(feature = "vello")]
            AnyBackend::Vello(b) => b.render_frame(scene),
        }
    }
}
