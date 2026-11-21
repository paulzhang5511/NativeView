//! `android/input.h` 的手写绑定（NDK 27.3.13750724）。
//!
//! V1 只绑定触摸/按键路径所需符号（PRD §6.4.2 最小必要集）。

use core::ffi::c_int;

#[cfg(target_os = "android")]
use super::looper::ALooper;

/// 不透明输入事件句柄（借用语义；事件由 `AInputQueue_getEvent` 交出、
/// 由 `AInputQueue_finishEvent` 归还，Rust 侧不构造、不释放）。
#[repr(C)]
pub struct AInputEvent {
    _private: [u8; 0],
}

/// 不透明输入队列句柄（借用语义；队列生命周期由框架管理）。
#[repr(C)]
pub struct AInputQueue {
    _private: [u8; 0],
}

// AINPUT_EVENT_TYPE_*（头文件 enum）。
pub const AINPUT_EVENT_TYPE_KEY: c_int = 1;
pub const AINPUT_EVENT_TYPE_MOTION: c_int = 2;

// AKEY_EVENT_ACTION_*（头文件 enum）。
pub const AKEY_EVENT_ACTION_DOWN: c_int = 0;
pub const AKEY_EVENT_ACTION_UP: c_int = 1;
pub const AKEY_EVENT_ACTION_MULTIPLE: c_int = 2;

// AMOTION_EVENT_ACTION_*（头文件 enum，PRD §5.2 要求全常量对齐）。
pub const AMOTION_EVENT_ACTION_MASK: c_int = 0xff;
pub const AMOTION_EVENT_ACTION_POINTER_INDEX_SHIFT: c_int = 8;
pub const AMOTION_EVENT_ACTION_POINTER_INDEX_MASK: c_int = 0xff00;
pub const AMOTION_EVENT_ACTION_DOWN: c_int = 0;
pub const AMOTION_EVENT_ACTION_UP: c_int = 1;
pub const AMOTION_EVENT_ACTION_MOVE: c_int = 2;
pub const AMOTION_EVENT_ACTION_CANCEL: c_int = 3;
pub const AMOTION_EVENT_ACTION_OUTSIDE: c_int = 4;
pub const AMOTION_EVENT_ACTION_POINTER_DOWN: c_int = 5;
pub const AMOTION_EVENT_ACTION_POINTER_UP: c_int = 6;
pub const AMOTION_EVENT_ACTION_HOVER_MOVE: c_int = 7;
pub const AMOTION_EVENT_ACTION_SCROLL: c_int = 8;
pub const AMOTION_EVENT_ACTION_HOVER_ENTER: c_int = 9;
pub const AMOTION_EVENT_ACTION_HOVER_EXIT: c_int = 10;
pub const AMOTION_EVENT_ACTION_BUTTON_PRESS: c_int = 11;
pub const AMOTION_EVENT_ACTION_BUTTON_RELEASE: c_int = 12;

// AMOTION_EVENT_AXIS_*（V1 用到的轴）。
pub const AMOTION_EVENT_AXIS_X: c_int = 0;
pub const AMOTION_EVENT_AXIS_Y: c_int = 1;
pub const AMOTION_EVENT_AXIS_PRESSURE: c_int = 2;

