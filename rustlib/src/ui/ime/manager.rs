//! 焦点管理器：进程级焦点请求排队（SPEC-ime.md「回推合并进帧回调」）。
//!
//! # 为什么需要排队
//! IME 副作用（`requestFocus` + `showSoftInput` / `hideSoftInput` / 更新
//! `EditorInfo`）**只能**在 UI 线程且必须在帧回调之外的安全时机执行：
//! 触摸分发链内部直接调 JNI 会在手势状态机中途重入系统输入法，导致
//! 焦点/键盘状态与视图树不一致。因此这里只做「登记意图」，由平台层
//! 在帧回调（Choreographer 近似 poll 超时）里调 [`take_effect`] 消费。
//!
//! # 合并语义
//! 同一帧内多次请求按「最后一次覆盖」合并——与 Android
//! `InputMethodManager` 的 `showSoftInput` 幂等性一致；
//! show 与 hide 同时出现时以**最后登记者**为准（用户操作先后序）。
//!
//! # 零开销不变量（SPEC-ime.md 成功标准 5）
//! IME 从未被请求时，本模块只有两个原子读，不产生任何 JNI 调用。

use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::ui::ime::text_input::{ImeOptions, InputType};

/// 待执行效果（合并后的单条意图）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PendingEffect {
    /// 无待执行动作。
    None,
    /// 聚焦并弹出键盘（携带编辑器配置与当前文本）。
    ///
    /// # 为什么带 text
    /// 隐形 EditText 是 IME 的真实文本源；宿主已渲染的文本（Rust 侧）
    /// 必须在弹出前灌入，否则重聚焦会从空文本开始，导致已有输入丢失。
    Show {
        input_type: i32,
        ime_options: i32,
        text: String,
    },
    /// 收起键盘。
    Hide,
}

/// 效果槽：0 = 无，1 = Show，2 = Hide（原子标记，避免锁竞争）。
static KIND: AtomicU8 = AtomicU8::new(0);

/// Show 参数槽（仅 KIND==1 时有效）。
static PARAMS: Mutex<Option<(i32, i32, String)>> = Mutex::new(None);

/// 请求输入焦点并弹出键盘（帧回调内消费）。
pub fn request_focus(input_type: InputType, ime_options: ImeOptions, text: String) {
    *PARAMS.lock().unwrap_or_else(|e| e.into_inner()) =
        Some((input_type.to_android(), ime_options.to_android(), text));
    KIND.store(1, Ordering::Release);
}

/// 请求收起键盘（失焦/确认后调用）。
pub fn request_blur() {
    KIND.store(2, Ordering::Release);
}

/// 取走待执行效果（由平台层每帧调用一次）。
///
/// # 返回
/// 合并后的单条意图；无待执行动作返回 [`PendingEffect::None`]。
pub fn take_effect() -> PendingEffect {
    match KIND.swap(0, Ordering::Acquire) {
        1 => {
            let params = PARAMS.lock().unwrap_or_else(|e| e.into_inner()).take();
            match params {
                Some((input_type, ime_options, text)) => PendingEffect::Show {
                    input_type,
                    ime_options,
                    text,
                },
                // 参数缺失（理论上不可能）：退化为无动作，避免下发脏配置。
                None => PendingEffect::None,
            }
        }
        2 => {
            // hide 覆盖先前 show 的参数，清掉防泄漏。
            *PARAMS.lock().unwrap_or_else(|e| e.into_inner()) = None;
            PendingEffect::Hide
        }
        _ => PendingEffect::None,
    }
}

/// 清空待执行效果（测试与 Activity 重建时用）。
pub fn clear() {
    KIND.store(0, Ordering::Release);
    *PARAMS.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试串行化：全局状态，多个测试必须顺序执行。
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        let g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear();
        g
    }

    /// 初始无待执行动作（零开销路径）。
    #[test]
    fn starts_empty() {
        let _g = serial();
        assert_eq!(take_effect(), PendingEffect::None);
    }

    /// 请求聚焦：Show 携带 Android 位值。
    #[test]
    fn request_focus_emits_show() {
        let _g = serial();
        request_focus(InputType::Number, ImeOptions::Search, String::new());
        assert_eq!(
            take_effect(),
            PendingEffect::Show {
                input_type: 0x2,
                ime_options: 3,
                text: String::new()
            }
        );
        // 取走后清空。
        assert_eq!(take_effect(), PendingEffect::None);
    }

    /// 同帧多次请求：最后一次覆盖。
    #[test]
    fn last_request_wins() {
        let _g = serial();
        request_focus(InputType::Text, ImeOptions::Done, String::new());
        request_focus(InputType::Password, ImeOptions::Next, String::new());
        assert_eq!(
            take_effect(),
            PendingEffect::Show {
                input_type: 0x81,
                ime_options: 5,
                text: String::new()
            }
        );
    }

    /// hide 覆盖先前的 show（用户随后收起键盘）。
    #[test]
    fn hide_overrides_show() {
        let _g = serial();
        request_focus(InputType::Text, ImeOptions::Done, String::new());
        request_blur();
        assert_eq!(take_effect(), PendingEffect::Hide);
        // 参数槽已清空，不留脏配置。
        assert_eq!(take_effect(), PendingEffect::None);
    }

    /// show 覆盖先前的 hide。
    #[test]
    fn show_overrides_hide() {
        let _g = serial();
        request_blur();
        request_focus(InputType::Text, ImeOptions::Done, String::new());
        assert_eq!(
            take_effect(),
            PendingEffect::Show {
                input_type: 0x1,
                ime_options: 6,
                text: String::new()
            }
        );
    }

    /// clear 后无残留。
    #[test]
    fn clear_removes_pending() {
        let _g = serial();
        request_focus(InputType::Text, ImeOptions::Done, String::new());
        clear();
        assert_eq!(take_effect(), PendingEffect::None);
    }
}
