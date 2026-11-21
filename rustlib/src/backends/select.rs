//! 后端选择算法与静默降级链（PRD §5.3.2 / §5.3.3）。
//!
//! # 四步选择算法（自上而下，命中即停）
//! 1. `debug.nativeview.backend` 强制开关 → 直接采用；
//! 2. 高端 SoC 白名单 → `vello`；
//! 3. 可用 Vulkan 适配器（wgpu 探测）→ `vello_gpu`；
//! 4. 兜底 → `cpu`。
//!
//! # 静默降级链
//! - 选中目标后初始化失败：`vello` → `vello_gpu` → `cpu` 逐级尝试；
//! - 降级对用户完全无感（不弹窗、不 Toast），仅输出英文告警日志；
//! - 运行期失败降级（连续渲染失败阈值触发）由 glue 接入，V1 先落选择期降级。
//!
//! # 可查询性
//! 当前生效后端经 `debug.nativeview.backend.current` 只读属性暴露
//! （PRD §5.3.3），自动化验收断言用；属性写入由 select 成功后落值，
//! 未选中前读取返回空（非 vello 值）。

#[cfg(target_os = "android")]
use crate::backends::AnyBackend;
use crate::backends::BackendKind;
use crate::backends::capability::SocInfo;

/// 强制后端调试开关的系统属性名（PRD §6.4.7：调试开关走系统属性，不经 Kotlin）。
pub const PROP_FORCE_BACKEND: &str = "debug.nativeview.backend";
/// 当前生效后端的只读查询属性名（自动化验收断言用）。
pub const PROP_CURRENT_BACKEND: &str = "debug.nativeview.backend.current";

/// 纯逻辑版选择决策：给定强制值与 SoC 信息，返回第一步即命中的目标后端。
///
/// # 参数
/// - `forced`：`debug.nativeview.backend` 的值（非法值视为未设置）。
///
/// # 返回
/// 返回"期望目标"；实际能否采用由 [`select_backend`] 中的降级链兜底
/// （例如强制 `vello` 但无 Vulkan 时静默降级）。
pub fn decide(forced: Option<&str>, soc: &SocInfo) -> BackendKind {
    // 第 1 步：强制开关（表驱动校验合法值，非法值忽略并告警）。
    if let Some(v) = forced {
        match parse_backend_name(v) {
            Some(kind) => return kind,
            None => log::warn!("ignored invalid {} value: {:?}", PROP_FORCE_BACKEND, v),
        }
    }
    // 第 2 步：高端 SoC 白名单 → vello。
    if soc.is_high_end() {
        return BackendKind::Vello;
    }
    // 第 3 步（Vulkan 探测）与第 4 步（cpu 兜底）在 select_backend 中执行：
    // 探测需要真机构造 wgpu，纯逻辑层不模拟。
    BackendKind::VelloGpu
}

/// 解析后端名（`cpu` / `vello_gpu` / `vello`，大小写不敏感）。
///
/// 与 [`BackendKind::name`] 互逆，保证开关值与日志名同源。
pub fn parse_backend_name(name: &str) -> Option<BackendKind> {
    match name.trim().to_ascii_lowercase().as_str() {
        "cpu" => Some(BackendKind::Cpu),
        "vello_gpu" => Some(BackendKind::VelloGpu),
        "vello" => Some(BackendKind::Vello),
        _ => None,
    }
}

/// 读取强制开关（Android 专属；主机测试经 `decide` 注入）。
#[cfg(target_os = "android")]
fn forced_backend() -> Option<String> {
    crate::platform::android::sysprop::SysProp.get(PROP_FORCE_BACKEND)
}

/// 把当前生效后端写入只读查询属性（setprop 由调试方执行，此处仅记日志兜底）。
///
/// PRD §5.3.3 要求 `debug.nativeview.backend.current` 可查——应用进程无
/// `system_properties` 写权限（`debug.` 域 setprop 需 shell/root），故此处
/// 输出同格式日志行，验收脚本从 logcat 断言；setprop 场景由外层（调试者）执行。
#[cfg(target_os = "android")]
fn log_current_backend(kind: BackendKind) {
    log::info!("{}={}", PROP_CURRENT_BACKEND, kind.name());
}

/// 后端选择入口：执行四步算法 + 逐级降级，返回首个初始化成功的后端。
///
/// # 失败语义
/// 所有目标均初始化失败（极端：无渲染能力）返回 `None`，调用方（glue）
/// 记日志并保持空窗；禁止 panic。
#[cfg(target_os = "android")]
pub fn select_backend(
    window_ptr: *mut std::ffi::c_void,
    width: i32,
    height: i32,
) -> Option<AnyBackend> {
    let (w, h) = (width.max(1) as u32, height.max(1) as u32);

    let soc = SocInfo::detect();
    let forced = forced_backend();
    let target = decide(forced.as_deref(), &soc);
    log::info!(
        "backend select: target={} (soc={:?}, forced={:?})",
        target.name(),
        soc,
        forced.as_deref().unwrap_or("")
    );

    // 降级序列：目标起，vello -> vello_gpu -> cpu；cpu 无处可降。
    let chain = fallback_chain(target);
    for kind in chain {
        match try_init(kind, window_ptr, w, h) {
            Some(backend) => {
                if kind != target {
                    log::warn!(
                        "backend fallback: {} -> {} (reason: init failed, likely no vulkan)",
                        target.name(),
                        kind.name()
                    );
                }
                log::info!("backend selected: {}", kind.name());
                log_current_backend(kind);
                return Some(backend);
            }
            None => continue,
        }
    }
    log::error!("no backend available");
    None
}