#[cfg(target_os = "android")]
unsafe extern "C" {
    // AInputEvent_*（事件只读访问；release 为 __INTRODUCED_IN(31)，调用前门控）。
    pub fn AInputEvent_getType(event: *const AInputEvent) -> i32;
    pub fn AInputEvent_getDeviceId(event: *const AInputEvent) -> i32;
    pub fn AInputEvent_getSource(event: *const AInputEvent) -> i32;
    /// __INTRODUCED_IN(31)：minSdk=29 下必须先判 `sdkVersion >= 31` 再调用。
    pub fn AInputEvent_release(event: *const AInputEvent);

    // AKeyEvent_*（按键事件只读访问）。
    pub fn AKeyEvent_getAction(key_event: *const AInputEvent) -> i32;
    pub fn AKeyEvent_getFlags(key_event: *const AInputEvent) -> i32;
    pub fn AKeyEvent_getKeyCode(key_event: *const AInputEvent) -> i32;
    pub fn AKeyEvent_getScanCode(key_event: *const AInputEvent) -> i32;
    pub fn AKeyEvent_getMetaState(key_event: *const AInputEvent) -> i32;
    pub fn AKeyEvent_getRepeatCount(key_event: *const AInputEvent) -> i32;
    pub fn AKeyEvent_getDownTime(key_event: *const AInputEvent) -> i64;
    pub fn AKeyEvent_getEventTime(key_event: *const AInputEvent) -> i64;

    // AMotionEvent_*（触摸事件只读访问，PRD §5.2 全表）。
    pub fn AMotionEvent_getAction(motion_event: *const AInputEvent) -> i32;
    pub fn AMotionEvent_getFlags(motion_event: *const AInputEvent) -> i32;
    pub fn AMotionEvent_getMetaState(motion_event: *const AInputEvent) -> i32;
    pub fn AMotionEvent_getDownTime(motion_event: *const AInputEvent) -> i64;
    pub fn AMotionEvent_getEventTime(motion_event: *const AInputEvent) -> i64;
    pub fn AMotionEvent_getPointerCount(motion_event: *const AInputEvent) -> usize;
    pub fn AMotionEvent_getPointerId(motion_event: *const AInputEvent, pointer_index: usize)
    -> i32;
    pub fn AMotionEvent_getToolType(motion_event: *const AInputEvent, pointer_index: usize) -> i32;
    pub fn AMotionEvent_getRawX(motion_event: *const AInputEvent, pointer_index: usize) -> f32;
    pub fn AMotionEvent_getRawY(motion_event: *const AInputEvent, pointer_index: usize) -> f32;
    pub fn AMotionEvent_getX(motion_event: *const AInputEvent, pointer_index: usize) -> f32;
    pub fn AMotionEvent_getY(motion_event: *const AInputEvent, pointer_index: usize) -> f32;
    pub fn AMotionEvent_getPressure(motion_event: *const AInputEvent, pointer_index: usize) -> f32;
    pub fn AMotionEvent_getAxisValue(
        motion_event: *const AInputEvent,
        axis: i32,
        pointer_index: usize,
    ) -> f32;
    pub fn AMotionEvent_getHistorySize(motion_event: *const AInputEvent) -> usize;
    pub fn AMotionEvent_getHistoricalEventTime(
        motion_event: *const AInputEvent,
        history_index: usize,
    ) -> i64;
    pub fn AMotionEvent_getHistoricalX(
        motion_event: *const AInputEvent,
        pointer_index: usize,
        history_index: usize,
    ) -> f32;
    pub fn AMotionEvent_getHistoricalY(
        motion_event: *const AInputEvent,
        pointer_index: usize,
        history_index: usize,
    ) -> f32;

    // AInputQueue_*（队列泵取，PRD §8.3 启动链路第 4 步）。
    /// `callback` 与 `data` 允许 NULL（配合 pollOnce 轮询模式）。
    pub fn AInputQueue_attachLooper(
        queue: *mut AInputQueue,
        looper: *mut ALooper,
        ident: i32,
        callback: Option<unsafe extern "C" fn(*mut core::ffi::c_void, i32) -> i32>,
        data: *mut core::ffi::c_void,
    );
    pub fn AInputQueue_detachLooper(queue: *mut AInputQueue);
    pub fn AInputQueue_hasEvents(queue: *mut AInputQueue) -> i32;
    pub fn AInputQueue_getEvent(queue: *mut AInputQueue, out_event: *mut *mut AInputEvent) -> i32;
    pub fn AInputQueue_preDispatchEvent(queue: *mut AInputQueue, event: *mut AInputEvent) -> i32;
    pub fn AInputQueue_finishEvent(queue: *mut AInputQueue, event: *mut AInputEvent, handled: i32);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 动作常量与 NDK 头文件逐一对照（PRD §5.1.8 常量对齐要求）。
    #[test]
    fn action_constants_match_ndk() {
        assert_eq!(AINPUT_EVENT_TYPE_KEY, 1);
        assert_eq!(AINPUT_EVENT_TYPE_MOTION, 2);
        assert_eq!(AKEY_EVENT_ACTION_DOWN, 0);
        assert_eq!(AKEY_EVENT_ACTION_UP, 1);
        assert_eq!(AMOTION_EVENT_ACTION_DOWN, 0);
        assert_eq!(AMOTION_EVENT_ACTION_UP, 1);
        assert_eq!(AMOTION_EVENT_ACTION_MOVE, 2);
        assert_eq!(AMOTION_EVENT_ACTION_CANCEL, 3);
        assert_eq!(AMOTION_EVENT_ACTION_POINTER_DOWN, 5);
        assert_eq!(AMOTION_EVENT_ACTION_POINTER_UP, 6);
        // ACTION_MASK 与 POINTER_INDEX 组合语义：getActionMasked/getActionIndex
        // 的位拆分必须与 Android 一致（PRD §5.2）。
        let action =
            AMOTION_EVENT_ACTION_POINTER_DOWN | (3 << AMOTION_EVENT_ACTION_POINTER_INDEX_SHIFT);
        assert_eq!(
            action & AMOTION_EVENT_ACTION_MASK,
            AMOTION_EVENT_ACTION_POINTER_DOWN
        );
        assert_eq!(
            (action & AMOTION_EVENT_ACTION_POINTER_INDEX_MASK)
                >> AMOTION_EVENT_ACTION_POINTER_INDEX_SHIFT,
            3
        );
    }
}
