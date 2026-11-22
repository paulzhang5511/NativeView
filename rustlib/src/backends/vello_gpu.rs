//! vello_gpu 后端：GPU 预处理后端（PRD §5.3.2）。
//!
//! # V1 渲染路径
//! 命令经软件光栅化为 Pixmap 后整帧上传 blit（vello_gpu 预处理直渲归 V2）。

use super::Backend;
use super::gpu_common::{GpuBase, PixmapPresenter};
use crate::gfx::scene::Scene;

/// vello_gpu 后端（借用窗口，不拥有）。
#[cfg(target_os = "android")]
pub struct VelloGpuBackend {
    pub(crate) base: GpuBase,
    /// 持久化「光栅 → 纹理 → blit」呈现器（纹理/光栅器/字形缓存跨帧复用）。
    presenter: PixmapPresenter,
}

#[cfg(target_os = "android")]
impl VelloGpuBackend {
    /// 初始化：wgpu 基座。
    ///
    /// # 失败语义
    /// 无 Vulkan 适配器或基座构建失败均返回 `Err`，调用方降级。
    pub fn new(window_ptr: *mut std::ffi::c_void, width: u32, height: u32) -> Result<Self, ()> {
        let base = GpuBase::new(window_ptr, width, height)?;
        Ok(Self {
            base,
            presenter: PixmapPresenter::new(),
        })
    }
}

#[cfg(target_os = "android")]
impl Backend for VelloGpuBackend {
    fn render_frame(&mut self, mut scene: Scene) -> Result<(), ()> {
        // V1：软件光栅化 → 纹理上传 → blit（命令级 GPU 管线归 V2）。
        self.presenter.render(&mut self.base, scene)
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.base.resize(width, height);
    }
}
