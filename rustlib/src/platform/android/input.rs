//! 平台安全封装层：输入队列泵取与事件解码（借用语义）。

use super::sys::input::{AInputEvent, AInputQueue};
use crate::ui::input::{KeyEvent, MotionEvent};

/// 不安全泵取原语：Android 真实现，主机空桩（集中 cfg，避免散落表达式）。
#[cfg(target_os = "android")]
mod ffi {
    use super::super::sys::input::*;
    use crate::ui::input::{HistoricalSample, KeyAction, Keycode, MotionEvent, PointerCoords};

    pub const TYPE_KEY: i32 = AINPUT_EVENT_TYPE_KEY;
    pub const TYPE_MOTION: i32 = AINPUT_EVENT_TYPE_MOTION;

    // SAFETY: raw 均由安全层校验非空后传入。
    pub unsafe fn has_events(raw: *mut AInputQueue) -> bool {
        unsafe { AInputQueue_hasEvents(raw) > 0 }
    }

    /// 借出事件；0 = 队列空，负 = 错误。
    pub unsafe fn get_event(raw: *mut AInputQueue, out: *mut *mut AInputEvent) -> i32 {
        unsafe { AInputQueue_getEvent(raw, out) }
    }

    pub unsafe fn pre_dispatch(queue: *mut AInputQueue, event: *mut AInputEvent) -> bool {
        unsafe { AInputQueue_preDispatchEvent(queue, event) != 0 }
    }

    pub unsafe fn finish(queue: *mut AInputQueue, event: *mut AInputEvent, handled: i32) {
        unsafe { AInputQueue_finishEvent(queue, event, handled) }
    }

    pub unsafe fn event_type(event: *mut AInputEvent) -> i32 {
        unsafe { AInputEvent_getType(event) }
    }

    /// 解码按键事件（`AKeyEvent_*` 全 getter 语义）。
    pub unsafe fn decode_key(event: *mut AInputEvent) -> Option<crate::ui::input::KeyEvent> {
        // SAFETY: 事件来自 getEvent，借用期间有效。
        unsafe {
            Some(crate::ui::input::KeyEvent {
                action: KeyAction::from_raw(AKeyEvent_getAction(event)),
                keycode: Keycode::from_raw(AKeyEvent_getKeyCode(event)),
                event_time: AKeyEvent_getEventTime(event),
                down_time: AKeyEvent_getDownTime(event),
                repeat_count: AKeyEvent_getRepeatCount(event),
            })
        }
    }

    /// 解码触摸事件（`AMotionEvent_*` 全 getter 语义，含历史采样）。
    ///
    /// # 失败语义
    /// 指针数为 0 或超 32（协议异常）时返回 `None`，调用方按未处理归还。
    pub unsafe fn decode_motion(event: *mut AInputEvent) -> Option<MotionEvent> {
        // SAFETY: 事件来自 getEvent，借用期间有效；pointer_count 由框架
        // 保证与本事件实际指针数一致（BAD_VALUE 已在 NDK 内部校验）。
        unsafe {
            let pointer_count = AMotionEvent_getPointerCount(event);
            if pointer_count == 0 || pointer_count > 32 {
                return None;
            }
            let mut pointers = Vec::with_capacity(pointer_count);
            for i in 0..pointer_count {
                pointers.push(PointerCoords {
                    pointer_id: AMotionEvent_getPointerId(event, i),
                    x: AMotionEvent_getX(event, i),
                    y: AMotionEvent_getY(event, i),
                    pressure: AMotionEvent_getPressure(event, i),
                });
            }
            let history_size = AMotionEvent_getHistorySize(event);
            let mut historical = Vec::with_capacity(history_size);
            for h in 0..history_size {
                let mut coords = Vec::with_capacity(pointer_count);
                for i in 0..pointer_count {
                    coords.push(PointerCoords {
                        pointer_id: AMotionEvent_getPointerId(event, i),
                        x: AMotionEvent_getHistoricalX(event, i, h),
                        y: AMotionEvent_getHistoricalY(event, i, h),
                        pressure: AMotionEvent_getPressure(event, i),
                    });
                }
                historical.push(HistoricalSample {
                    event_time: AMotionEvent_getHistoricalEventTime(event, h),
                    coords,
                });
            }
            Some(MotionEvent {
                packed_action: AMotionEvent_getAction(event),
                down_time: AMotionEvent_getDownTime(event),
                event_time: AMotionEvent_getEventTime(event),
                pointers,
                historical,
            })
        }
    }
}

#[cfg(not(target_os = "android"))]
mod ffi {
    use super::super::sys::input::{AInputEvent, AInputQueue};
    use crate::ui::input::{KeyEvent, MotionEvent};

    pub const TYPE_KEY: i32 = 1;
    pub const TYPE_MOTION: i32 = 2;

    pub unsafe fn has_events(_: *mut AInputQueue) -> bool {
        false
    }

    pub unsafe fn get_event(_: *mut AInputQueue, _: *mut *mut AInputEvent) -> i32 {
        -1
    }

