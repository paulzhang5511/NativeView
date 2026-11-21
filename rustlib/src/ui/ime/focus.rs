//! 输入焦点状态机（SPEC-ime.md §2「焦点编辑状态机」）。
//!
//! # 状态语义（对齐 Android `InputMethodManager` 生命周期）
//! - `Idle`：无焦点，IME 必定关闭；
//! - `Focused`：编辑控件获得焦点且请求输入，IME 已请求打开；
//! - `Editing`：IME 回推了非空 composing 区间（拼音预编辑中）；
//! - `Committed`：本次编辑确认落字，composing 已清除；
//!   下一次 composition 开始时回到 `Editing`，失焦回 `Idle`。
//!
//! # 不变量
//! 1. 任何状态下 `blur()`（失焦/键盘强收/窗口失焦/Activity 重建）一律回
//!    `Idle` 并清除 composing——IME 生命周期由系统异步驱动，任何「半开」
//!    状态都会导致后续文本回流错乱；
//! 2. 只有 `Focused`/`Editing`/`Committed` 会请求系统弹出键盘；
//!    `Idle` 状态下所有输入回调都必须被忽略（IME 未使用时零开销）；
//! 3. `composing` 区间始终满足 `start <= end`，且 `!text.is_empty()`
//!    时 `end <= text_len`（越界回推按协议错误丢弃）。

/// 焦点状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusState {
    /// 无焦点（IME 关闭）。
    Idle,
    /// 已聚焦（IME 已请求）。
    Focused,
    /// 预编辑中（composing 区间非空）。
    Editing,
    /// 已确认落字（composing 已清除）。
    Committed,
}

/// 焦点状态机：由输入事件驱动，产出「需要通知 Kotlin 的动作」。
///
/// # 设计
/// 纯状态机、无 IO、无全局状态——每条转换可独立单测；
/// JNI 副作用由调用方（平台桥）按 [`FocusEffect`] 执行。
#[derive(Debug)]
pub struct FocusMachine {
    state: FocusState,
    /// IME 期待的 inputType 位（Kotlin `EditorInfo.inputType`）。
    input_type: i32,
    /// IME 期待的 imeOptions 位（Kotlin `EditorInfo.imeOptions`）。
    ime_options: i32,
}

/// 状态机产出的副作用（由平台桥执行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusEffect {
    /// 无动作（IME 未使用路径，零 JNI）。
    None,
    /// 请求聚焦并弹出键盘。
    RequestShow,
    /// 收起键盘。
    RequestHide,
    /// 需要同步 `EditorInfo`（inputType/imeOptions 变更）。
    UpdateEditorInfo,
}

