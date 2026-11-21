//! 快照构建器：把访问到的视图节点累积成扁平节点列表。
//!
//! # 为什么扁平化
//! Android `AccessibilityNodeProvider` 需要按 id 随机访问节点，并用
//! `parentId` 表达层级。扁平的 `Vec<A11yNode>` 正好对应这个接口：
//! 每项的 `id` 是数组下标，`parent_id` 指回父项——Kotlin 侧无需再解析树。
//!
//! # id 分配规则
//! 按**先序遍历**顺序分配 0..n-1，父节点必定先于子节点出现，
//! 因此构建过程只需记住「当前父 id」而不必后处理。
//!
//! # 可见性
//! `Visibility::Gone` 的节点连同其子树整体跳过（Android 语义：
//! GONE 既不显示也不参与无障碍遍历）；`Invisible` 仍保留在快照中
//!（占位但不可见，TalkBack 会跳过播报但仍可定位）。

use crate::ui::a11y::node::{A11yNode, A11yRole};
use crate::ui::view::{View, Visibility};

/// 快照构建器：累积节点并按需分配 id。
#[derive(Debug, Default)]
pub struct SnapshotBuilder {
    nodes: Vec<A11yNode>,
}

impl SnapshotBuilder {
    /// 创建空构建器。
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    /// 已收集的节点数。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// 是否为空（无任何可访问节点）。
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// 访问一个视图节点，返回其在快照中的 id。
    ///
    /// # 参数
    /// `view`：视图（提供几何/可见性/可点击/无障碍属性）；
    /// `text`：控件自身文本（容器传空串）；
    /// `parent_id`：父节点 id（根传 -1）。
    ///
    /// # 返回
    /// `Some(id)` = 已加入快照；`None` = 子树整体跳过（GONE）。
    pub fn visit(&mut self, view: &View, text: &str, parent_id: i32) -> Option<i32> {
        // GONE：自身与子树都不进快照（Android 语义）。
        if view.visibility() == Visibility::Gone {
            return None;
        }
        let id = self.nodes.len() as i32;
        let role = view.a11y_role();
        self.nodes.push(A11yNode {
            id,
            parent_id,
            bounds: (
                view.left().round() as i32,
                view.top().round() as i32,
                view.right().round() as i32,
                view.bottom().round() as i32,
            ),
            // 无显式内容描述时退回控件文本（Android 同语义）。
            text: text.to_string(),
            content_description: view.content_description().to_string(),
            class_name: role.class_name(),
            clickable: view.is_clickable(),
            enabled: view.is_enabled(),
            checked: view.a11y_checked(),
            visible: view.visibility() == Visibility::Visible,
            action_tag: view.a11y_action_tag(),
        });
        Some(id)
    }

    /// 取出节点列表（消费构建器）。
    pub fn finish(self) -> Vec<A11yNode> {
        self.nodes
    }
}

/// 单节点角色的语义判定辅助（供调用方决定是否需要显式设置角色）。
///
/// # 用于
/// 装配阶段按控件类型推断默认角色：TextView → Text、Button → Button、
/// ImageView → Image、EditText → EditText，纯容器保持 None。
pub fn default_role_for_text_control(clickable: bool) -> A11yRole {
    if clickable {
        // 可点击的文本在语义上就是按钮（TalkBack 播报「按钮」）。
        A11yRole::Button
    } else {
        A11yRole::Text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view_at(l: f32, t: f32, r: f32, b: f32) -> View {
        let mut v = View::new();
        v.layout(l, t, r, b);
        v
    }

    /// 空构建器状态。
    #[test]
    fn empty_builder() {
        let b = SnapshotBuilder::new();
        assert!(b.is_empty());
        assert_eq!(b.len(), 0);
    }

    /// 单节点：id=0、父为 -1、bounds 四舍五入。
    #[test]
    fn single_node() {
        let mut b = SnapshotBuilder::new();
        let v = view_at(1.4, 2.6, 100.0, 50.0);
        let id = b.visit(&v, "hello", -1).expect("visible");
        assert_eq!(id, 0);
        let nodes = b.finish();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].bounds, (1, 3, 100, 50));
        assert_eq!(nodes[0].text, "hello");
        assert_eq!(nodes[0].parent_id, -1);
    }

    /// GONE 节点整体跳过。
    #[test]
    fn gone_skipped() {
        let mut b = SnapshotBuilder::new();
        let mut v = view_at(0.0, 0.0, 10.0, 10.0);
        v.set_visibility(Visibility::Gone);
        assert!(b.visit(&v, "x", -1).is_none());
        assert!(b.is_empty());
    }

    /// INVISIBLE 仍在快照中但标记不可见。
    #[test]
    fn invisible_present_but_marked() {
        let mut b = SnapshotBuilder::new();
        let mut v = view_at(0.0, 0.0, 10.0, 10.0);
        v.set_visibility(Visibility::Invisible);
        let id = b.visit(&v, "x", -1).expect("invisible is still visited");
        assert_eq!(id, 0);
        assert!(!b.finish()[0].visible);
    }

    /// 先序 id 分配与父子关系。
    #[test]
    fn preorder_ids_and_parents() {
        let mut b = SnapshotBuilder::new();
        let root = view_at(0.0, 0.0, 100.0, 100.0);
        let root_id = b.visit(&root, "", -1).unwrap();
        let child = view_at(0.0, 0.0, 50.0, 50.0);
        let c1 = b.visit(&child, "a", root_id).unwrap();
        let c2 = b.visit(&child, "b", root_id).unwrap();
        assert_eq!((root_id, c1, c2), (0, 1, 2));
        let nodes = b.finish();
        assert_eq!(nodes[1].parent_id, root_id);
        assert_eq!(nodes[2].parent_id, root_id);
    }

    /// 语义属性透传到快照。
    #[test]
    fn semantic_props_copied() {
        let mut b = SnapshotBuilder::new();
        let mut v = view_at(0.0, 0.0, 10.0, 10.0);
        v.set_a11y_role(A11yRole::Button);
        v.set_clickable(true);
        v.set_content_description("确定");
        v.set_a11y_checked(true);
        b.visit(&v, "OK", -1);
        let n = &b.finish()[0];
        assert_eq!(n.class_name, "android.widget.Button");
        assert!(n.clickable);
        assert_eq!(n.content_description, "确定");
        assert!(n.checked);
    }

    /// 可点击文本默认推断为按钮角色。
    #[test]
    fn clickable_text_role_inference() {
        assert_eq!(default_role_for_text_control(true), A11yRole::Button);
        assert_eq!(default_role_for_text_control(false), A11yRole::Text);
    }
}
