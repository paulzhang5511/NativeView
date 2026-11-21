//! vello 后端：全 GPU 管线后端（PRD §5.3.2）。
//!
//! # V1 渲染路径
//! V1 复用 GPU 公共基座：命令经软件光栅化为 Pixmap 后整帧上传 blit
//! （vello 计算着色器命令级直渲归 V2——V1 优先打通 Scene 载体统一，
//! 后端选择/降级链与真机验证不变）。

use super::Backend;
use super::gpu_common::{GpuBase, render_pixmap_frame};
use crate::gfx::scene::Scene;

/// vello 后端（借用窗口，不拥有）。
#[cfg(target_os = "android")]
pub struct VelloBackend {
    base: GpuBase,
}

#[cfg(target_os = "android")]
impl VelloBackend {
    /// 初始化：wgpu 基座。
    ///
    /// # 失败语义
    /// 无 Vulkan / surface 构建失败返回 `Err`，由选择层（Task 8）降级。
    pub fn new(window_ptr: *mut std::ffi::c_void, width: u32, height: u32) -> Result<Self, ()> {
        let base = GpuBase::new(window_ptr, width, height)?;
        Ok(Self { base })
    }
}

#[cfg(target_os = "android")]
impl Backend for VelloBackend {
    fn render_frame(&mut self, scene: &Scene) -> Result<(), ()> {
        render_pixmap_frame(&mut self.base, scene)
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.base.resize(width, height);
    }
}
