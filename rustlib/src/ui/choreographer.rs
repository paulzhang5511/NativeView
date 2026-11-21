//! Choreographer：帧回调调度与 invalidate 合并（PRD §5.1.6，Task 15）。
//!
//! # 设计
//! V1 的帧源为外部驱动（Android Looper/近似 vsync），本类型只承担
//! 「同一帧内多次请求只派发一次」的合并语义；真实帧到达时调用方调
//! `dispatch_pending`。不内建线程——事件泵在 platform 层。

/// 帧调度器：invalidate / requestLayout 合并。
pub struct Choreographer {
    /// 本帧待绘制（true 时下帧派发一次 draw）。
    invalidate_pending: bool,
    /// 本帧待布局。
    layout_pending: bool,
}

impl Choreographer {
    /// 创建。
    pub fn new() -> Self {
        Self {
            invalidate_pending: false,
            layout_pending: false,
        }
    }

    /// 请求重绘（同帧多次合并）。
    pub fn post_invalidate(&mut self) {
        self.invalidate_pending = true;
    }

    /// 请求布局（同帧多次合并；布局隐含重绘）。
    pub fn post_layout_request(&mut self) {
        self.layout_pending = true;
        self.invalidate_pending = true;
    }

    /// 是否有待处理的绘制。
    pub fn has_pending(&self) -> bool {
        self.invalidate_pending || self.layout_pending
    }

    /// 帧到达：派发回调（无待处理时不调用）。
    ///
    /// # 参数
    /// `f`：`(frame_time_nanos)`；布局/绘制标志由调用方经 [`Self::has_pending`]
    /// 及派发后状态判断（V1 合并语义：单回调即可，帧内无先后分离）。
    pub fn dispatch_pending(&mut self, mut f: impl FnMut(u64)) {
        if !self.has_pending() {
            return;
        }
        // 派发后立即清位：同帧后续 post 重新置位下一帧。
        self.layout_pending = false;
        self.invalidate_pending = false;
        f(0);
    }
}

impl Default for Choreographer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 派发后清位。
    #[test]
    fn dispatch_clears_flags() {
        let mut ch = Choreographer::new();
        ch.post_invalidate();
        let mut calls = 0;
        ch.dispatch_pending(|_| calls += 1);
        assert_eq!(calls, 1);
        ch.dispatch_pending(|_| calls += 1);
        assert_eq!(calls, 1, "flags cleared after dispatch");
    }
}