    pub unsafe fn pre_dispatch(_: *mut AInputQueue, _: *mut AInputEvent) -> bool {
        false
    }

    pub unsafe fn finish(_: *mut AInputQueue, _: *mut AInputEvent, _: i32) {}

    pub unsafe fn event_type(_: *mut AInputEvent) -> i32 {
        0
    }

    pub unsafe fn decode_key(_: *mut AInputEvent) -> Option<KeyEvent> {
        None
    }

    pub unsafe fn decode_motion(_: *mut AInputEvent) -> Option<MotionEvent> {
        None
    }
}

/// 输入队列的安全借用。
///
/// # 不变量
/// - 队列由框架在 `onInputQueueCreated` 交出、`onInputQueueDestroyed` 收回，
///   本类型只借用；
/// - 事件句柄生命周期：`getEvent` 借出、`finishEvent` 归还，绝不跨帧持有。
#[derive(Debug)]
pub struct InputQueue {
    raw: *mut AInputQueue,
}

/// 从队列借出的单个事件：必须归还（drop 自动 finish）。
pub struct InputEvent<'q> {
    queue: &'q InputQueue,
    raw: *mut AInputEvent,
}

impl InputQueue {
    /// 从回调交出的原生指针构造借用；NULL 视作协议错误返回 `None`。
    pub fn from_raw(raw: *mut AInputQueue) -> Option<Self> {
        if raw.is_null() {
            None
        } else {
            Some(Self { raw })
        }
    }

    /// 原生指针（供 detachLooper；队列生命周期由框架管理）。
    pub fn as_ptr(&self) -> *mut AInputQueue {
        self.raw
    }

    /// 队列是否还有待处理事件。
    pub fn has_events(&self) -> bool {
        // SAFETY: raw 构造时已校验非空。
        unsafe { ffi::has_events(self.raw) }
    }

    /// 取出下一个事件。
    ///
    /// # 失败语义
    /// 队列空返回 `Ok(None)`；错误返回 `Err(())`，调用方应结束本轮泵取。
    pub fn get_event(&self) -> Result<Option<InputEvent<'_>>, ()> {
        let mut raw: *mut AInputEvent = core::ptr::null_mut();
        // SAFETY: raw 出参合法。
        let r = unsafe { ffi::get_event(self.raw, &mut raw) };
        if r < 0 {
            Err(())
        } else if raw.is_null() {
            Ok(None)
        } else {
            Ok(Some(InputEvent { queue: self, raw }))
        }
    }

    /// 事件预分发（交给 IME 等先行消费）。
    ///
    /// # 返回
    /// `true` 表示事件已被预分发消费，必须立即 finish（由 [`InputEvent::finish`]
    /// 统一处理）；`false` 表示可正常处理。
    pub fn pre_dispatch(&self, event: &mut InputEvent<'_>) -> bool {
        // SAFETY: 事件来自本队列的 getEvent，尚未归还。
        unsafe { ffi::pre_dispatch(self.raw, event.raw) }
    }
}

impl InputEvent<'_> {
    /// 事件类型：按键或触摸。
    pub fn kind(&self) -> EventKind {
        // SAFETY: 事件来自 getEvent，借用期间有效。
        match unsafe { ffi::event_type(self.raw) } {
            ffi::TYPE_KEY => EventKind::Key,
            ffi::TYPE_MOTION => EventKind::Motion,
            _ => EventKind::Other,
        }
    }

    /// 解码为按键事件（非 KEY 类事件调用返回 None）。
    pub fn decode_key(&self) -> Option<KeyEvent> {
        // SAFETY: 事件来自 getEvent，借用期间有效。
        unsafe { ffi::decode_key(self.raw) }
    }

    /// 解码为触摸事件（非 MOTION 类或指针数异常事件返回 None）。
    pub fn decode_motion(&self) -> Option<MotionEvent> {
        // SAFETY: 事件来自 getEvent，借用期间有效。
        unsafe { ffi::decode_motion(self.raw) }
    }

    /// 显式标记处理结果并归还事件（未调用则 drop 时按“已处理”归还）。
    pub fn finish(self, handled: bool) {
        // SAFETY: 事件尚未归还；用 ManuallyDrop 阻止 Drop 二次 finish。
        let this = core::mem::ManuallyDrop::new(self);
        unsafe { ffi::finish(this.queue.raw, this.raw, i32::from(handled)) }
    }
}

impl Drop for InputEvent<'_> {
    fn drop(&mut self) {
        // SAFETY: 未显式 finish 的事件按“已处理”归还，防止事件句柄泄漏。
        unsafe { ffi::finish(self.queue.raw, self.raw, 1) }
    }
}

/// 事件大类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    Key,
    Motion,
    Other,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// NULL 队列必须被拒绝。
    #[test]
    fn null_queue_rejected() {
        assert!(InputQueue::from_raw(core::ptr::null_mut()).is_none());
    }
}
