//! NativeActivity 入口装配（glue）：`ANativeActivity_onCreate`、回调注册、
//! ALooper 主循环、首帧清屏绘制（PRD §8.3 启动链路）。
//!
//! # 架构（对齐 android_native_app_glue 的线程模型）
//! - 框架在专用主线程回调 `onCreate`/`onNativeWindowCreated` 等；
//! - `onCreate` 拉起**主循环线程**：prepare 自己的 ALooper，负责窗口首帧
//!   渲染与输入泵取；
//! - 跨线程状态经 `Shared`（Mutex）交接，回调侧置位后 `ALooper_wake` 唤醒；
//! - `onDestroy` 置退出标记、唤醒并 join 主循环线程。
//!
//! # 不变量
//! - 窗口引用计数：created 回调 acquire（回调外的跨回调持有必须 acquire）；
//!   主循环线程取用时再 acquire、窗口销毁或退出时 release。
//! - `activity.instance` 挂载 `Arc<Mutex<Shared>>`（`Arc::into_raw`），
//!   仅 `onDestroy` 中对称还原销毁。
//! - 输入事件不跨轮持有：泵取循环内借出即 finish。

use std::sync::{Arc, Mutex};

use log::{debug, info, warn};

use super::input::{EventKind, InputQueue as SafeInputQueue};
use super::log::AndroidLogger;
use super::sys::activity::{ANativeActivity, ANativeActivityCallbacks};
use super::sys::input::AInputQueue;
use super::sys::looper::{
    ALOOPER_EVENT_INPUT, ALOOPER_POLL_CALLBACK, ALOOPER_POLL_ERROR, ALooper, ALooper_addFd,
};
use super::sys::window::ANativeWindow;
use super::window::Window;
use crate::backends::select::select_backend;
use crate::backends::{AnyBackend, Backend};

/// 主循环上输入队列的 looper 标识（自定义值，非 NDK 常量）。
const LOOPER_ID_INPUT: i32 = 1;
/// 唤醒 fd 的 looper 标识（自定义值，非 NDK 常量）。
const LOOPER_ID_WAKE: i32 = 2;

/// 上一帧的 IME 遮挡高度（用于检测键盘弹出/收起的布局变化）。
static LAST_IME_INSET: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// 上次推送无障碍快照时的页树代数（去重，避免每帧重推）。
static LAST_A11Y_GEN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(u32::MAX);

/// 状态保存交接槽（主循环线程写 JSON，onSaveInstanceState 回调线程读走）。
///
/// # 线程模型
/// 主循环每轮把当前状态序列化后覆盖写入；回调线程原子 take。
/// 任何时刻只有一方持有内容，无需额外同步。
static SAVED_STATE_SLOT: Mutex<Option<String>> = Mutex::new(None);

/// 首帧清屏颜色（深灰，肉眼可辨于黑屏/未绘制）。
/// V1 首帧场景由 `demo_scene` 提供，此常量保留给后续错误路径 fallback。

/// 跨线程交接的原生指针包装：仅在交接语境中安全。
///
/// # 安全
/// 指针在 created 回调 acquire 后才放入，在目标线程取用时再次 acquire；
/// 纯指针搬家（不解引用）满足 `Send`。
#[derive(Clone, Copy)]
struct SendPtr<T>(*mut T);
unsafe impl<T> Send for SendPtr<T> {}
impl<T> PartialEq for SendPtr<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl<T> SendPtr<T> {
    fn as_ptr(self) -> *mut T {
        self.0
    }
}

/// 回调线程与主循环线程的交接状态。
struct Shared {
    /// 待挂接的新窗口（created 回调已 acquire，等待主循环线程取用）。
    window_new: Option<SendPtr<ANativeWindow>>,
    /// 窗口已销毁（框架侧引用已 release，主循环线程必须立即丢弃借用）。
    window_gone: bool,
    /// 待 attach 的新输入队列。
    queue_new: Option<SendPtr<AInputQueue>>,
    /// 输入队列已销毁（主循环线程必须停止泵取并丢弃）。
    queue_gone: bool,
    /// 队列已由主循环 detachLooper（回调线程据此结束等待）。
    queue_detached: bool,
    /// detach 完成信号（回调线程在框架销毁队列前等待，避免跨线程竞态）。
    detach_done: std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
    /// 窗口释放完成信号（后端 drop + release 后才允许框架销毁窗口）。
    window_done: std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
    /// 窗口尺寸变化请求（resized 回调写入；主循环消费后重建 surface）。
    window_resized: Option<(u32, u32)>,
    /// 窗口失焦（键盘强收/分屏切换）：主循环据此合成 ACTION_CANCEL。
    focus_lost: bool,
    /// 表面需重绘（redrawNeeded/内容区变化）：主循环整帧重绘。
    redraw_pending: bool,
    /// 请求主循环线程退出（onDestroy 置位）。
    quit: bool,
    /// 唤醒通道：回调侧 wake，主循环侧 drain（onCreate 创建，进程生命周期存活）。
    wake: Option<std::sync::Arc<WakePipe>>,
    /// 主循环线程句柄（onDestroy join 用）。
    loop_thread: Option<std::thread::JoinHandle<()>>,
    /// Activity 指针（onCreate 记录；主循环线程 BACK 事件 finish 用）。
    ///
    /// # 安全
    /// 原生活动生命周期覆盖主循环线程存活期（onDestroy join 在后），
    /// 因此从回调线程移交到主循环线程裸读是安全的。
    activity: SendPtr<ANativeActivity>,
    /// Activity 重建时框架透传的已保存状态（主循环构造 Demo 前消费）。
    saved_state: Option<crate::ui::state::SavedState>,
}

