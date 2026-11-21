//! `android/keycodes.h` 的手写绑定（NDK 27.3.13750724）。
//!
//! V1 仅覆盖 PRD §5.2 要求的按键子集（返回键、方向键、确认键），
//! 不做全量 300+ 键位枚举。

/// `AKEYCODE_*` 子集（头文件 enum AKEYCODE）。
pub const AKEYCODE_BACK: i32 = 4;
pub const AKEYCODE_VOLUME_UP: i32 = 24;
pub const AKEYCODE_VOLUME_DOWN: i32 = 25;
pub const AKEYCODE_DPAD_UP: i32 = 19;
pub const AKEYCODE_DPAD_DOWN: i32 = 20;
pub const AKEYCODE_DPAD_LEFT: i32 = 21;
pub const AKEYCODE_DPAD_RIGHT: i32 = 22;
pub const AKEYCODE_ENTER: i32 = 66;
pub const AKEYCODE_MENU: i32 = 82;
pub const AKEYCODE_VOLUME_MUTE: i32 = 164;

#[cfg(test)]
mod tests {
    use super::*;

    /// 键位常量与 NDK 头文件逐一对照（错误值会导致按键路由错乱）。
    #[test]
    fn keycode_constants_match_ndk() {
        assert_eq!(AKEYCODE_BACK, 4);
        assert_eq!(AKEYCODE_DPAD_UP, 19);
        assert_eq!(AKEYCODE_DPAD_DOWN, 20);
        assert_eq!(AKEYCODE_DPAD_LEFT, 21);
        assert_eq!(AKEYCODE_DPAD_RIGHT, 22);
        assert_eq!(AKEYCODE_ENTER, 66);
        assert_eq!(AKEYCODE_MENU, 82);
    }
}
