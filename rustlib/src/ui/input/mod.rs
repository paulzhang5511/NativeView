//! 输入模块：Android MotionEvent/KeyEvent 语义（PRD §5.2，Task 17）。
//!
//! # 设计
//! - 事件模型为平台无关纯数据：Android 真机路径由 platform 层从
//!   `AInputEvent` 解码填充，主机单测直接构造验证语义；
//! - 常量与 Android `MotionEvent.ACTION_*` / `KeyEvent` 完全一致
//!   （PRD §5.1.8 常量对齐），与 `platform::android::sys` 常量交叉断言。

pub mod coords;
pub mod dispatch;
pub mod key_event;
pub mod motion_event;

pub use coords::{LocalPoint, hit_test, to_local};
pub use key_event::{KeyAction, KeyEvent, Keycode};
pub use motion_event::{Action, HistoricalSample, MotionEvent, PointerCoords};