/// 框架入口：`ANativeActivity_onCreate`（PRD §6.4.1 入口符号）。
///
/// # 安全
/// 由框架以正确参数调用；`#[unsafe(no_mangle)]` 保证动态符号表可见。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ANativeActivity_onCreate(
    activity: *mut ANativeActivity,
    saved_state: *mut core::ffi::c_void,
    saved_state_size: usize,
) {
    // 启动链路第 2 步：先装日志后端，后续所有路径可观测。
    AndroidLogger::install();
    // A1 冷启动计时起点（日志后端就绪后第一件事）。
    crate::platform::perf::mark_boot_start();
    info!("NativeActivity onCreate");

    // SAFETY: callbacks 由框架分配且非空（框架协议）；写回调表签名与 NDK
    // 头文件逐一一致。
    let callbacks = unsafe { (*activity).callbacks };
    if !callbacks.is_null() {
        unsafe { register_callbacks(callbacks) };
    } else {
        warn!("NativeActivity callbacks table is null");
    }

    // 唤醒通道：先建 eventfd，回调侧即刻可安全 wake（主循环 attach 前
    // 的唤醒只会堆积计数，attach 后第一次 poll 即返回，无丢失）。
    let wake = match WakePipe::new() {
        Some(w) => Arc::new(w),
        None => {
            warn!("failed to create wake pipe, falling back to polling mode");
            Arc::new(WakePipe::none())
        }
    };
    // 记录 fd 供 JNI 侧（IME 桥）跨线程唤醒主循环。
    WAKE_FD.store(wake.fd as i32, std::sync::atomic::Ordering::Release);

    // Activity 重建恢复：解析框架透传的 UTF-8 JSON 载荷（PRD §5.1.9
    // 状态保存子集）。失败按冷启动处理。
    let restored = if !saved_state.is_null() && saved_state_size > 0 {
        // SAFETY: 框架传入的缓冲在回调期间有效；长度由框架保证。
        let bytes =
            unsafe { std::slice::from_raw_parts(saved_state as *const u8, saved_state_size) };
        match std::str::from_utf8(bytes) {
            Ok(json) => crate::ui::state::SavedState::from_json(json),
            Err(e) => {
                warn!("saved state not utf-8, cold start: {e}");
                None
            }
        }
    } else {
        None
    };
    if let Some(state) = &restored {
        info!("state restored: {} entries", state.len());
    }

    let shared: Arc<Mutex<Shared>> = Arc::new(Mutex::new(Shared {
        window_new: None,
        window_gone: false,
        queue_new: None,
        queue_gone: false,
        queue_detached: false,
        detach_done: std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new())),
        window_done: std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new())),
        window_resized: None,
        focus_lost: false,
        redraw_pending: false,
        quit: false,
        wake: Some(wake),
        loop_thread: None,
        activity: SendPtr(activity),
        /// onCreate 重建场景的状态载荷（主循环首帧前消费一次）。
        saved_state: restored,
    }));

    // 主循环线程：拉起失败时降级为空 activity（记录日志并请求结束）。
    let spawn_shared = shared.clone();
    let thread = std::thread::Builder::new()
        .name("velloview-main".into())
        .spawn(move || main_loop(spawn_shared));
    match thread {
        Ok(handle) => {
            shared
                .lock()
                .expect("shared state lock poisoned")
                .loop_thread = Some(handle);
        }
        Err(e) => {
            warn!("failed to spawn main loop thread: {e}");
            // SAFETY: activity 借用在本回调内有效。
            unsafe { super::sys::activity::ANativeActivity_finish(activity) };
        }
    }

    // SAFETY: instance 字段归原生侧自由使用（头文件契约）；此处挂载
    // Arc（唯一所有权移交 activity，onDestroy 对称还原）。
    unsafe { (*activity).instance = Arc::into_raw(shared) as *mut _ };

    // 启动链路第 2.5 步：预加载字体（Task 12 验证：mmap + fd 生命周期）。
    // 失败不阻断启动：文字功能在 Task 13 才接入渲染，此处仅验证加载链路。
    load_default_font(activity);

    // 导出等落盘功能需要私有目录：onCreate 时记录 internalDataPath。
    init_app_paths(activity);
}

/// 记录应用私有目录（`files`），供导出链路使用。
///
/// # 失败语义
/// `internal_data_path` 为空时打 warn，导出回退到临时目录（仅桌面测试会走到）。
#[cfg(target_os = "android")]
fn init_app_paths(activity: *mut ANativeActivity) {
    use std::ffi::CStr;
    // SAFETY: activity 存活期间 internal_data_path 有效（头文件契约）。
    let raw = unsafe { (*activity).internal_data_path };
    if raw.is_null() {
        warn!("internal data path unavailable, exports fall back to temp dir");
        return;
    }
    match unsafe { CStr::from_ptr(raw) }.to_str() {
        Ok(path) => {
            info!("app files dir resolved: {}", path);
            super::app_paths::set_files_dir(std::path::PathBuf::from(path));
        }
        Err(_) => warn!("internal data path is not valid utf-8"),
    }
}

/// 加载默认字体并注入进程缓存（真机验证锚点 + Demo 页排版用）。
///
/// # 失败语义
/// 打开/fd/mmap 全链失败时打 warn；进程仍可启动（Demo 页降级为空白文本）。
/// 字体数据按进程生命周期持有（全进程一份拷贝）。
#[cfg(target_os = "android")]
fn load_default_font(activity: *mut ANativeActivity) {
    use crate::text::font::load_from_assets;
    // SAFETY: activity 存活期间 asset_manager 有效（头文件契约）。
    let mgr_raw = unsafe { (*activity).asset_manager } as *mut _;
    let Some(mgr) = crate::platform::android::asset::AssetManager::from_raw(mgr_raw) else {
        warn!("asset manager unavailable, font not loaded");
        return;
    };
    match load_from_assets(&mgr, "fonts/NotoSerifSC-Regular.ttf") {
        Some(font) => {
            info!(
                "default font ready: {} bytes, hash {}, source {}",
                font.bytes().len(),
                font.hash8(),
                font.source()
            );
            // 注入进程字体缓存（Demo 页 TextContext 与渲染字形共用同一份）。
            crate::text::font::set_process_font(font);
        }
        None => warn!("default font load failed, text features degraded"),
    }
}

/// 注册回调表（V1 最小集；未注册项保持 NULL，框架按 NULL 跳过）。
unsafe fn register_callbacks(callbacks: *mut ANativeActivityCallbacks) {
    // SAFETY: 各回调签名与 NDK 头文件声明一致；写字段本身安全。
    unsafe {
        (*callbacks).on_start = Some(on_start);
        (*callbacks).on_resume = Some(on_resume);
        (*callbacks).on_pause = Some(on_pause);
        (*callbacks).on_stop = Some(on_stop);
        (*callbacks).on_save_instance_state = Some(on_save_instance_state);
        (*callbacks).on_destroy = Some(on_destroy);
        (*callbacks).on_window_focus_changed = Some(on_window_focus_changed);
        (*callbacks).on_native_window_created = Some(on_native_window_created);
        (*callbacks).on_native_window_resized = Some(on_native_window_resized);
        (*callbacks).on_native_window_redraw_needed = Some(on_native_window_redraw_needed);
        (*callbacks).on_content_rect_changed = Some(on_content_rect_changed);
        (*callbacks).on_native_window_destroyed = Some(on_native_window_destroyed);
        (*callbacks).on_input_queue_created = Some(on_input_queue_created);
        (*callbacks).on_input_queue_destroyed = Some(on_input_queue_destroyed);
        (*callbacks).on_configuration_changed = Some(on_configuration_changed);
        (*callbacks).on_low_memory = Some(on_low_memory);
    }
}

