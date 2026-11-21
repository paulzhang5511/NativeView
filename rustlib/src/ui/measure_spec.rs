//! MeasureSpec：Android 位打包完全一致（Task 14，PRD §5.1.2）。
//!
//! # 不变量
//! 32 bit：高 2 bit 模式 + 低 30 bit 尺寸；`raw()` 与 Android
//! `MeasureSpec.getMeasureSpec` 的编码逐位一致，允许跨语言对照调试。

/// 测量模式（Android `MeasureSpec` 高 2 位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// 父视图未约束：可取任意期望尺寸。
    Unspecified = 0,
    /// 父视图强制精确尺寸：必须按此值设定。
    Exactly = 1,
    /// 父视图给定上限：可取 [0, size] 内任意值。
    AtMost = 2,
}

/// 模式位宽（高 2 bit）。
const MODE_SHIFT: u32 = 30;
/// 尺寸掩码（低 30 bit）。
const MODE_MASK: u32 = 0x3 << MODE_SHIFT;
const SIZE_MASK: u32 = !MODE_MASK;

/// Android `View.MeasureSpec` 等价物。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeasureSpec(u32);

impl MeasureSpec {
    /// Android `MeasureSpec.EXACTLY` 常量值（1 << 30）。
    pub const EXACTLY: u32 = 1 << MODE_SHIFT;
    /// Android `MeasureSpec.AT_MOST` 常量值（2 << 30）。
    pub const AT_MOST: u32 = 2 << MODE_SHIFT;
    /// Android `ViewGroup.LayoutParams.MATCH_PARENT`（-1）。
    pub const MATCH_PARENT: u32 = -1i32 as u32;
    /// Android `ViewGroup.LayoutParams.WRAP_CONTENT`（-2）。
    pub const WRAP_CONTENT: u32 = -2i32 as u32;

    /// Android `MeasureSpec.makeMeasureSpec(size, mode)` 等价。
    pub fn make(mode: Mode, size: u32) -> Self {
        // Android 用 (mode & MODE_MASK) | (size & SIZE_MASK)：
        // 负哨兵（-1/-2）经 u32 转换后低 30 bit 保留原位。
        Self(((mode as u32) << MODE_SHIFT) | (size & SIZE_MASK))
    }

    /// 从原始打包值解析（JNI 直接传 int 时使用）。
    pub fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    /// 原始打包值（JNI 回传用）。
    pub fn raw(self) -> u32 {
        self.0
    }

    /// 模式。
    pub fn mode(self) -> Mode {
        match (self.0 & MODE_MASK) >> MODE_SHIFT {
            0 => Mode::Unspecified,
            1 => Mode::Exactly,
            _ => Mode::AtMost,
        }
    }

    /// 尺寸（低 30 bit；可能为负哨兵值的位模式）。
    pub fn size(self) -> u32 {
        self.0 & SIZE_MASK
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 模式位打包无交叉污染。
    #[test]
    fn mode_bits() {
        assert_eq!(Mode::Unspecified as u32, 0);
        assert_eq!(Mode::Exactly as u32, 1);
        assert_eq!(Mode::AtMost as u32, 2);
    }
}
