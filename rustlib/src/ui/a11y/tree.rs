//! 视图树 → 无障碍快照遍历（SPEC-accessibility.md §3「快照同步」）。
//!
//! # 与 `snapshot` 的分工
//! [`super::snapshot`] 只提供单节点累积能力，不认识容器类型；本模块负责
//! 沿 `Widget` 树结构推进，并在容器处递归。二者分离使得「节点数据」与
//! 「树遍历」可独立测试。
//!
//! # 文本提取
//! 节点文本来自控件类型而非 `View`（`View` 不持有内容）：
//! TextView/EditText 取自身文本，Button 取按钮标签，ImageView 取内容描述
//!（作为备选，真正播报优先用 contentDescription）。容器无文本。
//!
//! # 何时调用
//! 只在帧回调内、且 TalkBack 开启时调用——不得挂在 draw 路径
//!（SPEC-accessibility.md §3「禁止挂在 draw 路径上」）。

use crate::ui::a11y::node::A11yNode;
use crate::ui::a11y::snapshot::SnapshotBuilder;
use crate::ui::widgets::layout::{FrameLayout, LinearLayout};
use crate::ui::widgets::scroll_view::ScrollView;
use crate::ui::widgets::widget::Widget;

/// 从容器树根生成完整无障碍快照（先序遍历，扁平列表）。
///
/// # 返回
/// 节点列表；每项 `id` 等于其下标，`parent_id` 指回父项（根为 -1）。
pub fn snapshot_of(root: &FrameLayout) -> Vec<A11yNode> {
    let mut b = SnapshotBuilder::new();
    visit_container_root(root, &mut b, -1);
    b.finish()
}

/// 访问 FrameLayout 根（含自身 + 子项递归）。
fn visit_container_root(root: &FrameLayout, b: &mut SnapshotBuilder, parent_id: i32) -> i32 {
    let id = b.visit(&root.inner.view, "", parent_id).unwrap_or(-1);
    if id < 0 {
        // 根为 GONE：整棵树不进快照。
        return -1;
    }
    for child in &root.inner.children {
        visit_widget(child.child.as_ref(), b, id);
    }
    id
}

/// 访问任意控件（按具体类型取文本，容器递归）。
fn visit_widget(w: &dyn Widget, b: &mut SnapshotBuilder, parent_id: i32) {
    let view = w.view();
    let text = widget_text(w);
    let Some(id) = b.visit(view, text, parent_id) else {
        return;
    };
    // 容器递归（FrameLayout / LinearLayout / ScrollView）。
    if let Some(frame) = w.as_any().downcast_ref::<FrameLayout>() {
        for child in &frame.inner.children {
            visit_widget(child.child.as_ref(), b, id);
        }
        return;
    }
    if let Some(linear) = w.as_any().downcast_ref::<LinearLayout>() {
        for child in &linear.inner.children {
            visit_widget(child.child.as_ref(), b, id);
        }
        return;
    }
    if let Some(scroll) = w.as_any().downcast_ref::<ScrollView>() {
        // ScrollView 内容经其内部子项递归（滚动偏移不改变无障碍树结构，
        // 但影响可见 bounds——V1 不做滚动裁剪后 bounds 收缩，保持简单）。
        scroll.for_each_visible_child(|child| {
            visit_widget(child, b, id);
        });
    }
}

/// 提取控件文本（容器/无文本控件返回空串）。
fn widget_text(w: &dyn Widget) -> &str {
    if let Some(tv) = w
        .as_any()
        .downcast_ref::<crate::ui::widgets::text_view::TextView>()
    {
        return tv.text();
    }
    if let Some(et) = w
        .as_any()
        .downcast_ref::<crate::ui::widgets::edit_text::EditText>()
    {
        return et.text();
    }
    if let Some(btn) = w
        .as_any()
        .downcast_ref::<crate::ui::widgets::button::Button>()
    {
        return btn.text();
    }
    ""
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::a11y::node::A11yRole;
    use crate::ui::measure_spec::{MeasureSpec, Mode};
    use crate::ui::view::Gravity;
    use crate::ui::view_group::MarginLayoutParams;
    use crate::ui::widgets::TextView;

    fn spec(w: u32, h: u32) -> (MeasureSpec, MeasureSpec) {
        (
            MeasureSpec::make(Mode::Exactly, w),
            MeasureSpec::make(Mode::Exactly, h),
        )
    }

    /// 空 FrameLayout：只有根节点。
    #[test]
    fn empty_root_single_node() {
        let mut root = FrameLayout::new();
        let (ws, hs) = spec(100, 100);
        root.measure(ws, hs);
        root.layout(0.0, 0.0, 100.0, 100.0);
        let nodes = snapshot_of(&root);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].parent_id, -1);
    }

    /// 层级：FrameLayout > 两个子 View。
    #[test]
    fn nested_children() {
        let mut root = FrameLayout::new();
        let mut a = crate::ui::view::View::new();
        a.set_a11y_role(A11yRole::Button);
        let mut b = crate::ui::view::View::new();
        b.set_a11y_role(A11yRole::Text);
        root.add_child(a, MarginLayoutParams::new(40, 40), Gravity::TOP);
        root.add_child(b, MarginLayoutParams::new(40, 40), Gravity::TOP);
        let (ws, hs) = spec(100, 100);
        root.measure(ws, hs);
        root.layout(0.0, 0.0, 100.0, 100.0);
        let nodes = snapshot_of(&root);
        assert_eq!(nodes.len(), 3, "root + 2 children");
        assert_eq!(nodes[0].parent_id, -1);
        assert_eq!(nodes[1].parent_id, 0);
        assert_eq!(nodes[2].parent_id, 0);
        assert_eq!(nodes[1].class_name, "android.widget.Button");
        assert_eq!(nodes[2].class_name, "android.widget.TextView");
    }

    /// TextView 文本进入快照。
    #[test]
    fn text_view_text_captured() {
        let mut root = FrameLayout::new();
        let mut tv = TextView::new();
        tv.set_text("Hello a11y");
        tv.view.set_a11y_role(A11yRole::Text);
        root.add_widget(Box::new(tv), MarginLayoutParams::new(-1, 40), Gravity::TOP);
        let (ws, hs) = spec(100, 100);
        root.measure(ws, hs);
        root.layout(0.0, 0.0, 100.0, 100.0);
        let nodes = snapshot_of(&root);
        // 无字体时 TextView 文本仍保留在字段中。
        assert_eq!(nodes[1].text, "Hello a11y");
    }

    /// GONE 子项不进快照（含其子树）。
    #[test]
    fn gone_subtree_skipped() {
        let mut root = FrameLayout::new();
        let mut inner = FrameLayout::new();
        inner
            .inner
            .view
            .set_visibility(crate::ui::view::Visibility::Gone);
        inner.add_child(
            crate::ui::view::View::new(),
            MarginLayoutParams::new(10, 10),
            Gravity::TOP,
        );
        root.add_widget(
            Box::new(inner),
            MarginLayoutParams::new(50, 50),
            Gravity::TOP,
        );
        let (ws, hs) = spec(100, 100);
        root.measure(ws, hs);
        root.layout(0.0, 0.0, 100.0, 100.0);
        let nodes = snapshot_of(&root);
        assert_eq!(nodes.len(), 1, "only root survives");
    }
}