/// 从 `activity.instance` 借出共享状态的引用（计数 +1，不夺取所有权）。
///
/// # 安全
/// 仅在 onCreate 成功挂载 instance 后调用。
unsafe fn shared_of(activity: *mut ANativeActivity) -> Option<Arc<Mutex<Shared>>> {
    // 空指针防护：onDestroy 会先清空 instance，之后到达的迟到回调
    //（如 focus/window 回调）不得再解引用；`Arc::from_raw(null)` 是 UB。
    if activity.is_null() {
        return None;
    }
    // SAFETY: instance 挂载于 onCreate；清空前回调序保证非空。
    let raw = unsafe { (*activity).instance } as *const Mutex<Shared>;
    if raw.is_null() {
        return None;
    }
    // SAFETY: +1 与 from_raw 配对，形成合法借用句柄（onDestroy 负责
    // 最终的 -1 释放原始挂载引用）。
    unsafe { Arc::increment_strong_count(raw) };
    // SAFETY: 与 increment 配对。
    Some(unsafe { Arc::from_raw(raw) })
}

/// 跨线程唤醒通道：eventfd 供主循环线程 poll。
///
/// # 设计
/// 回调线程无法直接 `ALooper_wake` 主循环线程的 looper（forThread 拿不到
/// 对方线程的 looper），因此用 eventfd 注册进主循环 looper（ALOOPER_EVENT_INPUT），
/// 回调侧写 1 字节唤醒，主循环侧读干（保留 level-triggered 语义）。
struct WakePipe {
    fd: i32,
}

/// 唤醒 eventfd 的全局副本（-1 = 不可用）；供 IME 桥等跨线程路径唤醒主循环。
static WAKE_FD: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

/// 跨线程唤醒主循环（任何线程可调用；无唤醒通道时静默无效）。
pub fn wake_main_loop() {
    let fd = WAKE_FD.load(std::sync::atomic::Ordering::Acquire);
    if fd < 0 {
        return;
    }
    // SAFETY: fd 由主循环创建且进程生命周期存活；非阻塞写入。
    unsafe {
        let one: u64 = 1;
        let _ = libc::write(fd, &one as *const _ as *const core::ffi::c_void, 8);
    }
}

impl WakePipe {
    /// 创建 eventfd（不注册 looper；注册在主循环线程 prepare 后进行）。
    fn new() -> Option<Self> {
        // SAFETY: syscall 参数合法；-1 表示失败。
        let fd = unsafe { eventfd(0, EFD_CLOEXEC | EFD_NONBLOCK) };
        if fd < 0 { None } else { Some(Self { fd }) }
    }

    /// 无唤醒通道的降级形态（eventfd 创建失败时使用，主循环纯轮询）。
    fn none() -> Self {
        Self { fd: -1 }
    }

    /// 注册到主循环线程的 looper（在 looper prepare 之后调用）。
    ///
    /// # 失败语义
    /// 失败时返回 false，主循环退化为纯轮询模式（正确性不受影响）。
    fn attach(&self, looper: *mut ALooper) -> bool {
        if self.fd < 0 {
            return false;
        }
        // SAFETY: fd 有效；ident 传自定义值供 pollOnce 返回识别。
        let r = unsafe {
            ALooper_addFd(
                looper,
                self.fd,
                LOOPER_ID_WAKE,
                ALOOPER_EVENT_INPUT,
                None,
                core::ptr::null_mut(),
            )
        };
        r >= 0
    }

    /// 回调线程侧：写入 8 字节唤醒主循环 poll。
    fn wake(&self) {
        if self.fd < 0 {
            return;
        }
        // SAFETY: fd 有效且非阻塞；EAGAIN 视作已唤醒，静默忽略。
        unsafe {
            let one: u64 = 1;
            let _ = libc::write(self.fd, &one as *const _ as *const core::ffi::c_void, 8);
        }
    }

    /// 主循环线程侧：读干计数（level-triggered 必须清零，否则 poll 恒就绪空转）。
    fn drain(&self) {
        if self.fd < 0 {
            return;
        }
        // SAFETY: fd 有效且非阻塞；读 0 或 EAGAIN 均为正常。
        unsafe {
            let mut buf: u64 = 0;
            let _ = libc::read(self.fd, &mut buf as *mut _ as *mut core::ffi::c_void, 8);
        }
    }
}

impl Drop for WakePipe {
    fn drop(&mut self) {
        if self.fd >= 0 {
            // SAFETY: fd 由本类型独占拥有。
            unsafe { libc::close(self.fd) };
        }
    }
}

/// `eventfd(2)` 标志（Linux UAPI，非 NDK）。
const EFD_CLOEXEC: i32 = 0o2000000;
const EFD_NONBLOCK: i32 = 0o4000;

unsafe extern "C" {
    fn eventfd(initval: u32, flags: i32) -> i32;
}

// ---- 生命周期回调（框架主线程，只做交接 + 唤醒，不做重活） ----

unsafe extern "C" fn on_start(activity: *mut ANativeActivity) {
    debug!("onStart");
    let _ = unsafe { shared_of(activity) };
}

unsafe extern "C" fn on_resume(activity: *mut ANativeActivity) {
    debug!("onResume");
    let _ = unsafe { shared_of(activity) };
}

unsafe extern "C" fn on_pause(activity: *mut ANativeActivity) {
    debug!("onPause");
    let _ = unsafe { shared_of(activity) };
}

unsafe extern "C" fn on_stop(activity: *mut ANativeActivity) {
    debug!("onStop");
    let _ = unsafe { shared_of(activity) };
}

