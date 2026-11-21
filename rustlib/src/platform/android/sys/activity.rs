//! `android/native_activity.h` 的手写绑定（NDK 27.3.13750724）。
//!
//! # 字段顺序约束
//! `ANativeActivityCallbacks` 的字段顺序必须与头文件完全一致（PRD §6.4.3）。
//! 任何字段缺失或错位都会使其后的回调错位，表现为随机崩溃而非编译失败，
//! 因此本模块的布局断言测试是强制门禁。

use core::ffi::{c_char, c_int, c_void};
use std::ffi::c_uint;

use super::rect::ARect;
use super::window::ANativeWindow;

/// `android/native_activity.h` 的 `ANativeActivity`。
///
/// # 所有权
/// 由框架构造并借给原生侧；Rust 侧永不构造实例，只持有指针。
/// `env` 只能在主线程使用（框架约束，非本仓库附加）。
#[repr(C)]
pub struct ANativeActivity {
    /// 回调函数表指针；由框架分配，原生侧只改字段不改指针本身。
    pub callbacks: *mut ANativeActivityCallbacks,
    /// 进程级 JavaVM 句柄（借用）。
    pub vm: *mut c_void,
    /// 主线程 JNI 上下文（借用，仅主线程可用）。
    pub env: *mut c_void,
    /// NativeActivity 实例引用（借用；头文件注明命名历史误用 clazz）。
    pub clazz: *mut c_void,
    /// 应用内部数据目录路径（借用，可能为 NULL）。
    pub internal_data_path: *const c_char,
    /// 应用外部数据目录路径（借用，可能为 NULL）。
    pub external_data_path: *const c_char,
    /// 平台 SDK 版本号（int32_t）。
    pub sdk_version: i32,
    /// 应用自定义实例状态指针；框架不使用，由原生侧自行管理所有权。
    pub instance: *mut c_void,
    /// AssetManager 句柄（借用），资产（字体/图片）由此取得。
    pub asset_manager: *mut c_void,
    /// OBB 文件目录路径（借用，可能为 NULL）。
    pub obb_path: *const c_char,
}

/// `ANativeActivityCallbacks`：框架回调表。
///
/// # 不变量
/// - 字段顺序与 NDK 头文件逐一对应（16 个函数指针，严格顺序）；
/// - 默认全 NULL，框架按非 NULL 者回调；
/// - 全部回调发生在应用主线程。
#[repr(C)]
pub struct ANativeActivityCallbacks {
    pub on_start: Option<unsafe extern "C" fn(*mut ANativeActivity)>,
    pub on_resume: Option<unsafe extern "C" fn(*mut ANativeActivity)>,
    pub on_save_instance_state:
        Option<unsafe extern "C" fn(*mut ANativeActivity, *mut usize) -> *mut c_void>,
    pub on_pause: Option<unsafe extern "C" fn(*mut ANativeActivity)>,
    pub on_stop: Option<unsafe extern "C" fn(*mut ANativeActivity)>,
    pub on_destroy: Option<unsafe extern "C" fn(*mut ANativeActivity)>,
    pub on_window_focus_changed: Option<unsafe extern "C" fn(*mut ANativeActivity, c_int)>,
    pub on_native_window_created:
        Option<unsafe extern "C" fn(*mut ANativeActivity, *mut ANativeWindow)>,
    pub on_native_window_resized:
        Option<unsafe extern "C" fn(*mut ANativeActivity, *mut ANativeWindow)>,
    pub on_native_window_redraw_needed:
        Option<unsafe extern "C" fn(*mut ANativeActivity, *mut ANativeWindow)>,
    pub on_native_window_destroyed:
        Option<unsafe extern "C" fn(*mut ANativeActivity, *mut ANativeWindow)>,
    pub on_input_queue_created: Option<unsafe extern "C" fn(*mut ANativeActivity, *mut c_void)>,
    pub on_input_queue_destroyed: Option<unsafe extern "C" fn(*mut ANativeActivity, *mut c_void)>,
    pub on_content_rect_changed: Option<unsafe extern "C" fn(*mut ANativeActivity, *const ARect)>,
    pub on_configuration_changed: Option<unsafe extern "C" fn(*mut ANativeActivity)>,
    pub on_low_memory: Option<unsafe extern "C" fn(*mut ANativeActivity)>,
}

// ANativeActivity_finish：请求结束 Activity（任何线程可调）；
// 其余生命周期控制符号（setWindowFormat/setWindowFlags/showSoftInput/hideSoftInput）
// 语义见 NDK native_activity.h。
#[cfg(target_os = "android")]
unsafe extern "C" {
    pub fn ANativeActivity_finish(activity: *mut ANativeActivity);
    pub fn ANativeActivity_setWindowFormat(activity: *mut ANativeActivity, format: i32);
    pub fn ANativeActivity_setWindowFlags(
        activity: *mut ANativeActivity,
        add_flags: u32,
        remove_flags: u32,
    );
    pub fn ANativeActivity_showSoftInput(activity: *mut ANativeActivity, flags: u32);
    pub fn ANativeActivity_hideSoftInput(activity: *mut ANativeActivity, flags: u32);
}

/// `ANATIVEACTIVITY_SHOW_SOFT_INPUT_*` 常量（头文件 enum）。
pub const ANATIVEACTIVITY_SHOW_SOFT_INPUT_IMPLICIT: c_uint = 0x0001;
pub const ANATIVEACTIVITY_SHOW_SOFT_INPUT_FORCED: c_uint = 0x0002;
/// `ANATIVEACTIVITY_HIDE_SOFT_INPUT_*` 常量（头文件 enum）。
pub const ANATIVEACTIVITY_HIDE_SOFT_INPUT_IMPLICIT_ONLY: c_uint = 0x0001;
pub const ANATIVEACTIVITY_HIDE_SOFT_INPUT_NOT_ALWAYS: c_uint = 0x0002;

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, size_of};

    /// 回调表必须是 16 个函数指针、无填充（64 位上 16×8 = 128 字节）。
    /// 字段缺失/错位属于不可接受的错误（PRD §6.4.3 第 3 条）。
    #[test]
    fn callbacks_layout_matches_ndk() {
        assert_eq!(
            size_of::<ANativeActivityCallbacks>(),
            16 * size_of::<usize>()
        );
        assert_eq!(align_of::<ANativeActivityCallbacks>(), size_of::<usize>());
    }

    /// ANativeActivity 布局：10 个指针字段 + sdkVersion(i32)。
    /// sdkVersion 之后有 4 字节填充（对齐到指针宽度），
    /// 故总大小 = 10×8 = 80 字节，而非 11×8。
    #[test]
    fn activity_layout_matches_ndk() {
        assert_eq!(size_of::<ANativeActivity>(), 80);
        assert_eq!(align_of::<ANativeActivity>(), size_of::<usize>());
    }

    /// 常量与头文件逐一对照。
    #[test]
    fn soft_input_constants_match_ndk() {
        assert_eq!(ANATIVEACTIVITY_SHOW_SOFT_INPUT_IMPLICIT, 0x0001);
        assert_eq!(ANATIVEACTIVITY_SHOW_SOFT_INPUT_FORCED, 0x0002);
        assert_eq!(ANATIVEACTIVITY_HIDE_SOFT_INPUT_IMPLICIT_ONLY, 0x0001);
        assert_eq!(ANATIVEACTIVITY_HIDE_SOFT_INPUT_NOT_ALWAYS, 0x0002);
    }
}
