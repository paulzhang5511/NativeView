//! 无障碍桥接：把当前屏的可点区域导出成 `(Rect, label, tag)` 三元组，
//! 并把外部点击按 tag 反解回 `Action`。
//!
//! 约束：tag 必须落在 `A11Y_TAG_BASE` 之上且能 `Action::from_code` 解回，
//! 否则 TalkBack 点上去会静默失败——`a11y_click_rejects_unknown_tag` 守护这一点。

use crate::gfx::scene::Rect;

use super::{A11Y_TAG_BASE, Action, Inkflow};

impl Inkflow {
    /// 当前屏幕的无障碍可点目标：`(屏幕矩形, 中文描述, 动作标签)`。
    ///
    /// # 用途
    /// 整页是 Scene 直绘，View 树本身没有语义；这里产出与视觉按钮一一对应的
    /// 透明节点，由结构树装配后交给 TalkBack。坐标是屏幕绝对坐标（含上方 inset），
    /// 装配时再换算成相对内容区顶部的偏移。
    pub(crate) fn a11y_nodes(&self) -> Vec<(Rect, String, i32)> {
        // 必须与手势命中/绘制共用同一份 `layout()`：任何第二份几何计算
        // 都会让 TalkBack 焦点框与真实可点区域错位。
        let l = self.layout();
        l.hits(self.screen, self.exported.is_some())
            .into_iter()
            .map(|(rect, a)| (rect, self.a11y_label(a), A11Y_TAG_BASE + a.code()))
            .collect()
    }
    /// 无障碍点击入口：按标签还原动作并执行，返回是否发生了状态变更。
    ///
    /// 非法标签一律返回 false，避免宿主传入脏值改变页面状态。
    pub(crate) fn perform_a11y(&mut self, tag: i32) -> bool {
        let Some(code) = tag.checked_sub(A11Y_TAG_BASE) else {
            return false;
        };
        let Some(action) = Action::from_code(code) else {
            return false;
        };
        // TalkBack 点击与手势点击必须有一致的按压反馈。
        let l = self.layout();
        if let Some((rect, _)) = l
            .hits(self.screen, self.exported.is_some())
            .into_iter()
            .find(|(_, a)| *a == action)
        {
            self.press = Some((rect, self.clock, self.screen));
        }
        self.perform(action);
        true
    }
    /// 动作的中文无障碍描述。
    pub(super) fn a11y_label(&self, action: Action) -> String {
        match action {
            Action::CreateNew => "新建画布".into(),
            Action::PickSize(i) => format!("画布尺寸 {}", i + 1),
            Action::PickPaper(i) => format!("纸色 {}", i + 1),
            Action::StartDrawing => "创建画布".into(),
            Action::OpenSample(i) => format!("示例作品 {}", i + 1),
            Action::Back => "返回".into(),
            Action::Undo => "撤销".into(),
            Action::Redo => "重做".into(),
            Action::ToggleEraser => {
                if self.is_eraser() {
                    "关闭橡皮".into()
                } else {
                    "橡皮".into()
                }
            }
            Action::OpenBrushSheet => "笔刷面板".into(),
            Action::CloseBrushSheet => "完成".into(),
            Action::PickPreset(i) => format!("笔刷 {}", i + 1),
            Action::PickColor(i) => format!("颜色 {}", i + 1),
            Action::OpenExport => "导出 PNG".into(),
            Action::DoExport => "保存".into(),
            Action::Finish => "返回作品库".into(),
            Action::DragSlider(i) => ["粗细", "不透明度", "流量", "平滑"][i.min(3)].into(),
            Action::PickTool(i) => ["画笔", "直线", "橡皮", "取色", "移动画布"][i.min(4)].into(),
            Action::ToggleLayer => "切换图层".into(),
            Action::QuickBrush(i) => ["铅笔", "软笔", "喷枪", "水彩"][i.min(3)].into(),
            Action::DragSize => "画布尺寸".into(),
            Action::PickFilter(i) => ["全部", "收藏", "草稿", "已导出"][i.min(3)].into(),
            Action::DragDefault(i) => ["默认笔刷", "默认不透明度"][i.min(1)].into(),
            Action::PickFormat(i) => ["格式 PNG", "格式 JPEG", "格式 PDF"][i.min(2)].into(),
        }
    }
}