/// 状态保存：向框架返回 malloc 分配的 JSON 缓冲（框架负责 free）。
///
/// # 契约（NDK `native_activity.h`）
/// 返回的指针必须由 `malloc` 分配；`*out_size` 写入字节数。
/// 返回 NULL = 无状态保存。
///
/// # 数据流
/// 主循环线程持有 Demo 状态：经全局槽（`Mutex<Option<String>>`）
/// 交接，主循环下一轮填充；为不阻塞回调线程，此回调直接读槽，
/// 槽空时（首存/尚未填充）返回 NULL，框架按无状态处理。
unsafe extern "C" fn on_save_instance_state(
    activity: *mut ANativeActivity,
    out_size: *mut usize,
) -> *mut core::ffi::c_void {
    debug!("onSaveInstanceState");
    if out_size.is_null() {
        return core::ptr::null_mut();
    }
    let Some(_shared) = (unsafe { shared_of(activity) }) else {
        return core::ptr::null_mut();
    };
    // SAFETY: 状态槽由主循环线程写入、本回调线程在此读后清空；
    // 指针解引用前已由 Mutex 保护；毒锁（主循环 panic 时）按无状态处理。
    let payload = match unsafe { SAVED_STATE_SLOT.lock() } {
        Ok(mut guard) => guard.take(),
        Err(poisoned) => {
            debug!("state slot poisoned, treating as no state");
            poisoned.into_inner().take()
        }
    };
    let Some(json) = payload else {
        debug!("no state saved yet (main loop has not filled the slot)");
        return core::ptr::null_mut();
    };
    let bytes = json.as_bytes();
    // SAFETY: malloc/free 配对由框架负责（头文件契约）；此处只负责分配。
    let buf = unsafe { libc::malloc(bytes.len()) };
    if buf.is_null() {
        warn!("malloc failed for saved state");
        return core::ptr::null_mut();
    }
    // SAFETY: buf 由刚分配的 len 字节，拷贝来源有效。
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf as *mut u8, bytes.len()) };
    unsafe { *out_size = bytes.len() };
    info!("state saved: {} bytes", bytes.len());
    buf
}

unsafe extern "C" fn on_window_focus_changed(
    activity: *mut ANativeActivity,
    has_focus: core::ffi::c_int,
) {
    debug!("onWindowFocusChanged: {}", has_focus != 0);
    let Some(shared) = (unsafe { shared_of(activity) }) else {
        return;
    };
    {
        let mut st = shared.lock().expect("shared state lock poisoned");
        // 失焦时系统可能强收键盘/打断手势：置标记让主循环合成 CANCEL。
        st.focus_lost = has_focus == 0;
    }
    wake(&shared);
}

unsafe extern "C" fn on_configuration_changed(activity: *mut ANativeActivity) {
    debug!("onConfigurationChanged");
    let _ = unsafe { shared_of(activity) };
}

unsafe extern "C" fn on_low_memory(activity: *mut ANativeActivity) {
    warn!("onLowMemory");
    let _ = unsafe { shared_of(activity) };
}

/// 窗口创建：acquire 框架引用 → 交接 → 唤醒主循环线程渲染首帧。
unsafe extern "C" fn on_native_window_created(
    activity: *mut ANativeActivity,
    window: *mut ANativeWindow,
) {
    info!("onNativeWindowCreated");
    let Some(shared) = (unsafe { shared_of(activity) }) else {
        return;
    };
    // SAFETY: 回调内窗口必有效；acquire 使主循环线程可安全跨回调借用。
    unsafe { super::sys::window::ANativeWindow_acquire(window) };
    {
        let mut st = shared.lock().expect("shared state lock poisoned");
        st.window_new = Some(SendPtr(window));
        st.window_gone = false;
    }
    wake(&shared);
}

unsafe extern "C" fn on_native_window_resized(
    activity: *mut ANativeActivity,
    window: *mut ANativeWindow,
) {
    debug!("onNativeWindowResized");
    let Some(shared) = (unsafe { shared_of(activity) }) else {
        return;
    };
    let (w, h) = unsafe {
        (
            super::sys::window::ANativeWindow_getWidth(window).max(1) as u32,
            super::sys::window::ANativeWindow_getHeight(window).max(1) as u32,
        )
    };
    {
        let mut st = shared.lock().expect("shared state lock poisoned");
        st.window_resized = Some((w, h));
    }
    wake(&shared);
}

/// 表面内容丢失（上下文丢失/首帧前重绘请求）：唤醒主循环重绘整帧。
unsafe extern "C" fn on_native_window_redraw_needed(
    activity: *mut ANativeActivity,
    _window: *mut ANativeWindow,
) {
    debug!("onNativeWindowRedrawNeeded");
    let Some(shared) = (unsafe { shared_of(activity) }) else {
        return;
    };
    {
        let mut st = shared.lock().expect("shared state lock poisoned");
        st.redraw_pending = true;
    }
    wake(&shared);
}

/// 内容区变化（edge-to-edge 安全区）：Kotlin 已注入 insets，这里仅触发重绘。
unsafe extern "C" fn on_content_rect_changed(
    activity: *mut ANativeActivity,
    _rect: *const super::sys::rect::ARect,
) {
    debug!("onContentRectChanged");
    let Some(shared) = (unsafe { shared_of(activity) }) else {
        return;
    };
    {
        let mut st = shared.lock().expect("shared state lock poisoned");
        st.redraw_pending = true;
    }
    wake(&shared);
}

/// 窗口销毁：框架侧 release；主循环线程的借用由其自身的 acquire/release 平衡。
unsafe extern "C" fn on_native_window_destroyed(
    activity: *mut ANativeActivity,
    window: *mut ANativeWindow,
) {
    info!("onNativeWindowDestroyed");
    let Some(shared) = (unsafe { shared_of(activity) }) else {
        return;
    };
    let window_done = {
        let mut st = shared.lock().expect("shared state lock poisoned");
        // 若窗口尚未被主循环线程取走，清掉交接槽并在此 release（配对 created）。
        if st.window_new == Some(SendPtr(window)) {
            st.window_new = None;
            // SAFETY: 与 created 回调的 acquire 配对。
            unsafe { super::sys::window::ANativeWindow_release(window) };
        }
        st.window_gone = true;
        st.window_done.clone()
    };
    wake(&shared);
    // 后端可能仍在该窗口上渲染：等主循环 drop backend 并 release 后再返回，
    // 否则框架立刻销毁窗口，渲染线程继续写已释放的 ANativeWindow
    //（堆损坏，常在后续 InputQueue 析构处表现为 Scudo abort）。
    let (lock, cv) = &*window_done;
    let mut done = lock.lock().expect("window flag poisoned");
    // 同上：3s 足够软件光栅化收尾，同时避免 destroy 长时间阻塞。
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(3_000);
    while !*done {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            warn!("native window release timeout; proceeding");
            break;
        }
        let (guard, _) = cv.wait_timeout(done, left).expect("window wait poisoned");
        done = guard;
    }
}

