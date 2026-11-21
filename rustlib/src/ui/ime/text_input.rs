//! 输入属性：`inputType` / `imeOptions`（SPEC-ime.md §1「动态构造 EditorInfo」）。
//!
//! # 数值契约
//! 枚举到 `int` 的映射**必须**与 Android 常量逐位一致——Kotlin 侧直接把这些
//! 整数塞进 `EditorInfo.inputType` / `EditorInfo.imeOptions`，任何偏差都会让
//! 键盘面板类型错误（例如数字键盘不出现、回车键无 Done 图标）。
//!
//! 参考：`android.text.InputType` / `android.view.inputmethod.EditorInfo`。

/// 输入类型（`EditorInfo.inputType`）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputType {
    /// 普通文本：`TYPE_CLASS_TEXT` (0x00000001)。
    #[default]
    Text,
    /// 数字：`TYPE_CLASS_NUMBER` (0x00000002)。
    Number,
    /// 密码：`TYPE_CLASS_TEXT | TYPE_TEXT_VARIATION_PASSWORD` (0x00000081)。
    Password,
}

impl InputType {
    /// 转 Android `EditorInfo.inputType` 位值。
    pub fn to_android(self) -> i32 {
        match self {
            // InputType.TYPE_CLASS_TEXT
            InputType::Text => 0x0000_0001,
            // InputType.TYPE_CLASS_NUMBER
            InputType::Number => 0x0000_0002,
            // TYPE_CLASS_TEXT | TYPE_TEXT_VARIATION_PASSWORD
            InputType::Password => 0x0000_0081,
        }
    }
}

/// 回车键语义（`EditorInfo.imeOptions`）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImeOptions {
    /// 完成：`IME_ACTION_DONE` (6)。
    #[default]
    Done,
    /// 下一项：`IME_ACTION_NEXT` (5)。
    Next,
    /// 搜索：`IME_ACTION_SEARCH` (3)。
    Search,
}

impl ImeOptions {
    /// 转 Android `EditorInfo.imeOptions` 位值。
    pub fn to_android(self) -> i32 {
        match self {
            ImeOptions::Done => 6,
            ImeOptions::Next => 5,
            ImeOptions::Search => 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 数值契约：与 Android 常量逐位一致。
    #[test]
    fn android_constants_match() {
        assert_eq!(InputType::Text.to_android(), 0x1, "TYPE_CLASS_TEXT");
        assert_eq!(InputType::Number.to_android(), 0x2, "TYPE_CLASS_NUMBER");
        assert_eq!(
            InputType::Password.to_android(),
            0x81,
            "TYPE_CLASS_TEXT|TYPE_TEXT_VARIATION_PASSWORD"
        );
        assert_eq!(ImeOptions::Done.to_android(), 6, "IME_ACTION_DONE");
        assert_eq!(ImeOptions::Next.to_android(), 5, "IME_ACTION_NEXT");
        assert_eq!(ImeOptions::Search.to_android(), 3, "IME_ACTION_SEARCH");
    }

    /// 默认值：文本 + Done。
    #[test]
    fn defaults() {
        assert_eq!(InputType::default(), InputType::Text);
        assert_eq!(ImeOptions::default(), ImeOptions::Done);
    }
}
