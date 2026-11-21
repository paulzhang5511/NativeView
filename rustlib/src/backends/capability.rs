//! SoC 能力探测（PRD §5.3.2 选择算法的数据源）。
//!
//! # 设计
//! - 只读系统属性（`ro.soc.manufacturer` / `ro.soc.model` / `ro.board.platform`），
//!   经 `platform::android::sysprop` 安全封装读取，禁止直接 FFI 散落；
//! - 白名单为可维护常量表：新增旗舰 SoC 只改 [`HIGH_END_SOC_MODELS`] /
//!   [`HIGH_END_BOARD_PLATFORMS`]，不改逻辑（PRD §5.3.2）；
//! - 平台无关逻辑（属性值 → 能力判定）与 Android 属性读取分离：
//!   判定函数接收纯字符串，主机 `cargo test` 可完整覆盖。
//!
//! # 注意（PRD §6.4.5）
//! wgpu 关闭 `gles`，"GPU 可用" 的判定实质是"具备可用 Vulkan 适配器"，
//! 不含 GLES 兜底；该探测由选择层（select.rs）通过 wgpu 完成，本模块只管 SoC 侧。

/// 高端旗舰 SoC 型号白名单（`ro.soc.model`，精确匹配）。
///
/// 首版收录骁龙 8Gen2 至 8Elite（PRD §5.3.2）：SM8550 / SM8650 / SM8750 /
/// SM8850；SM8975 为同档平台（Adreno 850，实测真机），按 PRD
/// "及其同档平台"条款收录。命中即认为可跑 vello 全 GPU 管线。
pub const HIGH_END_SOC_MODELS: &[&str] = &["SM8550", "SM8650", "SM8750", "SM8850", "SM8975"];

/// 高端旗舰平台白名单（`ro.board.platform`，精确匹配）。
///
/// 与 [`HIGH_END_SOC_MODELS`] 同档的板级平台名（部分设备 `ro.soc.model` 为空，
/// 仅暴露板级名）；`art` 为 SM8975 实测板级名。
pub const HIGH_END_BOARD_PLATFORMS: &[&str] = &["kalama", "pineapple", "sun", "sun-pro", "art"];

/// 判定 SoC 是否命中高端白名单（PRD §5.3.2 第 2 步）。
///
/// # 参数
/// - `manufacturer`：`ro.soc.manufacturer` 的值（可能缺失）；
/// - `soc_model`：`ro.soc.model` 的值（可能缺失）；
/// - `board_platform`：`ro.board.platform` 的值（可能缺失）。
///
/// # 规则
/// 厂商须含 `qualcomm`/`qti`（大小写不敏感），**且** 型号或板级平台命中白名单。
/// 属性缺失按未命中处理（降级到第 3 步 Vulkan 探测，安全侧）。
pub fn is_high_end_soc(manufacturer: &str, soc_model: &str, board_platform: &str) -> bool {
    let mfg = manufacturer.to_ascii_lowercase();
    if !(mfg.contains("qualcomm") || mfg.contains("qti")) {
        return false;
    }
    HIGH_END_SOC_MODELS.iter().any(|&m| m == soc_model)
        || HIGH_END_BOARD_PLATFORMS
            .iter()
            .any(|&p| p == board_platform)
}

/// SoC 探测结果：选择算法第 2 步的输入。
///
/// 属性缺失用 `None` 表达；判定逻辑在 [`is_high_end_soc`] 之外，
/// 结构体仅承载读取结果，便于调用方记日志与测试注入。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SocInfo {
    /// `ro.soc.manufacturer`
    pub manufacturer: Option<String>,
    /// `ro.soc.model`
    pub model: Option<String>,
    /// `ro.board.platform`
    pub board_platform: Option<String>,
}

impl SocInfo {
    /// 读取 SoC 属性（Android 专属；主机测试直接构造结构体注入）。
    #[cfg(target_os = "android")]
    pub fn detect() -> Self {
        let prop = crate::platform::android::sysprop::SysProp;
        Self {
            manufacturer: prop.get("ro.soc.manufacturer"),
            model: prop.get("ro.soc.model"),
            board_platform: prop.get("ro.board.platform"),
        }
    }

    /// 是否命中高端白名单（属性缺失按空串处理，走安全侧）。
    pub fn is_high_end(&self) -> bool {
        is_high_end_soc(
            self.manufacturer.as_deref().unwrap_or(""),
            self.model.as_deref().unwrap_or(""),
            self.board_platform.as_deref().unwrap_or(""),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 白名单命中：厂商 + 型号（表驱动）。
    #[test]
    fn high_end_by_soc_model() {
        for model in HIGH_END_SOC_MODELS {
            assert!(is_high_end_soc(
                "Qualcomm Technologies, Inc",
                model,
                "unknown"
            ));
            assert!(is_high_end_soc("QTI", model, ""));
        }
    }

    /// 白名单命中：厂商 + 板级平台（表驱动）。
    #[test]
    fn high_end_by_board_platform() {
        for platform in HIGH_END_BOARD_PLATFORMS {
            assert!(is_high_end_soc("Qualcomm", "", platform));
        }
    }

    /// 厂商不匹配：即使型号命中也判否。
    #[test]
    fn rejected_when_manufacturer_mismatch() {
        assert!(!is_high_end_soc("MediaTek", "SM8550", "kalama"));
        assert!(!is_high_end_soc("", "SM8750", "sun"));
    }

    /// 型号/平台不在白名单：厂商正确也判否。
    #[test]
    fn rejected_when_model_not_whitelisted() {
        assert!(!is_high_end_soc("Qualcomm", "SM7325", "lahaina"));
        assert!(!is_high_end_soc("Qualcomm", "", ""));
    }

    /// 大小写不敏感：厂商字段含 qti 子串即可。
    #[test]
    fn manufacturer_case_insensitive() {
        assert!(is_high_end_soc("qualcomm technologies", "SM8650", ""));
        assert!(is_high_end_soc("Qti, Inc", "", "pineapple"));
    }

    /// 属性全缺失：判否（安全侧，走 Vulkan 探测降级）。
    #[test]
    fn missing_properties_rejected() {
        let info = SocInfo::default();
        assert!(!info.is_high_end());
    }

    /// 结构体注入 + 判定链路（模拟真机 SM8550 设备）。
    #[test]
    fn soc_info_from_values() {
        let info = SocInfo {
            manufacturer: Some("Qualcomm".into()),
            model: Some("SM8550".into()),
            board_platform: Some("kalama".into()),
        };
        assert!(info.is_high_end());
    }
}