unsafe extern "C" fn on_input_queue_created(
    activity: *mut ANativeActivity,
    queue: *mut core::ffi::c_void,
) {
    info!("onInputQueueCreated");
    let Some(shared) = (unsafe { shared_of(activity) }) else {
        return;
    };
    {
        let mut st = shared.lock().expect("shared state lock poisoned");
        st.queue_new = Some(SendPtr(queue as *mut AInputQueue));
    }
    wake(&shared);
}

unsafe extern "C" fn on_input_queue_destroyed(
    activity: *mut ANativeActivity,
    queue: *mut core::ffi::c_void,
) {
    info!("onInputQueueDestroyed");
    let _ = queue;
    let Some(shared) = (unsafe { shared_of(activity) }) else {
        return;
    };
    let detach_done = {
        let mut st = shared.lock().expect("shared state lock poisoned");
        st.queue_gone = true;
        st.detach_done.clone()
    };
    wake(&shared);
    // 必须在框架销毁队列前完成 detachLooper：等待主循环线程（唯一
    // attach 方）同步摘除。跨线程直接 detach 与 looper 内部的 poll
    // 存在竞态，曾触发 Scudo "invalid chunk state"（~InputQueue 崩溃）。
    let (lock, cv) = &*detach_done;
    let mut done = lock.lock().expect("detach flag poisoned");
    // 上界 3s：模拟器软件光栅化单帧约 250ms，留足收尾余量即可；
    // 过长会阻塞下一次 Activity 启动（测试/重建场景表现为卡顿）。
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(3_000);
    while !*done {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            warn!("input queue detach timeout; proceeding");
            break;
        }
        let (guard, _) = cv.wait_timeout(done, left).expect("detach wait poisoned");
        done = guard;
    }
}

/// 销毁：置退出标记 → 唤醒 → join 主循环线程 → 归还 Arc。
unsafe extern "C" fn on_destroy(activity: *mut ANativeActivity) {
    info!("onDestroy");
    if activity.is_null() {
        return;
    }
    // SAFETY: instance 由 onCreate 挂载（into_raw，计数 1）。
    let raw = unsafe { (*activity).instance } as *mut Mutex<Shared>;
    // 幂等：重复 destroy（框架异常路径/二次回调）时 instance 已清空，
    // 不能再 `Arc::from_raw(null)`（UB，会写坏相邻堆块）。
    if raw.is_null() {
        warn!("onDestroy without live instance; ignored");
        return;
    }
    // SAFETY: 直接接管 into_raw 移交的那一份强计数（不再额外 +1，
    // 否则原始引用永不回收，每次 Activity 重建泄漏 Shared + eventfd）。
    // 主循环线程自身持有另一份 clone，函数结束 drop 后仍至少为 1。
    let shared: Arc<Mutex<Shared>> = unsafe { Arc::from_raw(raw) };
    // 清空 instance，防止后续回调把已释放指针当成有效 Arc。
    unsafe { (*activity).instance = core::ptr::null_mut() };
    let handle = {
        let mut st = shared.lock().expect("shared state lock poisoned");
        st.quit = true;
        st.loop_thread.take()
    };
    wake(&shared);
    if let Some(handle) = handle {
        let _ = handle.join();
    }
}

/// 回调线程侧唤醒主循环：向 eventfd 写 8 字节。
///
/// # 注意
/// 唤醒不携带语义；主循环每轮开头全量同步交接状态，
/// 因此唤醒重复/堆积均无正确性影响。
fn wake(shared: &Arc<Mutex<Shared>>) {
    let st = shared.lock().expect("shared state lock poisoned");
    if let Some(w) = st.wake.as_ref() {
        w.wake();
    }
}

// ---- 主循环线程 ----

