//! 无障碍平台桥（SPEC-accessibility.md §2/§3）：快照推送与点击回推。
//!
//! # 数据流
//! ```text
//! Rust 帧回调 ──快照 Vec<A11yNode>──→ 本模块 ──JNI──→ A11yBridge.kt
//!                                                        │
//! Rust 状态变更 ←──── perform_a11y_click(nodeId) ────────┘
//! ```
//!
//! # 传输格式
//! 节点是结构化数据（含字符串与多个布尔），逐字段 JNI 调用会产生
//! 大量跨语言往返。因此序列化为**单次字符串**推送：
//! 每节点一行，字段用 `\u{1f}`（单元分隔符）分隔——该字符不可能是
//! 用户可见文本，且天然避免 CSV 转义问题。
//!
//! # 零开销（SPEC 成功标准 3）
//! 桥未挂载（TalkBack 关闭）时：`push_snapshot` 立即返回，
//! 不序列化、不 attach 线程、不产生任何 JNI 调用。

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use jni::Env;
use jni::objects::{Global, JObject, JValue};
use log::{info, warn};

use crate::ui::a11y::A11yNode;

/// Kotlin `A11yBridge` 实例全局引用。
static INSTANCE: Mutex<Option<Global<JObject<'static>>>> = Mutex::new(None);

/// JVM 句柄。
static VM: Mutex<Option<jni::JavaVM>> = Mutex::new(None);

/// 待处理的点击回调（Kotlin → Rust，UI 线程写入）。
static PENDING_CLICK: AtomicI32 = AtomicI32::new(-1);

/// TalkBack/无障碍服务是否启用（Kotlin 侧 AccessibilityManager 状态）。
///
/// # 为什么单独维护
/// 桥在 Activity 创建时就挂载，`is_attached()` 恒为真；若按挂载态建快照，
/// TalkBack 关闭时每次状态变更仍有序列化 + JNI + 解析开销（违反零开销约束）。
static A11Y_ENABLED: AtomicBool = AtomicBool::new(false);
/// 系统侧曾主动查询节点树（UiAutomation/无障碍服务建连），粘性开启。
///
/// `AccessibilityManager.isEnabled()` 只反映设置页总开关；UiAutomation、
/// 部分 ROM 的无障碍代理在建连时并不置位它，但确实会查询 provider。
/// 若只看总开关，会表现为「节点树偶发为空」。
static A11Y_REQUESTED: AtomicBool = AtomicBool::new(false);
/// 需要立即全量重推一次（provider 查询时节点为空，Kotlin 侧回调触发）。
static A11Y_FORCE_PUSH: AtomicBool = AtomicBool::new(true);

/// 挂载 Kotlin 桥实例（`onCreate` 调用一次）。
pub fn attach(env: &mut Env<'_>, bridge: &JObject<'_>) -> jni::errors::Result<()> {
    let global = env.new_global_ref(bridge)?;
    let vm = env.get_java_vm()?;
    *INSTANCE.lock().unwrap_or_else(|e| e.into_inner()) = Some(global);
    *VM.lock().unwrap_or_else(|e| e.into_inner()) = Some(vm);
    info!("a11y bridge attached");
    Ok(())
}

/// 解挂（Activity 销毁）。
pub fn detach() {
    *INSTANCE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *VM.lock().unwrap_or_else(|e| e.into_inner()) = None;
    A11Y_ENABLED.store(false, Ordering::Release);
    A11Y_REQUESTED.store(false, Ordering::Release);
    A11Y_FORCE_PUSH.store(true, Ordering::Release);
    PENDING_CLICK.store(-1, Ordering::Release);
    info!("a11y bridge detached");
}

/// 桥是否已挂载（TalkBack 开启且 Activity 已挂载）。
pub fn is_attached() -> bool {
    INSTANCE.lock().unwrap_or_else(|e| e.into_inner()).is_some()
}

/// Kotlin 侧上报无障碍服务开关（Activity 监听 AccessibilityManager 状态）。
/// 系统侧请求过节点（Kotlin provider 首次查询时调用）→ 粘性打开推送。
pub fn set_requested() {
    A11Y_REQUESTED.store(true, Ordering::Release);
    A11Y_FORCE_PUSH.store(true, Ordering::Release);
    super::glue::wake_main_loop();
}

