//! `android/rect.h` 的手写绑定（NDK 27.3.13750724）。
//!
/// `ARect`：矩形区域（android.graphics.Rect 的 C 对应物）。
///
/// # 不变量
/// 有效矩形中 `left <= right` 且 `top <= bottom`。
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ARect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, size_of};

    /// 4 个 i32，16 字节、4 字节对齐。
    #[test]
    fn rect_layout_matches_ndk() {
        assert_eq!(size_of::<ARect>(), 16);
        assert_eq!(align_of::<ARect>(), 4);
    }
}
