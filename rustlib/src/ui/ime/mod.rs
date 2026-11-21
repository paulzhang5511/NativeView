//! 输入法（IME）支持（SPEC-ime.md §2 `ui-ime` 模块）。
//!
//! # 范围
//! - [`focus`]：焦点编辑状态机（纯逻辑、表驱动单测）；
//! - [`text_input`]：`inputType`/`imeOptions` 的 Android 数值契约；
//! - [`manager`]：焦点效果排队（帧回调消费，IME 未用时零 JNI 开销）；
//! - 控件见 `ui/widgets/edit_text.rs`。
//!
//! # 边界
//! 本模块**不直接调用 JNI**：状态机与管理器只登记意图，由平台桥
//! （`platform::android::jni_bridge`）在帧回调内执行实际系统调用。

pub mod focus;
pub mod manager;
pub mod text_input;