/// 取出并清除「强制重推」请求。
pub fn take_force_push() -> bool {
    A11Y_FORCE_PUSH.swap(false, Ordering::AcqRel)
}

/// 是否需要向 Kotlin 推快照（总开关开启或系统侧曾查询）。
pub fn should_push() -> bool {
    is_attached() && (is_enabled() || A11Y_REQUESTED.load(Ordering::Acquire))
}

pub fn set_enabled(enabled: bool) {
    A11Y_ENABLED.store(enabled, Ordering::Release);
    if enabled {
        // 刚开启时需要一次全量快照，唤醒主循环立即推送。
        super::glue::wake_main_loop();
    }
}

/// 无障碍服务是否启用（零开销门禁）。
pub fn is_enabled() -> bool {
    A11Y_ENABLED.load(Ordering::Acquire)
}

/// 推送完整快照到 Kotlin（帧回调内调用）。
///
/// # 返回
/// true = 已实际推送。
///
/// # 为什么整表推送而非增量
/// V1 的视图树规模小（P1 为 200 行），整表推送实现简单且不存在
/// 增量与整表的时序竞态；节点数增长到需要增量时再演进协议。
pub fn push_snapshot(nodes: &[A11yNode]) -> bool {
    if !should_push() {
        return false;
    }
    let payload = encode_snapshot(nodes);
    let vm_guard = VM.lock().unwrap_or_else(|e| e.into_inner());
    let Some(vm) = vm_guard.as_ref() else {
        return false;
    };
    let inst_guard = INSTANCE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(bridge) = inst_guard.as_ref() else {
        return false;
    };
    let result = vm.attach_current_thread(|env| -> jni::errors::Result<()> {
        let jstr = env.new_string(&payload)?;
        env.call_method(
            bridge.as_obj(),
            jni::jni_str!("onSnapshot"),
            jni::jni_sig!("(Ljava/lang/String;)V"),
            &[JValue::Object(jstr.as_ref())],
        )?;
        Ok(())
    });
    match result {
        Ok(()) => {
            info!("a11y snapshot pushed: {} nodes", nodes.len());
            true
        }
        Err(e) => {
            warn!("a11y snapshot push failed: {e}");
            false
        }
    }
}

/// 取走待处理的点击（主循环消费）。
pub fn take_pending_click() -> Option<i32> {
    let v = PENDING_CLICK.swap(-1, Ordering::AcqRel);
    if v < 0 { None } else { Some(v) }
}

/// 记录点击回推（JNI 入口调用，UI 线程）。
///
/// 日志保留在 info 级：这是区分「TalkBack 无障碍激活」与「原生触摸点击」
/// 两条通路的唯一观测点，排障与验收都需要它。
pub fn put_pending_click(node_id: i32) {
    info!("a11y action click: node_id={node_id}");
    PENDING_CLICK.store(node_id, Ordering::Release);
    super::glue::wake_main_loop();
}

/// 快照序列化：每节点一行，字段以单元分隔符分隔。
///
/// # 字段顺序（Kotlin 侧必须逐位对应）
/// `id, parentId, left, top, right, bottom, text, contentDescription,
///  className, clickable, enabled, checked, visible, actionTag`
///
/// # 为什么用 `\u{1f}`
/// 该控制字符不可能出现在用户文本中，无需转义；字符串字段内的换行
/// 会被替换为空格，保证「一行一节点」的解析不变量。
fn encode_snapshot(nodes: &[A11yNode]) -> String {
    const SEP: char = '\u{1f}';
    let mut out = String::with_capacity(nodes.len() * 96);
    for n in nodes {
        let fields: [String; 14] = [
            n.id.to_string(),
            n.parent_id.to_string(),
            n.bounds.0.to_string(),
            n.bounds.1.to_string(),
            n.bounds.2.to_string(),
            n.bounds.3.to_string(),
            sanitize(&n.text),
            sanitize(&n.content_description),
            n.class_name.to_string(),
            (n.clickable as u8).to_string(),
            (n.enabled as u8).to_string(),
            (n.checked as u8).to_string(),
            (n.visible as u8).to_string(),
            n.action_tag.to_string(),
        ];
        out.push_str(&fields.join(&SEP.to_string()));
        out.push('\n');
    }
    out
}

/// 清洗文本字段：去掉分隔符与换行，防止破坏行/字段结构。
fn sanitize(s: &str) -> String {
    s.replace(['\u{1f}', '\n', '\r'], " ")
}