/// 主循环线程入口：poll 等待 → 同步交接状态 → 渲染/泵取。
fn main_loop(shared: Arc<Mutex<Shared>>) {
    // SAFETY: 在本线程首次 prepare，返回本线程专属 looper。
    let looper = unsafe {
        super::sys::looper::ALooper_prepare(super::sys::looper::ALOOPER_PREPARE_ALLOW_NON_CALLBACKS)
    };
    if looper.is_null() {
        warn!("failed to prepare looper, main loop exits");
        return;
    }
    // 唤醒 fd 注册进本线程 looper（pollOnce 将以 LOOPER_ID_WAKE 返回）。
    let wake = shared
        .lock()
        .expect("shared state lock poisoned")
        .wake
        .clone();
    if let Some(w) = wake.as_deref() {
        if !w.attach(looper) {
            warn!("failed to attach wake fd, falling back to polling");
        }
    }
    info!("main loop started");
    let mut current_window: Option<Window> = None;
    // 当前后端与窗口同生命周期：created 时构造、destroyed 前 drop。
    let mut _current_backend: Option<AnyBackend> = None;
    let mut current_queue: Option<SafeInputQueue> = None;
    // 三页 Demo 宿主（Task 20）：窗口尺寸在首次 window created 时注入。
    let mut demo: Option<crate::demo::Demo> = None;
    // A2 帧率统计（滚动/动画期间持续输出 P50/P95 汇总）。
    let mut frame_stats = crate::platform::perf::FrameStats::new();
    // FPS 叠加层（环境变量 NATIVEVIEW_FPS_HUD=1 门控，默认关闭）。
    let mut fps_hud = crate::platform::perf::FpsHud::new();
    // A3 RSS 采样间隔（静态页稳定后周期采样；滚动/动画时不采样，
    // 避免 statm 读文件抖动混入帧时长）。
    let mut last_rss_sample = std::time::Instant::now();

    loop {
        // 1) 同步交接状态（每轮全量拉取，唤醒丢失无正确性影响）。
        let (
            take_window,
            window_gone,
            take_queue,
            queue_gone,
            queue_detached,
            _detach_done,
            window_done_ack,
            quit,
            resized,
            focus_lost,
            redraw,
        ) = {
            let mut st = shared.lock().expect("shared state lock poisoned");
            let take_window = st.window_new.take();
            let window_gone = st.window_gone;
            st.window_gone = false;
            let take_queue = st.queue_new.take();
            let queue_gone = st.queue_gone;
            st.queue_gone = false;
            let queue_detached = st.queue_detached;
            let _detach_done = st.detach_done.clone();
            let window_done_ack = st.window_done.clone();
            let resized = st.window_resized.take();
            let focus_lost = st.focus_lost;
            st.focus_lost = false;
            let redraw = st.redraw_pending;
            st.redraw_pending = false;
            (
                take_window,
                window_gone,
                take_queue,
                queue_gone,
                queue_detached,
                _detach_done,
                window_done_ack,
                st.quit,
                resized,
                focus_lost,
                redraw,
            )
        };

        if quit {
            // 退出前必须从本线程 looper 摘除输入队列：否则框架随后
            // dispose 输入队列时会与已销毁的 looper mutex 冲突（SIGABRT）。
            if let Some(q) = current_queue.take() {
                unsafe { super::sys::input::AInputQueue_detachLooper(q.as_ptr()) };
            }
            let (lock, cv) = &*_detach_done;
            *lock.lock().expect("detach flag poisoned") = true;
            cv.notify_all();
            info!("main loop quit");
            break;
        }

        if window_gone {
            // 后端借用窗口，必须先于窗口 release drop。
            _current_backend = None;
            // SAFETY: 与 take 时的 acquire 配对释放本线程引用。
            if let Some(w) = current_window.take() {
                unsafe { super::sys::window::ANativeWindow_release(w.as_ptr()) };
            }
            // 通知 destroyed 回调：窗口引用已全部释放，可以安全销毁。
            let (lock, cv) = &*window_done_ack;
            *lock.lock().expect("window flag poisoned") = true;
            cv.notify_all();
        }

        if let Some(raw) = take_window.map(SendPtr::as_ptr) {
            // SAFETY: created 回调已 acquire；此处再 acquire 供本线程跨回调借用。
            unsafe { super::sys::window::ANativeWindow_acquire(raw) };
            match Window::from_raw(raw) {
                Some(w) => {
                    // 启动链路第 3 步：渲染首帧（Demo 页树，Task 20 装配）。
                    // 后端选择：四步算法（强制开关/SoC 白名单/Vulkan 探测/cpu 兜底）
                    // + 静默降级链（select.rs Task 8）。
                    let scene = if demo.is_none() {
                        // 首个窗口：构造 Demo（字体已由 onCreate 预加载进程缓存，
                        // 此处从全局字体缓存注入）。
                        let font = crate::text::font::cached_font();
                        let mut d = crate::demo::Demo::new(w.width(), w.height(), font);
                        // Activity 重建：恢复上次状态（PRD §5.1.9 子集）。
                        let restored = shared
                            .lock()
                            .expect("shared state lock poisoned")
                            .saved_state
                            .take();
                        if let Some(state) = restored {
                            d.restore_state(&state);
                        }
                        let scene = d.render();
                        demo = Some(d);
                        scene
                    } else {
                        // 窗口重建（旋转/分屏）：尺寸变更后重渲染。
                        demo.as_mut().unwrap().resize(w.width(), w.height());
                        demo.as_mut().unwrap().render()
                    };
                    info!(
                        "demo scene built: {} commands, {}x{}",
                        scene.commands.len(),
                        scene.width,
                        scene.height
                    );
                    match select_backend(raw as *mut std::ffi::c_void, w.width(), w.height()) {
                        Some(mut backend) => {
                            match backend.render_frame(scene) {
                                Ok(()) => {
                                    // A1 终点：首帧已上屏。
                                    if let Some(ms) = crate::platform::perf::millis_since_boot() {
                                        info!(
                                            "first frame presented {}x{} in {}ms",
                                            w.width(),
                                            w.height(),
                                            ms
                                        );
                                    } else {
                                        info!(
                                            "first frame presented ({}x{})",
                                            w.width(),
                                            w.height()
                                        );
                                    }
                                }
                                Err(()) => warn!("first frame render failed"),
                            }
                            _current_backend = Some(backend);
                        }
                        None => warn!("no backend available"),
                    }
                    current_window = Some(w);
                }
                None => {
                    // 协议错误（NULL）：归还引用防泄漏。
                    unsafe { super::sys::window::ANativeWindow_release(raw) };
                }
            }
        }

        if let Some((w, h)) = resized {
            if let (Some(d), Some(b)) = (demo.as_mut(), _current_backend.as_mut()) {
                d.resize(w as i32, h as i32);
                b.resize(w, h);
                let scene = d.render();
                if b.render_frame(scene).is_ok() {
                    frame_stats.record_frame();
                } else {
                    warn!("resize frame render failed");
                }
            } else if let Some(d) = demo.as_mut() {
                d.resize(w as i32, h as i32);
            }
        }

        if focus_lost {
            // 合成系统 CANCEL：手势链复位（否则 ScrollView 会永久停留在
            // 拖拽态，之后所有触摸都被当滚动、点击全部失效）。
            if let Some(d) = demo.as_mut() {
                d.cancel_touch();
                if let Some(b) = _current_backend.as_mut() {
                    let scene = d.render();
                    if b.render_frame(scene).is_ok() {
                        frame_stats.record_frame();
                    } else {
                        warn!("cancel frame render failed");
                    }
                }
            }
        }

        if redraw {
            if let (Some(d), Some(b)) = (demo.as_mut(), _current_backend.as_mut()) {
                let scene = d.render();
                if b.render_frame(scene).is_ok() {
                    frame_stats.record_frame();
                } else {
                    warn!("redraw frame failed");
                }
            }
        }

        if queue_gone {
            // 框架即将销毁队列：先摘除 looper fd（本线程是 attach 方），
            // 再通知 destroyed 回调放行。
            if let Some(q) = current_queue.take() {
                unsafe { super::sys::input::AInputQueue_detachLooper(q.as_ptr()) };
            }
            let (lock, cv) = &*_detach_done;
            *lock.lock().expect("detach flag poisoned") = true;
            cv.notify_all();
        }

        if let Some(raw) = take_queue.map(SendPtr::as_ptr) {
            // 启动链路第 4 步：attach 到本线程 looper（V1 轮询模式，回调 NULL）。
            // SAFETY: raw 来自框架 created 回调，本线程 looper 有效。
            unsafe {
                super::sys::input::AInputQueue_attachLooper(
                    raw,
                    looper,
                    LOOPER_ID_INPUT,
                    None,
                    core::ptr::null_mut(),
                )
            };
            current_queue = SafeInputQueue::from_raw(raw);
            info!("input queue attached");
        }

        // 2) 填充状态保存槽（onSaveInstanceState 随时可读到最新状态）。
        if let Some(d) = demo.as_ref() {
            let json = d.save_state().to_json();
            *SAVED_STATE_SLOT.lock().expect("state slot poisoned") = Some(json);
        }

        // 3) 等待事件：Draw 页动画时节拍为 16ms（近似 vsync），其余无限阻塞。
        // 无条件泵取输入（回调 NULL 的 attach 模式下，框架经 ALooper_wake
        // 唤醒本线程,pollOnce 可能返回 ALOOPER_POLL_WAKE 而非 LOOPER_ID_INPUT,
        // 事件实际在 AInputQueue 里——按 ident 分支会漏泵导致 ANR,故不分支)。
        if let Some(q) = current_queue.as_ref() {
            if q.has_events() {
                // SAFETY: activity 指针在框架生命周期内有效（onDestroy join 前）。
                let activity_ptr = {
                    let st = shared.lock().expect("shared state lock poisoned");
                    st.activity.0
                };
                pump_input(
                    current_queue.as_ref(),
                    &mut demo,
                    &mut _current_backend,
                    Some(activity_ptr),
                    &mut frame_stats,
                    &mut fps_hud,
                );
            }
        }

        // IME 效果消费（SPEC-ime.md）：焦点意图在此下发 Kotlin，
        // 文本回推在此并入 Demo 状态——均不在 TextWatcher 线程做渲染。
        // 无障碍：点击回推消费 + 快照推送（SPEC-accessibility.md §3）。
        // TalkBack 关闭时 is_attached 仍为真但 Kotlin 侧会丢弃快照，
        // 因此此处再判一次脏标记，避免无变更时每帧推送。
        if super::a11y_bridge::should_push() {
            let mut a11y_dirty = false;
            if let Some(node_id) = super::a11y_bridge::take_pending_click() {
                if let Some(d) = demo.as_mut() {
                    // 与触摸路径同构：记录切页日志，便于验收时区分
                    // 「无障碍激活」与「原生触摸」两条通路的状态变更。
                    let page_before = d.current_page();
                    if d.perform_a11y_click(node_id) {
                        let page_after = d.current_page();
                        if page_before != page_after {
                            info!("page switched (a11y): {} -> {}", page_before, page_after);
                        }
                        // 点击导致状态变更：重绘 + 重新推送快照。
                        a11y_dirty = true;
                        if let Some(b) = _current_backend.as_mut() {
                            let scene = d.render();
                            if b.render_frame(scene).is_ok() {
                                frame_stats.record_frame();
                            } else {
                                warn!("a11y click frame render failed");
                            }
                        }
                    }
                }
            }
            if let Some(d) = demo.as_mut() {
                // 页树重建（切页/高亮/输入）后节点结构变化，需重推快照。
                // 只有代数变化/点击/强制请求时才构建：避免每轮循环都遍历全树。
                let generation = d.a11y_generation();
                let force = super::a11y_bridge::take_force_push();
                if a11y_dirty
                    || force
                    || generation != LAST_A11Y_GEN.load(std::sync::atomic::Ordering::Relaxed)
                {
                    LAST_A11Y_GEN.store(generation, std::sync::atomic::Ordering::Relaxed);
                    let nodes = d.a11y_snapshot();
                    super::a11y_bridge::push_snapshot(&nodes);
                }
            }
        }

        if super::ime_bridge::is_attached() {
            let effect_sent = super::ime_bridge::consume_pending();
            let mut ime_dirty = effect_sent;
            // 键盘高度变化需重排内容区（避让）：作废页树后按新
            // content_bottom 重建布局。
            let inset = super::ime_bridge::ime_inset();
            let prev = LAST_IME_INSET.swap(inset, std::sync::atomic::Ordering::Relaxed);
            if prev != inset {
                ime_dirty = true;
                if let Some(d) = demo.as_mut() {
                    d.invalidate_for_insets();
                }
            }
            if let Some((text, composing)) = super::ime_bridge::take_pending_text() {
                if let Some(d) = demo.as_mut() {
                    ime_dirty |= d.on_ime_text(&text, composing);
                }
            }
            if super::ime_bridge::take_pending_blur() {
                if let Some(d) = demo.as_mut() {
                    ime_dirty |= d.on_ime_blur();
                }
            }
            if ime_dirty {
                if let (Some(d), Some(b)) = (demo.as_mut(), _current_backend.as_mut()) {
                    let scene = d.render();
                    if b.render_frame(scene).is_ok() {
                        frame_stats.record_frame();
                    } else {
                        warn!("ime frame render failed");
                    }
                }
            }
        }
        // 超时策略：Draw 页动画 16ms（近似 vsync），其余无限阻塞；
        // IME 桥的文本/inset 变化经 eventfd 唤醒（零轮询开销）。
        let anim_timeout: i32 = if demo.as_ref().is_some_and(|d| d.needs_frames()) {
            16
        } else {
            -1
        };
        // 唤醒通道不可用时必须有限等待：onDestroy 的 join 依赖主循环能醒来。
        let timeout_ms: i32 = match wake.as_deref() {
            Some(w) if w.fd >= 0 => anim_timeout,
            _ => anim_timeout.min(250),
        };
        let mut out_fd: i32 = 0;
        let mut out_events: i32 = 0;
        let mut out_data: *mut core::ffi::c_void = core::ptr::null_mut();
        // SAFETY: 出参均合法。
        let ident = unsafe {
            super::sys::looper::ALooper_pollOnce(
                timeout_ms,
                &mut out_fd,
                &mut out_events,
                &mut out_data,
            )
        };
        // 近似 vsync 超时到达：推进 P2 动画并按需重绘（PRD §5.1.6：
        // 时间基准统一 Choreographer 帧回调——V1 以 poll 超时近似）。
        if ident == super::sys::looper::ALOOPER_POLL_TIMEOUT {
            let frame_result = if let (Some(d), Some(b)) =
                (demo.as_mut(), _current_backend.as_mut())
            {
                // 单调时钟：墙上时钟跳变（NTP/用户改时间）会让动画倒退或冻结。
                static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
                let start = START.get_or_init(std::time::Instant::now);
                let now = start.elapsed().as_millis() as u64;
                // 调用链耗时分解：advance → build → render 三段独立计时。
                let mut trace = crate::platform::perf::FrameTrace::new();
                let adv_start = std::time::Instant::now();
                if d.advance(now) {
                    trace.set(crate::platform::perf::STAGE_ADVANCE, adv_start.elapsed());
                    let build_start = std::time::Instant::now();
                    let mut scene = d.render();
                    fps_hud.draw(&mut scene, frame_stats.fps());
                    trace.set(crate::platform::perf::STAGE_BUILD, build_start.elapsed());
                    trace.cmd_count = scene.commands.len() as u32;
                    let render_start = std::time::Instant::now();
                    let ok = b.render_frame(scene).is_ok();
                    trace.set(crate::platform::perf::STAGE_RENDER, render_start.elapsed());
                    Some((ok, trace))
                } else {
                    None
                }
            } else {
                None
            };
            if let Some((ok, trace)) = frame_result {
                if ok {
                    frame_stats.record_render_time(
                        trace.stages[crate::platform::perf::STAGE_RENDER] / 1000,
                    );
                    frame_stats.record_trace(&trace);
                    frame_stats.record_frame();
                } else {
                    warn!("animation frame render failed");
                }
            }
        }
        // A3 内存采样：静止页每 2s 一次（statm 读文件有调度开销，
        // 动画/滚动期间不采样，避免污染 A2 帧时长）。
        if demo.as_ref().is_some() && last_rss_sample.elapsed().as_millis() >= 2_000 {
            last_rss_sample = std::time::Instant::now();
            if let Some(rss) = crate::platform::perf::sample_rss_kb() {
                if rss < 1024 * 1024 {
                    // < 1GB 正常路径：debug 级；超 1GB 升级 warn 便于 A3 判定。
                    log::debug!("rss: {} KB", rss);
                } else {
                    log::warn!("rss high: {} KB", rss);
                }
            }
        }
        match ident {
            LOOPER_ID_WAKE => {
                // 回调线程置位了交接状态；读干 eventfd 后回循环头同步。
                if let Some(w) = wake.as_deref() {
                    w.drain();
                }
            }
            ALOOPER_POLL_CALLBACK => {}
            ALOOPER_POLL_ERROR => {
                warn!("looper poll error, main loop exits");
                break;
            }
            _ => {}
        }
    }
    // 退出前释放后端与窗口引用（若有）。
    _current_backend = None;
    if let Some(w) = current_window.take() {
        // SAFETY: 与 take 时的 acquire 配对。
        unsafe { super::sys::window::ANativeWindow_release(w.as_ptr()) };
    }
    {
        let done = shared
            .lock()
            .expect("shared state lock poisoned")
            .window_done
            .clone();
        let (lock, cv) = &*done;
        *lock.lock().expect("window flag poisoned") = true;
        cv.notify_all();
    }
    info!("main loop exited");
}

