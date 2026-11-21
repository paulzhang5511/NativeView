//! KeyEvent：按键事件语义（PRD §5.2，Task 17）。
//!
//! V1 只建模 BACK/音量/方向键基础映射（PRD §5.2 按键行），
//! 不做全量键位与组合键。

/// 按键动作（Android `KeyEvent.ACTION_DOWN/UP`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    /// 按下。
    Down,
    /// 抬起。
    Up,
}

impl KeyAction {
    /// Android `ACTION_DOWN/UP` 常量值。
    pub const DOWN: i32 = 0;
    pub const UP: i32 = 1;

    /// 从 NDK 常量值解析。
    pub fn from_raw(raw: i32) -> Self {
        match raw {
            Self::DOWN => KeyAction::Down,
            _ => KeyAction::Up,
        }
    }
}

/// 键位（V1 子集，`AKEYCODE_*`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keycode {
    Back,
    VolumeUp,
    VolumeDown,
    VolumeMute,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    Enter,
    Menu,
    /// 未建模键位（透传语义，V1 忽略）。
    Other,
}

impl Keycode {
    /// 从 NDK `AKEYCODE_*` 值解析（值与 android/keycodes.h 一致）。
    pub fn from_raw(raw: i32) -> Self {
        match raw {
            4 => Keycode::Back,
            24 => Keycode::VolumeUp,
            25 => Keycode::VolumeDown,
            164 => Keycode::VolumeMute,
            19 => Keycode::DpadUp,
            20 => Keycode::DpadDown,
            21 => Keycode::DpadLeft,
            22 => Keycode::DpadRight,
            66 => Keycode::Enter,
            82 => Keycode::Menu,
            _ => Keycode::Other,
        }
    }

    /// 与 `platform::android::sys::keycodes` 常量一致的原始值
    /// （跨模块对照断言用）。
    pub fn raw(self) -> i32 {
        match self {
            Keycode::Back => 4,
            Keycode::VolumeUp => 24,
            Keycode::VolumeDown => 25,
            Keycode::VolumeMute => 164,
            Keycode::DpadUp => 19,
            Keycode::DpadDown => 20,
            Keycode::DpadLeft => 21,
            Keycode::DpadRight => 22,
            Keycode::Enter => 66,
            Keycode::Menu => 82,
            Keycode::Other => -1,
        }
    }
}

/// 按键事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    /// 动作。
    pub action: KeyAction,
    /// 键位。
    pub keycode: Keycode,
    /// 事件时间（与 MotionEvent 同基准）。
    pub event_time: i64,
    /// 按下起始时间（`getDownTime`）。
    pub down_time: i64,
    /// 重复计数（长按连发）。
    pub repeat_count: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 键位解析与常量对照（§5.1.8）。
    #[test]
    fn keycode_parse_matches_android() {
        assert_eq!(Keycode::from_raw(4), Keycode::Back);
        assert_eq!(Keycode::from_raw(24), Keycode::VolumeUp);
        assert_eq!(Keycode::from_raw(25), Keycode::VolumeDown);
        assert_eq!(Keycode::from_raw(19), Keycode::DpadUp);
        assert_eq!(Keycode::from_raw(66), Keycode::Enter);
        assert_eq!(Keycode::from_raw(999), Keycode::Other);
        // 原始值往返一致。
        for raw in [4, 24, 25, 164, 19, 20, 21, 22, 66, 82] {
            assert_eq!(Keycode::from_raw(raw).raw(), raw);
        }
    }

    /// 动作解析。
    #[test]
    fn key_action_parse() {
        assert_eq!(KeyAction::from_raw(0), KeyAction::Down);
        assert_eq!(KeyAction::from_raw(1), KeyAction::Up);
    }
}
