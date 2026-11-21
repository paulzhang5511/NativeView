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
    /// 上一帧 Scene（帧间 diff 用；首帧 None → 整帧）。
    prev_scene: Option<Scene>,
    /// 持久像素缓冲（帧间复用；窗口多缓冲内容不持久，必须自持）。
    pixmap: Option<crate::gfx::render::Pixmap>,
}

#[cfg(target_os = "android")]
impl CpuBackend {
    /// 构造：设置 RGBA_8888 缓冲格式。
    ///
    /// # 失败语义
    /// `setBuffersGeometry` 失败返回 `Err`（surface 状态异常），调用方降级跳帧。
    pub fn new(window: Window) -> Result<Self, ()> {
        window.set_rgba8888()?;
        Ok(Self {
            window,
            prev_scene: None,
            pixmap: None,
        })
    }
}

#[cfg(target_os = "android")]
impl Backend for CpuBackend {
    /// 帧调度（PRD §12.R9）：
    /// 1. 与上一帧 diff：无变化 → 跳过（不 lock/post，静止帧零开销）；
    /// 2. 有变化 → 仅重绘脏区到持久 Pixmap（damage-based partial redraw），
    ///    脏区外像素保持上一帧内容（避免全帧光栅化）；
    /// 3. 持久 Pixmap 整帧 memcpy 到窗口缓冲（多缓冲模型要求整帧上屏）。
    fn render_frame(&mut self, mut scene: Scene) -> Result<(), ()> {
        // 先 diff（不 lock）：静止帧直接返回，避免无谓的 lock/post。
        if let (Some(prev), Some(pm)) = (&self.prev_scene, &self.pixmap) {
            if pm.width() == scene.width as u16 && pm.height() == scene.height as u16 {
                if crate::gfx::render::diff_damage(prev, &scene).is_none() {
                    return Ok(()); // 静止帧：不上屏。
                }
            }
        }

        let mut buffer = self.window.lock()?;
        let (w, h) = buffer.size();
        let stride = buffer.stride();
        if w <= 0 || h <= 0 || stride < w || w > u16::MAX as i32 || h > u16::MAX as i32 {
            // 尺寸非法/超上限/stride 倒挂（NDK 不允许）：放弃本帧。
            return Err(());
        }
        let (w, h) = (w as u16, h as u16);
        scene.width = u32::from(w);
        scene.height = u32::from(h);

        // 持久 Pixmap 尺寸变化或首帧 → 全帧重建；否则仅重绘脏区。
        let need_full = self.pixmap.is_none()
            || self.pixmap.as_ref().unwrap().width() != w
            || self.pixmap.as_ref().unwrap().height() != h
            || self.prev_scene.is_none();

        if need_full {
            let Some(pixmap) = crate::gfx::render::render_scene(&scene) else {
                return Err(());
            };
            self.pixmap = Some(pixmap);
        } else {
            // 局部重绘：仅光栅化脏区行段，脏区外像素保持上一帧。
            let prev = self.prev_scene.as_ref().unwrap();
            let damage =
                crate::gfx::render::diff_damage(prev, &scene).unwrap_or(crate::gfx::scene::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: f64::from(w),
                    h: f64::from(h),
                });
            let pm = self.pixmap.as_mut().unwrap();
            let _ = crate::gfx::render::render_damage(&scene, pm, &damage);
        }
        self.prev_scene = Some(scene);

        // 逐行搬入窗口缓冲（Pixmap 紧凑 → 按 stride 步进）。
        let src = self.pixmap.as_ref().unwrap().data_as_u8_slice();
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