/// 输入泵取：借出 → 预分发 → 解码 → 路由 → 归还。
///
/// # 分发线程不变量（PRD §5.2）
/// 本函数仅由主循环线程（持 Looper）调用——触摸/按键事件在泵取循环内
/// 完成解码与路由，禁止在 `AInputQueue` 回调线程里触碰 UI 状态。
fn pump_input(
    queue: Option<&SafeInputQueue>,
    demo: &mut Option<crate::demo::Demo>,
    backend: &mut Option<AnyBackend>,
    activity: Option<*mut ANativeActivity>,
    frame_stats: &mut crate::platform::perf::FrameStats,
    fps_hud: &mut crate::platform::perf::FpsHud,
) {
    let Some(queue) = queue else { return };
    while queue.has_events() {
        let Ok(Some(mut event)) = queue.get_event() else {
            break;
        };
        if queue.pre_dispatch(&mut event) {
            // IME 已消费，立即归还。
            event.finish(false);
            continue;
        }
        match event.kind() {
            EventKind::Key => {
                let handled = event
                    .decode_key()
                    .map(|key| handle_key(&key, activity))
                    .unwrap_or(false);
                event.finish(handled);
            }
            EventKind::Motion => {
                let handled = event
                    .decode_motion()
                    .map(|motion| {
                        let (x, y) = motion.primary().map(|p| (p.x, p.y)).unwrap_or((0.0, 0.0));
                        info!("motion: action={:?} x={:.1} y={:.1}", motion.action(), x, y);
                        handle_motion(&motion, demo.as_mut(), backend, frame_stats, fps_hud)
                    })
                    .unwrap_or(false);
                event.finish(handled);
            }
            EventKind::Other => event.finish(false),
        }
    }
}

