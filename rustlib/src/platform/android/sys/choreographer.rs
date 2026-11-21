//! AChoreographer NDK 绑定（真 vsync 帧回调，PRD §5.1.6）。
//!
//! # 用途
//! 替代 16ms poll 超时的近似 vsync：注册 `CHOREOGRAPHER_CALLBACK_ANIMATION`
//! 回调，由系统 vsync 信号驱动帧推进（60/90/120Hz 自适应）。
//!
//! # 线程模型
//! 回调在注册线程的 looper 上执行（本框架 = 主循环线程），
//! 回调内只置位原子标志 + 写 eventfd 唤醒，实际帧处理在主循环体完成
//! （避免回调内持有 demo/backend 可变借用）。

/// 不透明句柄：AChoreographer 实例。
#[repr(C)]
pub struct AChoreographer {
    _private: [u8; 0],
}

/// 回调类型常量（与 NDK `AChoreographer_CallbackType` 一致）。
pub const CHOREOGRAPHER_CALLBACK_INPUT: i32 = 0x00;
pub const CHOREOGRAPHER_CALLBACK_ANIMATION: i32 = 0x01;
pub const CHOREOGRAPHER_CALLBACK_TRAVERSAL: i32 = 0x02;
pub const CHOREOGRAPHER_CALLBACK_COMMIT: i32 = 0x03;
pub const CHOREOGRAPHER_CALLBACK_RESIZE: i32 = 0x04;

/// 帧回调函数签名（frame_time_nanos = 本帧 vsync 时间戳，ns）。
pub type FrameCallback = extern "C" fn(frame_time_nanos: i64, user_data: *mut core::ffi::c_void);

/// 获取当前线程的 Choreographer 实例（线程局部，不转移所有权）。
///
/// # 返回
/// 非空指针（当前线程已 attach looper 时有效）；调用方不 release。
#[cfg(target_os = "android")]
#[link(name = "android")]
unsafe extern "C" {
    pub fn AChoreographer_getInstance() -> *const AChoreographer;
}

/// 注册帧回调（同类型重复注册覆盖旧回调）。
///
/// # 参数
/// - `callback_type`：回调阶段（ANIMATION = 动画帧）；
/// - `callback`：帧回调函数；
/// - `data`：用户数据指针（原样传回回调）。
#[cfg(target_os = "android")]
#[link(name = "android")]
unsafe extern "C" {
    pub fn AChoreographer_postCallback(
        choreographer: *const AChoreographer,
        callback_type: i32,
        callback: FrameCallback,
        data: *mut core::ffi::c_void,
    );
}

/// 移除指定类型的帧回调。
#[cfg(target_os = "android")]
#[link(name = "android")]
unsafe extern "C" {
    pub fn AChoreographer_removeCallback(
        choreographer: *const AChoreographer,
        callback_type: i32,
        callback: FrameCallback,
        data: *mut core::ffi::c_void,
    );
}

/// 布局断言：AChoreographer 为不透明空结构（与 NDK 头文件一致）。
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choreographer_is_opaque() {
        assert_eq!(core::mem::size_of::<AChoreographer>(), 0);
    }
}
