//! 平台安全封装层：NativeActivity 生命周期借用。

use super::asset::AssetManager;
use super::sys::activity::{ANativeActivity, ANativeActivityCallbacks};
use super::sys::window::ANativeWindow;

/// NativeActivity 的安全借用。
///
/// # 不变量
/// - 回调期间由框架保证 activity 存活；本类型只在回调栈内构造，
///   绝不缓存到回调之外（否则悬垂，PRD §12.R12）。
/// - `env` 仅主线程可用，本封装不暴露 JNI 上下文。
#[derive(Debug)]
pub struct Activity<'a> {
    raw: *mut ANativeActivity,
    /// 标记借用关系（编译期提示，运行时不使用）。
    _borrow: core::marker::PhantomData<&'a ()>,
}

impl<'a> Activity<'a> {
    /// 从回调参数构造借用；NULL 视作协议错误返回 `None`。
    pub fn from_raw(raw: *mut ANativeActivity) -> Option<Self> {
        if raw.is_null() {
            None
        } else {
            Some(Self {
                raw,
                _borrow: core::marker::PhantomData,
            })
        }
    }

    /// 平台 SDK 版本（用于 `__INTRODUCED_IN` 符号门控）。
    pub fn sdk_version(&self) -> i32 {
        // SAFETY: 借用期间 activity 有效。
        unsafe { (*self.raw).sdk_version }
    }

    /// 借出 AssetManager（生命周期随 activity）。
    pub fn asset_manager(&self) -> Option<AssetManager> {
        // SAFETY: 框架已填充字段；空指针交给 AssetManager::from_raw 拒绝。
        unsafe { AssetManager::from_raw((*self.raw).asset_manager as *mut _) }
    }

    /// 请求结束当前 activity（任何回调内可调）。
    pub fn finish(&self) {
        // SAFETY: 借用期间 activity 有效。
        #[cfg(target_os = "android")]
        unsafe {
            super::sys::activity::ANativeActivity_finish(self.raw)
        };
        #[cfg(not(target_os = "android"))]
        {}
    }
}

/// 回调注册辅助：包装框架回调表的写入。
///
/// # 安全约束
/// - callbacks 由框架分配，本类型只写字段；
/// - 回调签名必须与 `ANativeActivityCallbacks` 字段类型完全一致，
///   否则错位调用即崩溃（PRD §12.R12）。
pub struct CallbackRegistry {
    callbacks: *mut ANativeActivityCallbacks,
}

impl CallbackRegistry {
    /// 挂接回调表（`onCreate` 时由框架交出）。
    pub fn new(callbacks: *mut ANativeActivityCallbacks) -> Option<Self> {
        if callbacks.is_null() {
            None
        } else {
            Some(Self { callbacks })
        }
    }

    /// 注册生命周期回调集合（V1 路线 B 最小集）。
    ///
    /// # 安全
    /// 所有闭包必须在 activity 存活期间有效；本方法只写字段，
    /// 未注册的回调保持 NULL（框架按 NULL 跳过）。
    pub unsafe fn register(&mut self, cbs: &Callbacks) {
        // SAFETY: 框架分配的回调表，写字段本身安全；由调用方保证函数指针签名正确。
        unsafe {
            (*self.callbacks).on_start = Some(cbs.on_start);
            (*self.callbacks).on_resume = Some(cbs.on_resume);
            (*self.callbacks).on_pause = Some(cbs.on_pause);
            (*self.callbacks).on_stop = Some(cbs.on_stop);
            (*self.callbacks).on_destroy = Some(cbs.on_destroy);
            (*self.callbacks).on_window_focus_changed = Some(cbs.on_window_focus_changed);
            (*self.callbacks).on_native_window_created = Some(cbs.on_native_window_created);
            (*self.callbacks).on_native_window_resized = Some(cbs.on_native_window_resized);
            (*self.callbacks).on_native_window_destroyed = Some(cbs.on_native_window_destroyed);
            (*self.callbacks).on_input_queue_created = Some(cbs.on_input_queue_created);
            (*self.callbacks).on_input_queue_destroyed = Some(cbs.on_input_queue_destroyed);
            (*self.callbacks).on_configuration_changed = Some(cbs.on_configuration_changed);
            (*self.callbacks).on_low_memory = Some(cbs.on_low_memory);
        }
    }
}

/// 回调函数表（V1 集合；字段签名与 NDK 头文件一致）。
#[allow(clippy::type_complexity)]
pub struct Callbacks {
    pub on_start: unsafe extern "C" fn(*mut ANativeActivity),
    pub on_resume: unsafe extern "C" fn(*mut ANativeActivity),
    pub on_pause: unsafe extern "C" fn(*mut ANativeActivity),
    pub on_stop: unsafe extern "C" fn(*mut ANativeActivity),
    pub on_destroy: unsafe extern "C" fn(*mut ANativeActivity),
    pub on_window_focus_changed: unsafe extern "C" fn(*mut ANativeActivity, core::ffi::c_int),
    pub on_native_window_created: unsafe extern "C" fn(*mut ANativeActivity, *mut ANativeWindow),
    pub on_native_window_resized: unsafe extern "C" fn(*mut ANativeActivity, *mut ANativeWindow),
    pub on_native_window_destroyed: unsafe extern "C" fn(*mut ANativeActivity, *mut ANativeWindow),
    pub on_input_queue_created: unsafe extern "C" fn(*mut ANativeActivity, *mut core::ffi::c_void),
    pub on_input_queue_destroyed:
        unsafe extern "C" fn(*mut ANativeActivity, *mut core::ffi::c_void),
    pub on_configuration_changed: unsafe extern "C" fn(*mut ANativeActivity),
    pub on_low_memory: unsafe extern "C" fn(*mut ANativeActivity),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// NULL 指针必须被拒绝。
    #[test]
    fn null_rejected() {
        assert!(Activity::from_raw(core::ptr::null_mut()).is_none());
        assert!(CallbackRegistry::new(core::ptr::null_mut()).is_none());
    }

    /// 借用构造必须原样保持指针。
    #[test]
    fn from_raw_keeps_pointer() {
        let fake = 0x2000usize as *mut ANativeActivity;
        let a = Activity::from_raw(fake).unwrap();
        assert_eq!(a.raw, fake);
    }
}
