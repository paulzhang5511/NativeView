//! JNI 入口：Kotlin 侧桥接（路线 B 允许的最小 JNI 面）。
//!
//! # 设计
//! - 仅承接"Kotlin 空壳 Activity 一次性注入的平台参数"（V1：系统栏 insets）；
//! - 符号命名遵循 JNI 规范：`Java_com_nativeview_app_MyNativeActivity_方法名`；
//! - 禁止任何业务 UI 逻辑经此层进入 Rust。

use jni::Env;
use jni::objects::{JClass, JObject, JString};

/// Kotlin 侧注入系统栏高度（onCreate 读取 WindowInsets 后调用）。
///
/// # 参数
/// `status`：状态栏高度（px）；`nav`：导航栏高度（px，含手势条区）。
///
/// # 安全
/// 由 JNI 框架调用；参数为基本类型，无引用生命周期风险。
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_nativeview_app_MyNativeActivity_setSystemBarInsets(
    _env: jni::EnvUnowned,
    _class: JClass,
    status: i32,
    nav: i32,
) {
    // 负值视为协议错误，按 0 处理（防越界布局）。
    let status = status.max(0) as u32;
    let nav = nav.max(0) as u32;
    super::insets::set_system_bar_insets(status, nav);
}

/// Kotlin 侧注入显示密度（`onCreate` 读取 `DisplayMetrics` 后调用）。
///
/// # 参数
/// `density`：px/dp；`scaled_density`：px/sp（含用户字体缩放）。
///
/// # 安全
/// 由 JNI 框架调用；参数为基本类型，无引用生命周期风险。
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_nativeview_app_MyNativeActivity_setDisplayDensity(
    _env: jni::EnvUnowned,
    _class: JClass,
    density: f32,
    scaled_density: f32,
) {
    super::density::set_density(density, scaled_density);
}

/// Kotlin 侧注入系统动画时长倍率（`onCreate`/`onResume` 读 `ANIMATOR_DURATION_SCALE`）。
///
/// # 参数
/// `scale`：1.0 为正常速度；0 表示用户已关闭系统动画，动效须立即落到终态。
///
/// # 安全
/// 由 JNI 框架调用；参数为基本类型，无引用生命周期风险。
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_nativeview_app_MyNativeActivity_setAnimatorScale(
    _env: jni::EnvUnowned,
    _class: JClass,
    scale: f32,
) {
    crate::platform::motion::set_animator_scale(scale);
}

/// Kotlin 侧挂载 IME 桥（`ImeBridge` 实例创建后调用一次）。
///
/// # 参数
/// `env`：JNI 环境；`bridge`：`ImeBridge` 实例（转为全局引用长期持有）。
///
/// # 线程
/// 由系统在 UI 线程调用（Activity.onCreate）。
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_nativeview_app_MyNativeActivity_nativeAttachImeBridge(
    mut env: jni::EnvUnowned,
    _class: JClass,
    bridge: JObject,
) {
    let _ = env.with_env(|env: &mut Env| -> jni::errors::Result<()> {
        super::ime_bridge::attach(env, &bridge)
    });
}

/// Kotlin 侧解挂 IME 桥（Activity 销毁）。
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_nativeview_app_MyNativeActivity_nativeDetachImeBridge(
    _env: jni::EnvUnowned,
    _class: JClass,
) {
    super::ime_bridge::detach();
}

/// IME 文本回推（`TextWatcher` 逐事件调用，含 composing 区间）。
///
/// # 参数
/// `text`：全文；`composing_start`/`composing_end`：composing 区间
///（字符下标，`< 0` 或相等表示无预编辑）。
///
/// # 线程
/// UI 线程。此处**只登记状态**，禁止触发渲染（SPEC-ime 边界约束）。
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_nativeview_app_ImeBridge_nativeOnImeText(
    mut env: jni::EnvUnowned,
    _this: JObject,
    text: JString,
    composing_start: i32,
    composing_end: i32,
) {
    let _ = env.with_env(|env: &mut Env| -> jni::errors::Result<()> {
        let text: String = text.try_to_string(env)?;
        let composing = if composing_start >= 0 && composing_end > composing_start {
            Some((composing_start as usize, composing_end as usize))
        } else {
            None
        };
        super::ime_bridge::put_pending_text(text, composing);
        Ok(())
    });
}

/// IME 被系统收起（窗口失焦/键盘强收）。
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_nativeview_app_ImeBridge_nativeOnImeHidden(
    _env: jni::EnvUnowned,
    _this: JObject,
) {
    super::ime_bridge::put_pending_blur();
}

/// 键盘高度变化（避让转发）。
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_nativeview_app_ImeBridge_nativeOnImeInset(
    _env: jni::EnvUnowned,
    _this: JObject,
    height_px: i32,
) {
    super::ime_bridge::set_ime_inset(height_px.max(0) as u32);
}

/// Kotlin 侧挂载无障碍桥（`A11yBridge` 实例创建后调用一次）。
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_nativeview_app_MyNativeActivity_nativeAttachA11yBridge(
    mut env: jni::EnvUnowned,
    _class: JClass,
    bridge: JObject,
) {
    let _ = env.with_env(|env: &mut Env| -> jni::errors::Result<()> {
        super::a11y_bridge::attach(env, &bridge)
    });
}

/// Kotlin 侧解挂无障碍桥（Activity 销毁）。
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_nativeview_app_MyNativeActivity_nativeDetachA11yBridge(
    _env: jni::EnvUnowned,
    _class: JClass,
) {
    super::a11y_bridge::detach();
}

/// Kotlin 侧上报无障碍服务开关（Activity 监听 AccessibilityManager）。
///
/// # 线程
/// UI 线程；仅写原子标记并唤醒主循环，不触发渲染。
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_nativeview_app_MyNativeActivity_nativeSetA11yEnabled(
    _env: jni::EnvUnowned,
    _class: JClass,
    enabled: jni::sys::jboolean,
) {
    super::a11y_bridge::set_enabled(enabled);
}

/// 系统无障碍层首次查询节点（UiAutomation/TalkBack 建连）。
///
/// # 语义
/// 关闭无障碍闸门时快照不推送；但系统侧（尤其是 UiAutomation 测试）
/// 可能在 `AccessibilityManager.isEnabled` 仍为 false 时直接查询 provider，
/// 此时由 Kotlin 侧回调本函数临时打开闸门，保证节点树可枚举。
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_nativeview_app_A11yBridge_nativeRequestSnapshot(
    _env: jni::EnvUnowned,
    _this: JObject,
) {
    super::a11y_bridge::set_requested();
}

/// TalkBack 触发节点点击（`ACTION_CLICK` 回推）。
///
/// # 线程
/// UI 线程。只登记 id 并唤醒主循环，点击副作用在主循环内执行，
/// 避免与手势分发链并发修改视图状态。
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_nativeview_app_A11yBridge_nativeOnNodeClick(
    _env: jni::EnvUnowned,
    _this: JObject,
    node_id: i32,
) {
    super::a11y_bridge::put_pending_click(node_id);
}
