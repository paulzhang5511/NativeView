//! 尺寸单位换算：dp / sp → px（框架公共层）。
//!
//! # 为什么框架层需要它
//! Android 的布局尺寸语义是 **dp**（density-independent pixel），文字尺寸
//! 语义是 **sp**（scale-independent pixel，随用户字体大小缩放）。
//! 这两者都不是物理像素，必须先换算成 px 才能参与 measure/layout/draw。
//!
//! 若框架不提供统一入口，每个使用方都会自己乘 density——一旦某处漏乘
//! 或乘错基准（density vs scaledDensity），表现就是「同一份代码在高密度
//! 设备上元素过小/文字不可读」，且极难定位。
//!
//! # 主机侧语义
//! 非 Android 目标下 density 恒为 1.0：`dp(16.0) == 16.0`，黄金图基准与
//! 单元测试因此与设备无关、完全可复现。
//!
//! # 边界
//! 本模块只做换算，不持有状态；基准值由平台层
//! （`platform::android::density`）注入。

/// dp 换算所需的密度基准（px/dp）。
///
/// # 平台差异
/// Android 取 Kotlin 注入的 `DisplayMetrics.density`；
/// 其他平台固定 1.0（1dp = 1px，测试确定性所需）。
#[inline]
fn density() -> f32 {
    #[cfg(target_os = "android")]
    {
        crate::platform::android::density::density()
    }
    #[cfg(not(target_os = "android"))]
    {
        1.0
    }
}

/// sp 换算所需的字体缩放密度（px/sp）。
#[inline]
fn scaled_density() -> f32 {
    #[cfg(target_os = "android")]
    {
        crate::platform::android::density::scaled_density()
    }
    #[cfg(not(target_os = "android"))]
    {
        1.0
    }
}

/// dp → px（布局尺寸：宽高、边距、内边距、圆角）。
///
/// # 示例
/// 640dpi 设备（density = 4.0）上 `dp(48.0) == 192.0` px。
#[inline]
pub fn dp(value: f32) -> f32 {
    value * density()
}

/// sp → px（文字尺寸：随用户系统字体大小缩放）。
///
/// # 与 dp 的区别
/// `sp` 使用 `scaledDensity`，用户把系统字体调大时文字必须同步变大
///（无障碍要求）；纯 `dp` 文字不随字体设置变化。
#[inline]
pub fn sp(value: f32) -> f32 {
    value * scaled_density()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 主机侧 dp 恒等（基准可复现的前提）。
    #[test]
    fn dp_identity_on_host() {
        #[cfg(not(target_os = "android"))]
        {
            assert_eq!(dp(16.0), 16.0);
            assert_eq!(dp(0.0), 0.0);
            assert_eq!(dp(-8.0), -8.0);
        }
    }

    /// 主机侧 sp 恒等。
    #[test]
    fn sp_identity_on_host() {
        #[cfg(not(target_os = "android"))]
        {
            assert_eq!(sp(15.0), 15.0);
        }
    }

    /// dp 与 sp 在主机侧等价（都退化为 1.0）。
    #[test]
    fn dp_equals_sp_on_host() {
        #[cfg(not(target_os = "android"))]
        {
            assert_eq!(dp(24.0), sp(24.0));
        }
    }
}
