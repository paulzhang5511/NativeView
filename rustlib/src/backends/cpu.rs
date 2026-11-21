//! cpu 后端：`ANativeWindow_lock` → 命令级软件光栅化 → 按 stride 逐行
//! 写入 → `unlockAndPost`（PRD §5.3.4）。
//!
//! # stride 不变量
//! `stride` 是一行占用的**像素数**（不是字节），可能 > width；
//! 必须逐行拷贝，直接整块 memcpy 会导致画面斜切（PRD §12.R7）。

#[cfg(target_os = "android")]
use crate::platform::android::window::Window;

#[cfg(target_os = "android")]
use super::Backend;

#[cfg(target_os = "android")]
use crate::gfx::scene::Scene;

/// cpu 后端（持有窗口借用包装，不拥有原生窗口）。
///
/// # 所有权说明
/// 持有 `Window` 值（8 字节裸指针包装，无 Drop，不 release），
/// 而非 `&'w Window` 借用——后端由类型擦除容器（AnyBackend）持有，
/// 借用无法满足其生命周期要求；窗口引用计数的 acquire/release 由 glue 负责。
#[cfg(target_os = "android")]
pub struct CpuBackend {
    window: Window,
}

#[cfg(target_os = "android")]
impl CpuBackend {
    /// 构造：设置 RGBA_8888 缓冲格式。
    ///
    /// # 失败语义
    /// `setBuffersGeometry` 失败返回 `Err`（surface 状态异常），调用方降级跳帧。
    pub fn new(window: Window) -> Result<Self, ()> {
        window.set_rgba8888()?;
        Ok(Self { window })
    }
}

#[cfg(target_os = "android")]
impl Backend for CpuBackend {
    /// V1 资源复用策略：RenderContext/Resources/Pixmap 全量重建（重建
    /// 开销远小于逐帧光栅化，且窗口尺寸变化时必须重建）。
    fn render_frame(&mut self, scene: &Scene) -> Result<(), ()> {
        let mut buffer = self.window.lock()?;
        let (w, h) = buffer.size();
        let stride = buffer.stride();
        if w <= 0 || h <= 0 || stride < w || w > u16::MAX as i32 || h > u16::MAX as i32 {
            // 尺寸非法/超上限/stride 倒挂（NDK 不允许）：放弃本帧。
            return Err(());
        }
        let (w, h) = (w as u16, h as u16);

        // 命令级软件光栅化（gfx::render 翻译层，含裁剪/路径/渐变/字形）。
        let mut scene = scene.clone();
        scene.width = u32::from(w);
        scene.height = u32::from(h);
        let Some(pixmap) = crate::gfx::render::render_scene(&scene) else {
            return Err(());
        };

        // 逐行搬入窗口缓冲（Pixmap 紧凑 → 按 stride 步进）。
        let src = pixmap.data_as_u8_slice();
        let row = (w as usize) * 4;
        let pitch = (stride as usize) * 4;
        let dst = buffer.pixels_mut();
        for y in 0..h as usize {
            dst[y * pitch..y * pitch + row].copy_from_slice(&src[y * row..(y + 1) * row]);
        }

        // 上屏。
        buffer.unlock()
    }
}

#[cfg(test)]
mod tests {
    /// stride 步进拷贝的核心逻辑：构造 stride > width 的假缓冲验证逐行写入
    /// 不斜切（PRD §12.R7，Task 6 验收标准）。
    ///
    /// 场景：width=4、height=3、stride=8（每行后 4 像素为填充区），
    /// 源为紧凑 Pixmap；验证每行内容精确落位且填充区不被越界写入。
    #[test]
    fn stride_copy_with_padding() {
        let w: usize = 4;
        let h: usize = 3;
        let stride: usize = 8;
        // 紧凑源：每行 4 像素 RGBA，行号编码进 R 通道。
        let mut src = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) * 4;
                src[i] = y as u8; // R = 行号
                src[i + 1] = x as u8; // G = 列号
                src[i + 2] = 0;
                src[i + 3] = 0xFF;
            }
        }
        // 目标：stride 步进，填充区预置哨兵 0xAA。
        let mut dst = vec![0xAAu8; stride * h * 4];
        let row = w * 4;
        let pitch = stride * 4;
        for y in 0..h {
            dst[y * pitch..y * pitch + row].copy_from_slice(&src[y * row..(y + 1) * row]);
        }
        // 每行有效区内容正确。
        for y in 0..h {
            for x in 0..w {
                let i = y * pitch + x * 4;
                assert_eq!(dst[i], y as u8, "row mismatch at ({x},{y})");
                assert_eq!(dst[i + 1], x as u8, "col mismatch at ({x},{y})");
            }
            // 填充区未被触碰（哨兵不变 → 无越界/斜切）。
            for b in &dst[y * pitch + row..(y + 1) * pitch] {
                assert_eq!(*b, 0xAA);
            }
        }
    }
}
