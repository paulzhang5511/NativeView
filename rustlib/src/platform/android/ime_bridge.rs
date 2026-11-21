//! IME 平台桥（SPEC-ime.md §1）：Rust ↔ Kotlin 隐形 EditText 的双向通道。
//!
//! # 数据流
//! ```text
//! Rust 控件点击 → ime::manager 登记意图
//!        ↓（帧回调消费）
//! 本模块 → JavaVM attach → ImeBridge.show/hide
//!        ↓（用户输入）
//! ImeBridge.nativeOnImeText → 本模块入口 → 待处理槽 → Demo 帧回调取用
//! ```
//!
//! # 线程约束
//! - 所有 JNI 入口由系统在 UI 线程调用；
//! - Rust → Kotlin 只在主循环线程（帧回调）调用，经
//!   `attach_current_thread` 借用主线程环境；
//! - **禁止**在 `TextWatcher` 线程触发渲染——入口只写待处理槽。
//!
//! # 零开销
//! 桥未挂载时所有路径立即返回，不 attach JVM（SPEC-ime 成功标准 5）。

use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use jni::Env;
use jni::objects::{Global, JObject, JValue};
use log::{info, warn};

use crate::ui::ime::manager::PendingEffect;

/// Kotlin `ImeBridge` 实例的全局引用。
static INSTANCE: Mutex<Option<Global<JObject<'static>>>> = Mutex::new(None);

/// JVM 句柄（挂载时缓存；JavaVM 为轻量 Clone 句柄）。
static VM: Mutex<Option<jni::JavaVM>> = Mutex::new(None);

/// 最近一次 IME 高度（px）；0 = 收起。供布局层查询。
static IME_INSET: AtomicU32 = AtomicU32::new(0);

/// 待消费的文本回推（Kotlin → Rust）。
static PENDING_TEXT: Mutex<Option<(String, Option<(usize, usize)>)>> = Mutex::new(None);

/// 待消费的失焦通知（键盘被系统强收/窗口失焦）。
static PENDING_BLUR: Mutex<bool> = Mutex::new(false);

/// 记录桥实例（Kotlin `onCreate` 调 `nativeAttachImeBridge`）。
///
/// # 参数
/// `env`：JNI 入口环境；`bridge`：Kotlin `ImeBridge` 实例（局部引用）。
pub fn attach(env: &mut Env<'_>, bridge: &JObject<'_>) -> jni::errors::Result<()> {
    let global = env.new_global_ref(bridge)?;
    let vm = env.get_java_vm()?;
    *INSTANCE.lock().unwrap() = Some(global);
    *VM.lock().unwrap() = Some(vm);
    info!("ime bridge attached");
    Ok(())
}

/// 解挂（Activity 销毁）。
pub fn detach() {
    *INSTANCE.lock().unwrap() = None;
    *VM.lock().unwrap() = None;
    IME_INSET.store(0, Ordering::Release);
    *PENDING_TEXT.lock().unwrap() = None;
    *PENDING_BLUR.lock().unwrap() = false;
    info!("ime bridge detached");
}

/// 桥是否已挂载。
pub fn is_attached() -> bool {
    INSTANCE.lock().unwrap_or_else(|e| e.into_inner()).is_some()
}

/// 最近一次键盘遮挡高度（px；0 = 收起）。
pub fn ime_inset() -> u32 {
    IME_INSET.load(Ordering::Acquire)
}

/// 消费队列中的 IME 效果（主循环帧回调调用一次）。
///
/// # 返回
/// true = 已下发系统调用（调用方需要重绘）。
pub fn consume_pending() -> bool {
    match crate::ui::ime::manager::take_effect() {
        PendingEffect::None => false,
        PendingEffect::Show {
            input_type,
            ime_options,
            text,
        } => {
            call_bridge(input_type, ime_options, &text);
            true
        }
        PendingEffect::Hide => {
            call_bridge_hide();
            true
        }
    }
}

/// 取走 Kotlin 回推的文本（主循环消费；无则 None）。
pub fn take_pending_text() -> Option<(String, Option<(usize, usize)>)> {
    PENDING_TEXT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
}

/// 取走失焦通知（主循环消费）。
pub fn take_pending_blur() -> bool {
    let mut g = PENDING_BLUR.lock().unwrap_or_else(|e| e.into_inner());
    let v = *g;
    *g = false;
    v
}

/// 写入文本回推（JNI 入口调用，UI 线程）。
///
/// # 合并语义
/// 同帧多次回推只保留最后一次：拼音 composing 抖动不应触发多帧无效重绘，
/// 且中间态对最终渲染无意义。
pub fn put_pending_text(text: String, composing: Option<(usize, usize)>) {
    *PENDING_TEXT.lock().unwrap_or_else(|e| e.into_inner()) = Some((text, composing));
    super::glue::wake_main_loop();
}

