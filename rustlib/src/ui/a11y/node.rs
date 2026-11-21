//! 无障碍节点属性（SPEC-accessibility.md §3）。
//!
//! # 语义映射（对齐 Android `AccessibilityNodeInfo`）
//! Rust 视图树的每个节点在影子树中对应一个无障碍节点，节点对外暴露的
//! 信息**只有**以下子集（规格明确的能力边界，不追求 `AccessibilityNodeInfo`
//! 全集）：
//! - 层级（父子关系，由快照遍历顺序表达）；
//! - `bounds_in_screen`（屏幕像素矩形）；
//! - 文本 / 内容描述；
//! - `clickable` / `enabled` / `checked`；
//! - 角色（映射成 TalkBack 播报用的 `className`）。
//!
//! **禁止**携带像素或图像数据（PRD §6.1/§6.2 的 JNI 边界铁律）。
//!
//! # 为什么角色独立于控件类型
//! 控件类型（TextView/Button/...）是 Rust 侧的绘制实现细节；无障碍角色
//! 是对使用者的语义承诺。二者刻意解耦：例如可点击的 `TextView` 应当被
//! 播报为按钮而非文本，此时把角色设为 [`A11yRole::Button`] 而不必换控件。

/// 无障碍角色（映射到 Android `className` 与播报语义）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum A11yRole {
    /// 普通容器/文本（`android.view.View`）。
    #[default]
    None,
    /// 文本（`android.widget.TextView`）。
    Text,
    /// 按钮（`android.widget.Button`，TalkBack 会播报「按钮」）。
    Button,
    /// 图像（`android.widget.ImageView`）。
    Image,
    /// 可编辑文本框（`android.widget.EditText`）。
    EditText,
    /// 复选框（`android.widget.CheckBox`，TalkBack 播报「已选中/未选中」）。
    Checkbox,
    /// 单选按钮（`android.widget.RadioButton`）。
    RadioButton,
    /// 开关（`android.widget.Switch`）。
    Switch,
    /// 滑块（`android.widget.SeekBar`，TalkBack 播报当前进度）。
    Slider,
}

impl A11yRole {
    /// 映射为 Android `className`（Kotlin 侧直接塞进 `AccessibilityNodeInfo`）。
    ///
    /// # 未知角色
    /// 返回 `android.view.View`——保证任何角色都能被系统识别为合法节点，
    /// 不会因 className 非法导致节点被丢弃。
    pub fn class_name(self) -> &'static str {
        match self {
            A11yRole::Text => "android.widget.TextView",
            A11yRole::Button => "android.widget.Button",
            A11yRole::Image => "android.widget.ImageView",
            A11yRole::EditText => "android.widget.EditText",
            A11yRole::Checkbox => "android.widget.CheckBox",
            A11yRole::RadioButton => "android.widget.RadioButton",
            A11yRole::Switch => "android.widget.Switch",
            A11yRole::Slider => "android.widget.SeekBar",
            A11yRole::None => "android.view.View",
        }
    }

    /// 是否需要把自身视为「可聚焦的叶子」（供 Kotlin 侧决定重点播报顺序）。
    ///
    /// # 语义
    /// 有角色即为语义节点；纯容器不参与 TalkBack 聚焦遍历。
    pub fn is_semantic(self) -> bool {
        self != A11yRole::None
    }
}

/// 单个无障碍节点的快照数据（跨 JNI 传输的最小单元）。
#[derive(Clone, Debug, PartialEq)]
pub struct A11yNode {
    /// 节点 id（快照内唯一；由遍历序号生成，仅本次快照有效）。
    pub id: i32,
    /// 父节点 id（-1 = 根）。
    pub parent_id: i32,
    /// 屏幕坐标矩形 `(left, top, right, bottom)`（px）。
    pub bounds: (i32, i32, i32, i32),
    /// 播报文本（空串 = 无文本）。
    pub text: String,
    /// 内容描述（优先级高于 text；空串 = 无）。
    pub content_description: String,
    /// 角色对应的 Android className。
    pub class_name: &'static str,
    /// 是否可点击。
    pub clickable: bool,
    /// 是否可用。
    pub enabled: bool,
    /// 是否被选中（复选框语义；V1 恒 false）。
    pub checked: bool,
    /// 是否可见（GONE 的节点不进快照）。
    pub visible: bool,
    /// 宿主动作标签（-1 = 点击无业务含义）。
    ///
    /// 由视图属性透传，框架不理解其语义，只在 `ACTION_CLICK` 回推时原样返回。
    pub action_tag: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 角色到 className 的映射固定（Kotlin 侧依赖这些字面量）。
    #[test]
    fn role_class_names() {
        assert_eq!(A11yRole::Text.class_name(), "android.widget.TextView");
        assert_eq!(A11yRole::Button.class_name(), "android.widget.Button");
        assert_eq!(A11yRole::Image.class_name(), "android.widget.ImageView");
        assert_eq!(A11yRole::EditText.class_name(), "android.widget.EditText");
        assert_eq!(A11yRole::None.class_name(), "android.view.View");
    }

    /// 只有 None 不是语义节点。
    #[test]
    fn semantic_roles() {
        assert!(!A11yRole::None.is_semantic());
        for role in [
            A11yRole::Text,
            A11yRole::Button,
            A11yRole::Image,
            A11yRole::EditText,
        ] {
            assert!(role.is_semantic(), "{role:?} must be semantic");
        }
    }

    /// 默认角色为 None（纯容器不播报）。
    #[test]
    fn default_role_is_none() {
        assert_eq!(A11yRole::default(), A11yRole::None);
    }
}