/// 按键路由：BACK 请求结束 Activity（PRD §5.2 BACK 基础映射）。
///
/// # 返回
/// 已消费返回 `true`；未建模键位返回 `false`（框架按未处理转发）。
fn handle_key(key: &crate::ui::input::KeyEvent, activity: Option<*mut ANativeActivity>) -> bool {
    use crate::ui::input::{KeyAction, Keycode};
    if key.action != KeyAction::Up || key.keycode != Keycode::Back {
        // 只在抬起时触发（Android BACK 语义）；其余键位透传。
        return false;
    }
    debug!("BACK pressed, finishing activity");
    // SAFETY: activity 指针由框架主循环线程传入，onDestroy 前有效。
    if let Some(activity) = activity {
        unsafe { super::sys::activity::ANativeActivity_finish(activity) };
    }
    true
}

/// 触摸路由（Task 20）：重建页树 → Demo 分发 → 状态变更时重渲染。
///
/// # 返回
/// Demo 消费返回 `true`；无页树（启动未完成）时 `false`。
fn handle_motion(
    motion: &crate::ui::input::MotionEvent,
    demo: Option<&mut crate::demo::Demo>,
    backend: &mut Option<AnyBackend>,
    frame_stats: &mut crate::platform::perf::FrameStats,
    fps_hud: &mut crate::platform::perf::FpsHud,
) -> bool {
    let Some(demo) = demo else { return false };
    let page_before = demo.current_page();
    // 调用链耗时分解：dispatch_touch（手势/滚动物理 = advance）→ render（build）→ render_frame。
    let mut trace = crate::platform::perf::FrameTrace::new();
    let adv_start = std::time::Instant::now();
    let consumed = demo.dispatch_touch(motion);
    trace.set(crate::platform::perf::STAGE_ADVANCE, adv_start.elapsed());
    let page_after = demo.current_page();
    if page_before != page_after {
        info!("page switched: {} -> {}", page_before, page_after);
    }
    if consumed {
        let build_start = std::time::Instant::now();
        let mut scene = demo.render();
        fps_hud.draw(&mut scene, frame_stats.fps());
        trace.set(crate::platform::perf::STAGE_BUILD, build_start.elapsed());
        trace.cmd_count = scene.commands.len() as u32;
        if let Some(b) = backend.as_mut() {
            let render_start = std::time::Instant::now();
            let ok = b.render_frame(scene).is_ok();
            trace.set(crate::platform::perf::STAGE_RENDER, render_start.elapsed());
            if ok {
                frame_stats
                    .record_render_time(trace.stages[crate::platform::perf::STAGE_RENDER] / 1000);
                frame_stats.record_trace(&trace);
                frame_stats.record_frame();
            } else {
                warn!("touch frame render failed");
            }
        }
        true
    } else {
        false
    }
}