/// 降级链：目标后端起，仅向下不向上（PRD §5.3.2 降级语义）。
///
/// 完整顺序 vello → vello_gpu → cpu；以目标为起点截取后缀。
/// （vello_gpu 失败不会回升到 vello；cpu 无处可降。）
pub fn fallback_chain(target: BackendKind) -> Vec<BackendKind> {
    let full = [BackendKind::Vello, BackendKind::VelloGpu, BackendKind::Cpu];
    // 找到目标在完整顺序中的位置，取其后缀（目标可能来自强制开关，位置固定）。
    let start = full
        .iter()
        .position(|&k| k == target)
        .unwrap_or(full.len() - 1);
    full[start..].to_vec()
}

/// 尝试初始化指定后端（feature 未编译时跳过，视为降级链中的一级缺失）。
#[cfg(target_os = "android")]
fn try_init(
    kind: BackendKind,
    window_ptr: *mut std::ffi::c_void,
    w: u32,
    h: u32,
) -> Option<AnyBackend> {
    match kind {
        BackendKind::Cpu => {
            #[cfg(feature = "cpu")]
            {
                cpu_backend_from_ptr(window_ptr, w, h)
                    .ok()
                    .map(AnyBackend::Cpu)
            }
            #[cfg(not(feature = "cpu"))]
            {
                let _ = (window_ptr, w, h);
                None
            }
        }
        BackendKind::VelloGpu => {
            #[cfg(feature = "vello_gpu")]
            {
                crate::backends::vello_gpu::VelloGpuBackend::new(window_ptr, w, h)
                    .ok()
                    .map(AnyBackend::VelloGpu)
            }
            #[cfg(not(feature = "vello_gpu"))]
            {
                let _ = (window_ptr, w, h);
                None
            }
        }
        BackendKind::Vello => {
            #[cfg(feature = "vello")]
            {
                crate::backends::vello::VelloBackend::new(window_ptr, w, h)
                    .ok()
                    .map(AnyBackend::Vello)
            }
            #[cfg(not(feature = "vello"))]
            {
                let _ = (window_ptr, w, h);
                None
            }
        }
    }
}

/// cpu 后端构造（值传递 Window 包装，消除 unsafe transmute）。
#[cfg(all(target_os = "android", feature = "cpu"))]
fn cpu_backend_from_ptr(
    window_ptr: *mut std::ffi::c_void,
    w: u32,
    h: u32,
) -> Result<crate::backends::cpu::CpuBackend, ()> {
    let _ = (w, h);
    // SAFETY: 调用方（glue）保证指针有效期覆盖后端存活期；
    // Window 为裸指针包装且无 Drop，move 语义与借用等价。
    let window = match crate::platform::android::window::Window::from_raw(
        window_ptr as *mut crate::platform::android::sys::window::ANativeWindow,
    ) {
        Some(w) => w,
        None => return Err(()),
    };
    crate::backends::cpu::CpuBackend::new(window)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn soc(model: &str, platform: &str) -> SocInfo {
        SocInfo {
            manufacturer: Some("Qualcomm".into()),
            model: Some(model.into()),
            board_platform: Some(platform.into()),
        }
    }

    /// 表驱动：强制开关四分支。
    #[test]
    fn forced_backend_overrides() {
        let s = soc("SM7325", "lahaina");
        assert_eq!(decide(Some("cpu"), &s), BackendKind::Cpu);
        assert_eq!(decide(Some("vello_gpu"), &s), BackendKind::VelloGpu);
        assert_eq!(decide(Some("vello"), &s), BackendKind::Vello);
        // 大小写与空白容错
        assert_eq!(decide(Some("  VELLO "), &s), BackendKind::Vello);
    }

    /// 表驱动：白名单命中 → vello；未命中 → vello_gpu（探测由真机层兜底）。
    #[test]
    fn whitelist_selects_vello() {
        for model in crate::backends::capability::HIGH_END_SOC_MODELS {
            let s = SocInfo {
                manufacturer: Some("Qualcomm".into()),
                model: Some((*model).into()),
                board_platform: None,
            };
            assert_eq!(decide(None, &s), BackendKind::Vello);
        }
        assert_eq!(
            decide(None, &soc("SM7325", "lahaina")),
            BackendKind::VelloGpu
        );
    }

    /// 表驱动：非法强制值忽略 + 告警，回落正常算法。
    #[test]
    fn invalid_forced_value_falls_through() {
        let s = soc("SM8550", "kalama");
        assert_eq!(decide(Some("gpu"), &s), BackendKind::Vello);
        assert_eq!(decide(Some(""), &s), BackendKind::Vello);
    }

    /// 表驱动：降级链四目标各自完整。
    #[test]
    fn fallback_chain_complete() {
        assert_eq!(
            fallback_chain(BackendKind::Vello),
            vec![BackendKind::Vello, BackendKind::VelloGpu, BackendKind::Cpu]
        );
        assert_eq!(
            fallback_chain(BackendKind::VelloGpu),
            vec![BackendKind::VelloGpu, BackendKind::Cpu]
        );
        assert_eq!(fallback_chain(BackendKind::Cpu), vec![BackendKind::Cpu]);
    }

    /// 后端名解析与 name() 互逆。
    #[test]
    fn parse_name_roundtrip() {
        for kind in [BackendKind::Cpu, BackendKind::VelloGpu, BackendKind::Vello] {
            assert_eq!(parse_backend_name(kind.name()), Some(kind));
        }
        assert_eq!(parse_backend_name("gles"), None);
    }
}
