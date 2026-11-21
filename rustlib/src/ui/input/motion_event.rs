//! MotionEvent：触摸事件语义（PRD §5.2，Task 17）。
//!
//! # 语义对齐（Android `MotionEvent`）
//! - `action` 打包位布局与 NDK 一致：低 8 bit 为动作、8~15 bit 为指针下标；
//! - 指针按 `pointer_id` 标识（跨事件稳定），按 `pointer_index` 存取
//!   （仅本事件内有效）——两者区分是多点触控正确性的前提；
//! - 历史采样（historical）为批量 MOVE 合并帧内的中间轨迹，
//!   用于高帧率手势插值；V1 保留语义不消费。

/// 触摸动作（Android `MotionEvent.ACTION_*`，仅低 8 bit 部分）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// 首指按下（手势序列起点）。
    Down,
    /// 抬起（手势序列终点）。
    Up,
    /// 移动。
    Move,
    /// 取消（父级拦截/窗口失焦；手势必须复位）。
    Cancel,
    /// 附加指针按下（`getActionIndex` 指明新指针下标）。
    PointerDown,
    /// 附加指针抬起。
    PointerUp,
    /// 其他未建模动作（V1 不细分 hover/scroll/button 系列）。
    Other,
}

impl Action {
    /// Android `ACTION_*` 常量（低 8 bit 值）。
    pub const DOWN: i32 = 0;
    pub const UP: i32 = 1;
    pub const MOVE: i32 = 2;
    pub const CANCEL: i32 = 3;
    pub const POINTER_DOWN: i32 = 5;
    pub const POINTER_UP: i32 = 6;

    /// 从打包 action 值解析动作（`getActionMasked` 语义）。
    pub fn from_masked(masked: i32) -> Action {
        match masked {
            Self::DOWN => Action::Down,
            Self::UP => Action::Up,
            Self::MOVE => Action::Move,
            Self::CANCEL => Action::Cancel,
            Self::POINTER_DOWN => Action::PointerDown,
            Self::POINTER_UP => Action::PointerUp,
            _ => Action::Other,
        }
    }
}

/// 打包 action 的位布局常量（NDK `AMOTION_EVENT_ACTION_*`）。
pub const ACTION_MASK: i32 = 0xff;
pub const POINTER_INDEX_SHIFT: i32 = 8;
pub const POINTER_INDEX_MASK: i32 = 0xff00;

/// 从打包 action 值提取指针下标（`getActionIndex` 语义）。
///
/// 仅 `POINTER_DOWN`/`POINTER_UP` 使用；其余动作返回 0。
pub fn action_index(packed: i32) -> usize {
    ((packed & POINTER_INDEX_MASK) >> POINTER_INDEX_SHIFT) as usize
}

/// 从打包 action 值提取动作（`getActionMasked` 语义）。
pub fn action_masked(packed: i32) -> i32 {
    packed & ACTION_MASK
}

/// 单个指针的坐标与状态（`PointerProperties` + `PointerCoords` 子集）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointerCoords {
    /// 指针 id（跨事件稳定；系统保证在单手势序列内不变）。
    pub pointer_id: i32,
    /// 事件坐标系下的 x（相对窗口原点）。
    pub x: f32,
    /// 事件坐标系下的 y。
    pub y: f32,
    /// 压力 [0,1]（触摸屏；鼠标恒 1.0）。
    pub pressure: f32,
}

/// 历史采样点：批量 MOVE 事件的中间轨迹。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HistoricalSample {
    /// 采样时间（与事件同基准，纳秒或毫秒由解码层统一）。
    pub event_time: i64,
    /// 各指针在该采样点的坐标（下标与主坐标表一致）。
    pub coords: Vec<PointerCoords>,
}

/// 触摸事件：完整指针快照 + 历史采样。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MotionEvent {
    /// 打包动作值（低 8 bit 动作 + 8~15 bit 指针下标，与 Android 逐位一致）。
    pub packed_action: i32,
    /// 手势起点时间（`getDownTime`：DOWN 事件时间）。
    pub down_time: i64,
    /// 本事件时间（`getEventTime`）。
    pub event_time: i64,
    /// 指针坐标表（按下标存取；顺序仅本事件内有效）。
    pub pointers: Vec<PointerCoords>,
    /// 历史采样（批量 MOVE 的中间轨迹；单事件内按时间升序）。
    pub historical: Vec<HistoricalSample>,
}

impl MotionEvent {
    /// 动作（`getActionMasked` 语义）。
    pub fn action(&self) -> Action {
        Action::from_masked(action_masked(self.packed_action))
    }

    /// 动作指针下标（`getActionIndex` 语义；仅 POINTER_DOWN/UP 有意义）。
    pub fn action_index(&self) -> usize {
        action_index(self.packed_action)
    }

    /// 指针数（`getPointerCount` 语义）。
    pub fn pointer_count(&self) -> usize {
        self.pointers.len()
    }

    /// 按下标取坐标（`getX(index)`/`getY(index)` 语义；越界返回 None）。
    pub fn pointer(&self, index: usize) -> Option<&PointerCoords> {
        self.pointers.get(index)
    }

    /// 按 id 检索下标（`findPointerIndex` 语义；未找到返回 None）。
    pub fn find_pointer_index(&self, pointer_id: i32) -> Option<usize> {
        self.pointers
            .iter()
            .position(|p| p.pointer_id == pointer_id)
    }