impl FocusMachine {
    /// 创建空闲状态机；`input_type`/`ime_options` 为默认编辑器配置。
    pub fn new(input_type: i32, ime_options: i32) -> Self {
        Self {
            state: FocusState::Idle,
            input_type,
            ime_options,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> FocusState {
        self.state
    }

    /// 当前 inputType。
    pub fn input_type(&self) -> i32 {
        self.input_type
    }

    /// 当前 imeOptions。
    pub fn ime_options(&self) -> i32 {
        self.ime_options
    }

    /// 是否持有焦点（IME 可能处于打开态）。
    pub fn is_focused(&self) -> bool {
        self.state != FocusState::Idle
    }

    /// 请求焦点（控件被点击 / 程序化 requestFocus）。
    ///
    /// # 返回
    /// `RequestShow`（首次聚焦）或 `UpdateEditorInfo`（已聚焦但配置变化）。
    pub fn focus(&mut self, input_type: i32, ime_options: i32) -> FocusEffect {
        let changed = self.input_type != input_type || self.ime_options != ime_options;
        self.input_type = input_type;
        self.ime_options = ime_options;
        match self.state {
            FocusState::Idle => {
                self.state = FocusState::Focused;
                FocusEffect::RequestShow
            }
            _ => {
                if changed {
                    FocusEffect::UpdateEditorInfo
                } else {
                    FocusEffect::None
                }
            }
        }
    }

    /// 失焦 / 键盘强收 / 窗口失焦 / Activity 重建。
    ///
    /// # 返回
    /// `RequestHide`（此前有焦点）或 `None`。
    pub fn blur(&mut self) -> FocusEffect {
        let was_focused = self.is_focused();
        self.state = FocusState::Idle;
        if was_focused {
            FocusEffect::RequestHide
        } else {
            FocusEffect::None
        }
    }

    /// IME 回推文本变化（含 composing 区间）。
    ///
    /// # 参数
    /// `composing`：当前 composing 区间 `(start, end)`；`None` = 无预编辑。
    /// `text_len`：Kotlin 侧全文长度（用于区间越界防御）。
    ///
    /// # 返回
    /// `None` = 状态不接受该事件（Idle，或越界协议错误）。
    pub fn on_text_changed(
        &mut self,
        composing: Option<(usize, usize)>,
        text_len: usize,
    ) -> Option<FocusState> {
        if self.state == FocusState::Idle {
            return None;
        }
        match composing {
            Some((start, end)) => {
                // 协议防御：区间荒谬则丢弃（不改变状态）。
                if start > end || end > text_len {
                    return None;
                }
                // 空区间等价于无 composing。
                if start == end {
                    self.state = FocusState::Committed;
                } else {
                    self.state = FocusState::Editing;
                }
            }
            None => {
                self.state = FocusState::Committed;
            }
        }
        Some(self.state)
    }

    /// 提交完成（回车/确认键/焦点转移前的落字）。
    pub fn commit(&mut self) -> FocusEffect {
        if self.state == FocusState::Idle {
            return FocusEffect::None;
        }
        self.state = FocusState::Committed;
        FocusEffect::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 默认配置常量（对应 Kotlin `InputType.TYPE_CLASS_TEXT` 等值）。
    const TEXT_TYPE: i32 = 1;
    const DONE_OPTIONS: i32 = 6;

    fn machine() -> FocusMachine {
        FocusMachine::new(TEXT_TYPE, DONE_OPTIONS)
    }

    /// 初始状态：Idle，无焦点。
    #[test]
    fn starts_idle() {
        let m = machine();
        assert_eq!(m.state(), FocusState::Idle);
        assert!(!m.is_focused());
    }

    /// Idle → Focused：请求键盘。
    #[test]
    fn focus_from_idle_requests_show() {
        let mut m = machine();
        assert_eq!(m.focus(TEXT_TYPE, DONE_OPTIONS), FocusEffect::RequestShow);
        assert_eq!(m.state(), FocusState::Focused);
        assert!(m.is_focused());
    }

    /// 已聚焦且配置未变：重复 focus 无副作用。
    #[test]
    fn focus_twice_noop() {
        let mut m = machine();
        m.focus(TEXT_TYPE, DONE_OPTIONS);
        assert_eq!(m.focus(TEXT_TYPE, DONE_OPTIONS), FocusEffect::None);
        assert_eq!(m.state(), FocusState::Focused);
    }

    /// 已聚焦但配置变更：同步 EditorInfo，不重复弹键盘。
    #[test]
    fn focus_with_changed_config_updates_editor_info() {
        let mut m = machine();
        m.focus(TEXT_TYPE, DONE_OPTIONS);
        assert_eq!(m.focus(2, DONE_OPTIONS), FocusEffect::UpdateEditorInfo);
        assert_eq!(m.input_type(), 2);
        assert_eq!(m.state(), FocusState::Focused);
    }

    /// Focused → Editing：composing 非空区间。
    #[test]
    fn composing_enters_editing() {
        let mut m = machine();
        m.focus(TEXT_TYPE, DONE_OPTIONS);
        assert_eq!(
            m.on_text_changed(Some((0, 3)), 3),
            Some(FocusState::Editing)
        );
        assert_eq!(m.state(), FocusState::Editing);
    }

    /// Editing → Committed：composing 空区间。
    #[test]
    fn empty_composing_commits() {
        let mut m = machine();
        m.focus(TEXT_TYPE, DONE_OPTIONS);
        m.on_text_changed(Some((0, 3)), 3);
        assert_eq!(
            m.on_text_changed(Some((2, 2)), 2),
            Some(FocusState::Committed)
        );
        assert_eq!(m.state(), FocusState::Committed);
    }

    /// 无 composing 的回推直接落字。
    #[test]
    fn no_composing_commits() {
        let mut m = machine();
        m.focus(TEXT_TYPE, DONE_OPTIONS);
        assert_eq!(m.on_text_changed(None, 5), Some(FocusState::Committed));
    }

    /// Committed 后可再次进入 Editing（连续输入）。
    #[test]
    fn committed_can_edit_again() {
        let mut m = machine();
        m.focus(TEXT_TYPE, DONE_OPTIONS);
        m.on_text_changed(None, 2);
        assert_eq!(m.state(), FocusState::Committed);
        assert_eq!(
            m.on_text_changed(Some((2, 4)), 4),
            Some(FocusState::Editing)
        );
    }

    /// 任意状态失焦 → Idle，且请求收起键盘。
    #[test]
    fn blur_from_every_state_returns_idle() {
        let cases = [
            FocusState::Idle,
            FocusState::Focused,
            FocusState::Editing,
            FocusState::Committed,
        ];
        for from in cases {
            let mut m = machine();
            // 构造到达目标状态。
            match from {
                FocusState::Idle => {}
                FocusState::Focused => {
                    m.focus(TEXT_TYPE, DONE_OPTIONS);
                }
                FocusState::Editing => {
                    m.focus(TEXT_TYPE, DONE_OPTIONS);
                    m.on_text_changed(Some((0, 2)), 2);
                }
                FocusState::Committed => {
                    m.focus(TEXT_TYPE, DONE_OPTIONS);
                    m.on_text_changed(None, 2);
                }
            }
            assert_eq!(m.state(), from, "setup state");
            let expected = if from == FocusState::Idle {
                FocusEffect::None
            } else {
                FocusEffect::RequestHide
            };
            assert_eq!(m.blur(), expected, "blur from {from:?}");
            assert_eq!(m.state(), FocusState::Idle, "after blur from {from:?}");
            assert!(!m.is_focused());
        }
    }

    /// Idle 状态忽略文本回推（IME 未使用时零开销不变量）。
    #[test]
    fn idle_ignores_text_events() {
        let mut m = machine();
        assert_eq!(m.on_text_changed(Some((0, 2)), 2), None);
        assert_eq!(m.on_text_changed(None, 2), None);
        assert_eq!(m.state(), FocusState::Idle);
    }

    /// 越界 composing 区间被丢弃且不改变状态（协议防御）。
    #[test]
    fn out_of_range_composing_rejected() {
        let mut m = machine();
        m.focus(TEXT_TYPE, DONE_OPTIONS);
        // end > text_len
        assert_eq!(m.on_text_changed(Some((0, 10)), 3), None);
        assert_eq!(m.state(), FocusState::Focused);
        // start > end
        assert_eq!(m.on_text_changed(Some((5, 2)), 10), None);
        assert_eq!(m.state(), FocusState::Focused);
    }

    /// commit：Idle 无动作，其余进入 Committed。
    #[test]
    fn commit_semantics() {
        let mut m = machine();
        assert_eq!(m.commit(), FocusEffect::None);
        assert_eq!(m.state(), FocusState::Idle);

        m.focus(TEXT_TYPE, DONE_OPTIONS);
        m.on_text_changed(Some((0, 2)), 2);
        assert_eq!(m.commit(), FocusEffect::None);
        assert_eq!(m.state(), FocusState::Committed);
    }

    /// 键盘强收/窗口失焦后重聚焦：状态干净（无残留 composing）。
    #[test]
    fn refocus_after_force_hide_is_clean() {
        let mut m = machine();
        m.focus(TEXT_TYPE, DONE_OPTIONS);
        m.on_text_changed(Some((0, 4)), 4);
        assert_eq!(m.state(), FocusState::Editing);
        // 键盘被系统强收。
        m.blur();
        assert_eq!(m.state(), FocusState::Idle);
        // 重新点击输入。
        assert_eq!(m.focus(TEXT_TYPE, DONE_OPTIONS), FocusEffect::RequestShow);
        assert_eq!(m.state(), FocusState::Focused, "no composing residue");
    }
}
