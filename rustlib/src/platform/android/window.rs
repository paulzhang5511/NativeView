//! 平台安全封装层：窗口（借用语义，对上不暴露裸指针）。

use super::sys::window::{ANativeWindow, ANativeWindow_Buffer};

/// 窗口的安全借用。
///
/// # 不变量
/// - 不拥有窗口：`drop` 时不调用 `ANativeWindow_release`，
///   生命周期由 activity 回调（created/destroyed）管理，本类型只借用；
/// - 指针保证非空：由 `from_raw` 构造时校验。
#[derive(Debug)]
pub struct Window {
    raw: *mut ANativeWindow,
}

/// 上屏缓冲区的安全借用：`unlock` 后立即失效。
/// `window` 字段仅在编译期保证借用关系（drop 检查），运行时不读取。
pub struct WindowBuffer<'w> {
    #[allow(dead_code)]
    window: &'w Window,
    inner: ANativeWindow_Buffer,
}

impl Window {
    /// 从框架回调交出的原生指针构造借用。
    ///
    /// # 安全
    /// 调用方必须保证 `raw` 来自 `onNativeWindowCreated` 且在借用期间保持有效；
    /// 返回 `None` 表示框架传了 NULL（视作协议错误，静默跳过）。
    pub fn from_raw(raw: *mut ANativeWindow) -> Option<Self> {
        if raw.is_null() {
            None
        } else {
            Some(Self { raw })
        }
    }

    /// 借用的原生指针（仅供同层 FFI 调用；Task 5 主循环与后端将使用）。
    #[allow(dead_code)]
    pub(super) fn as_ptr(&self) -> *mut ANativeWindow {
        self.raw
    }

    /// 窗口宽度（像素）。
    pub fn width(&self) -> i32 {
        // SAFETY: raw 构造时已校验非空。
        #[cfg(target_os = "android")]
        {
            unsafe { super::sys::window::ANativeWindow_getWidth(self.raw) }
        }
        #[cfg(not(target_os = "android"))]
        {
            0
        }
    }

    /// 窗口高度（像素）。
    pub fn height(&self) -> i32 {
        // SAFETY: 同上。
        #[cfg(target_os = "android")]
        {
            unsafe { super::sys::window::ANativeWindow_getHeight(self.raw) }
        }
        #[cfg(not(target_os = "android"))]
        {
            0
        }
    }

    /// 设置换冲区几何为 0×0（继承窗口尺寸）+ RGBA_8888。
    ///
    /// # 失败语义
    /// 返回 `Err` 表示 NDK 拒绝该几何（如 bad window state），
    /// 调用方应放弃本帧并记录日志，禁止重试循环。
    pub fn set_rgba8888(&self) -> Result<(), ()> {
        // SAFETY: raw 构造时已校验非空；0 表示继承当前窗口尺寸。
        #[cfg(target_os = "android")]
        let r = unsafe {
            super::sys::window::ANativeWindow_setBuffersGeometry(
                self.raw,
                0,
                0,
                super::sys::window::WINDOW_FORMAT_RGBA_8888,
            )
        };
        #[cfg(not(target_os = "android"))]
        let r = 0;
        if r == 0 { Ok(()) } else { Err(()) }
    }

    /// 锁定缓冲区准备写入。
    ///
    /// # 失败语义
    /// 返回 `Err` 表示锁失败（如 surface 尚未就绪），本帧应跳过。
    pub fn lock(&self) -> Result<WindowBuffer<'_>, ()> {
        #[cfg_attr(not(target_os = "android"), allow(unused_mut))]
        let mut inner: ANativeWindow_Buffer = unsafe { std::mem::zeroed() };
        #[cfg(target_os = "android")]
        // SAFETY: inner 为合法出参；dirty bounds 传 NULL 表示整帧重绘。
        let r = unsafe {
            super::sys::window::ANativeWindow_lock(self.raw, &mut inner, core::ptr::null_mut())
        };
        #[cfg(not(target_os = "android"))]
        let r = 0;
        if r == 0 {
            Ok(WindowBuffer {
                window: self,
                inner,
            })
        } else {
            Err(())
        }
    }
}

impl WindowBuffer<'_> {
    /// 帧缓冲尺寸（width × height，像素）。
    pub fn size(&self) -> (i32, i32) {
        (self.inner.width, self.inner.height)
    }

    /// 行步进（像素数，可能大于 width，写入必须按它逐行步进）。
    pub fn stride(&self) -> i32 {
        self.inner.stride
    }

    /// 帧位图（RGBA8888，借用语义，`unlock` 后失效）。
    ///
    /// # 安全约束
    /// 返回切片覆盖整个缓冲，长度按 `height × stride × 4` 计算；
    /// 调用方只能在持有本借用期间写入，不得缓存到 `unlock` 之后。
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        let len = (self.inner.height as usize).saturating_mul(self.inner.stride as usize) * 4;
        // SAFETY: lock 成功后 bits 指向框架交出的缓冲；
        // 主机桩下 bits 为 NULL，返回空切片。
        if self.inner.bits.is_null() {
            &mut []
        } else {
            unsafe { std::slice::from_raw_parts_mut(self.inner.bits, len) }
        }
    }

    /// 解锁并上屏。
    ///
    /// # 失败语义
    /// 返回 `Err` 表示 post 失败（如 surface 已销毁），调用方记录日志即可，
    /// 不做补偿性重试。
    pub fn unlock(self) -> Result<(), ()> {
        // SAFETY: 借用仍有效（`self` 尚未 drop）；成功后缓冲即失效。
        #[cfg(target_os = "android")]
        let r = unsafe { super::sys::window::ANativeWindow_unlockAndPost(self.window.raw) };
        #[cfg(not(target_os = "android"))]
        let r = 0;
        if r == 0 { Ok(()) } else { Err(()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// NULL 指针必须被拒绝，绝不进入 FFI 路径。
    #[test]
    fn null_raw_rejected() {
        assert!(Window::from_raw(core::ptr::null_mut()).is_none());
    }

    /// 借用构造必须原样保持指针。
    #[test]
    fn from_raw_keeps_pointer() {
        let fake = 0x1000usize as *mut ANativeWindow;
        let w = Window::from_raw(fake).unwrap();
        assert_eq!(w.as_ptr(), fake);
    }
}