/// 记录失焦（JNI 入口调用）。
pub fn put_pending_blur() {
    *PENDING_BLUR.lock().unwrap_or_else(|e| e.into_inner()) = true;
    super::glue::wake_main_loop();
}

/// 下发 show（含 inputType/imeOptions/初始文本）到 Kotlin（主循环线程）。
///
/// # 为什么带 text
/// 隐形 EditText 必须从宿主当前文本开始，否则重聚焦清空文本、已有输入丢失。
fn call_bridge(input_type: i32, ime_options: i32, text: &str) {
    call_bridge_impl_with_text(
        jni::jni_str!("show"),
        jni::jni_sig!("(IILjava/lang/String;)V"),
        input_type,
        ime_options,
        text,
    );
}

/// 带字符串参数的调用外壳（局部引用只在本次调用内有效）。
fn call_bridge_impl_with_text<S>(
    name: &jni::strings::JNIStr,
    sig: S,
    input_type: i32,
    ime_options: i32,
    text: &str,
) where
    S: AsRef<jni::signature::MethodSignature<'static, 'static>>,
{
    if !is_attached() {
        return;
    }
    let vm_guard = VM.lock().unwrap_or_else(|e| e.into_inner());
    let Some(vm) = vm_guard.as_ref() else { return };
    let inst_guard = INSTANCE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(bridge) = inst_guard.as_ref() else {
        return;
    };
    let result = vm.attach_current_thread(|env| -> jni::errors::Result<()> {
        let jtext = env.new_string(text)?;
        env.call_method(
            bridge.as_obj(),
            name,
            sig,
            &[
                JValue::Int(input_type),
                JValue::Int(ime_options),
                JValue::Object(jtext.as_ref()),
            ],
        )?;
        Ok(())
    });
    if let Err(e) = result {
        warn!("ime bridge call failed: {e}");
    }
}

/// 光标锚点（插入点矩形，px）：Kotlin 侧转 `CursorAnchorInfo` 下发系统。
///
/// # 为什么需要
/// 拼音候选窗/放大镜依赖 `updateCursorAnchorInfo` 跟随插入点；
/// 缺此调用时系统把锚点锁在 EditText 自身位置（不可见），体验断裂。
pub fn push_cursor_anchor(x: f32, y: f32, height: f32) {
    if !is_attached() {
        return;
    }
    call_bridge_impl(
        jni::jni_str!("onCursorAnchor"),
        jni::jni_sig!("(FFF)V"),
        &[JValue::Float(x), JValue::Float(y), JValue::Float(height)],
    );
}

/// 下发 hide 到 Kotlin（主循环线程）。
fn call_bridge_hide() {
    call_bridge_impl(jni::jni_str!("hide"), jni::jni_sig!("()V"), &[]);
}

/// 统一调用外壳：attach → call_method → 错误记录。
fn call_bridge_impl<S>(name: &jni::strings::JNIStr, sig: S, args: &[JValue])
where
    S: AsRef<jni::signature::MethodSignature<'static, 'static>>,
{
    if !is_attached() {
        return;
    }
    let vm_guard = VM.lock().unwrap_or_else(|e| e.into_inner());
    let Some(vm) = vm_guard.as_ref() else { return };
    let inst_guard = INSTANCE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(bridge) = inst_guard.as_ref() else {
        return;
    };
    let result = vm.attach_current_thread(|env| -> jni::errors::Result<()> {
        env.call_method(bridge.as_obj(), name, sig, args)?;
        Ok(())
    });
    if let Err(e) = result {
        warn!("ime bridge call failed: {e}");
    }
}

/// 更新 IME 高度（JNI 入口调用）。
pub fn set_ime_inset(height_px: u32) {
    let prev = IME_INSET.swap(height_px, Ordering::Release);
    if prev != height_px {
        info!("ime inset changed: {prev}px -> {height_px}px");
        // 键盘弹出/收起需要重排内容区（避让），唤醒主循环重绘。
        super::glue::wake_main_loop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 未挂载时零开销：不 attach、不 panic。
    #[test]
    fn unattached_is_noop() {
        detach();
        assert!(!is_attached());
        assert_eq!(ime_inset(), 0);
        assert!(take_pending_text().is_none());
        assert!(!take_pending_blur());
    }

    /// 文本回推槽：后写覆盖前写。
    #[test]
    fn pending_text_last_wins() {
        put_pending_text("a".into(), None);
        put_pending_text("ni".into(), Some((0, 2)));
        assert_eq!(take_pending_text(), Some(("ni".into(), Some((0, 2)))));
        assert!(take_pending_text().is_none(), "consumed once");
    }

    /// 失焦标记：读取即清除。
    #[test]
    fn pending_blur_consumed_once() {
        put_pending_blur();
        assert!(take_pending_blur());
        assert!(!take_pending_blur());
    }

    /// inset 写入/读取。
    #[test]
    fn inset_roundtrip() {
        set_ime_inset(880);
        assert_eq!(ime_inset(), 880);
        set_ime_inset(0);
    }
}