    /// 按 id 取坐标（`getX(pointerId)` 重载语义）。
    pub fn pointer_by_id(&self, pointer_id: i32) -> Option<&PointerCoords> {
        let idx = self.find_pointer_index(pointer_id)?;
        self.pointers.get(idx)
    }

    /// 首指针坐标（单指手势便捷入口；无指针时 None）。
    pub fn primary(&self) -> Option<&PointerCoords> {
        self.pointers.first()
    }

    /// 历史采样数（`getHistorySize` 语义）。
    pub fn history_size(&self) -> usize {
        self.historical.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 单指点击序列：动作解析与坐标存取。
    #[test]
    fn single_pointer_down_up() {
        let down = MotionEvent {
            packed_action: Action::DOWN,
            down_time: 100,
            event_time: 100,
            pointers: vec![PointerCoords {
                pointer_id: 0,
                x: 50.0,
                y: 80.0,
                pressure: 0.5,
            }],
            historical: vec![],
        };
        assert_eq!(down.action(), Action::Down);
        assert_eq!(down.pointer_count(), 1);
        assert_eq!(down.primary().map(|p| (p.x, p.y)), Some((50.0, 80.0)));
        assert_eq!(down.find_pointer_index(0), Some(0));
        assert_eq!(down.pointer_by_id(0).map(|p| p.pressure), Some(0.5));
    }

    /// 多指：POINTER_DOWN 打包值解析（下标在 8~15 bit）。
    #[test]
    fn pointer_down_packed_index() {
        // Android 实际发送形式：ACTION_POINTER_DOWN | (2 << 8)。
        let packed = Action::POINTER_DOWN | (2 << POINTER_INDEX_SHIFT);
        let ev = MotionEvent {
            packed_action: packed,
            down_time: 0,
            event_time: 10,
            pointers: vec![
                PointerCoords {
                    pointer_id: 0,
                    x: 0.0,
                    y: 0.0,
                    pressure: 0.0,
                },
                PointerCoords {
                    pointer_id: 1,
                    x: 10.0,
                    y: 10.0,
                    pressure: 0.0,
                },
                PointerCoords {
                    pointer_id: 5,
                    x: 20.0,
                    y: 20.0,
                    pressure: 0.0,
                },
            ],
            historical: vec![],
        };
        assert_eq!(ev.action(), Action::PointerDown);
        assert_eq!(ev.action_index(), 2, "pointer index from bits 8..15");
        // findPointerIndex 按 id 检索，id 与下标无关。
        assert_eq!(ev.find_pointer_index(5), Some(2));
        assert_eq!(ev.find_pointer_index(9), None);
        // 下标存取与 id 存取等价。
        assert_eq!(ev.pointer(2).map(|p| p.x), Some(20.0));
        assert_eq!(ev.pointer_by_id(5).map(|p| p.x), Some(20.0));
        assert!(ev.pointer(3).is_none(), "out of range index -> None");
    }

    /// 动作常量与 Android 逐一对照（§5.1.8 常量对齐）。
    #[test]
    fn action_constants_match_android() {
        assert_eq!(Action::DOWN, 0);
        assert_eq!(Action::UP, 1);
        assert_eq!(Action::MOVE, 2);
        assert_eq!(Action::CANCEL, 3);
        assert_eq!(Action::POINTER_DOWN, 5);
        assert_eq!(Action::POINTER_UP, 6);
        // 位布局与 NDK 常量一致（防止打包/解包漂移）。
        assert_eq!(ACTION_MASK, 0xff);
        assert_eq!(POINTER_INDEX_MASK, 0xff00);
        assert_eq!(POINTER_INDEX_SHIFT, 8);
    }

    /// 历史采样：批量 MOVE 的中间轨迹存取。
    #[test]
    fn historical_samples() {
        let ev = MotionEvent {
            packed_action: Action::MOVE,
            down_time: 0,
            event_time: 20,
            pointers: vec![PointerCoords {
                pointer_id: 0,
                x: 30.0,
                y: 40.0,
                pressure: 0.5,
            }],
            historical: vec![
                HistoricalSample {
                    event_time: 10,
                    coords: vec![PointerCoords {
                        pointer_id: 0,
                        x: 10.0,
                        y: 20.0,
                        pressure: 0.5,
                    }],
                },
                HistoricalSample {
                    event_time: 15,
                    coords: vec![PointerCoords {
                        pointer_id: 0,
                        x: 20.0,
                        y: 30.0,
                        pressure: 0.5,
                    }],
                },
            ],
        };
        assert_eq!(ev.history_size(), 2);
        assert_eq!(ev.historical[0].event_time, 10);
        assert_eq!(ev.historical[1].coords[0].x, 20.0);
        // 事件坐标为最后一个采样点之后的位置。
        assert_eq!(ev.primary().map(|p| p.x), Some(30.0));
    }

    /// CANCEL：滑出边界/拦截场景的复位信号。
    #[test]
    fn cancel_action_parses() {
        let ev = MotionEvent {
            packed_action: Action::CANCEL,
            down_time: 0,
            event_time: 1,
            pointers: vec![],
            historical: vec![],
        };
        assert_eq!(ev.action(), Action::Cancel);
        assert!(ev.primary().is_none());
    }
}
