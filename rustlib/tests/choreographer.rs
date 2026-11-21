//! Choreographer / invalidate 合并单测（Task 15，A4）。

use velloview::ui::choreographer::Choreographer;

/// 同帧多次 invalidate 只产生一次回调（A4）。
#[test]
fn multiple_invalidates_coalesce_into_one_frame() {
    let mut ch = Choreographer::new();
    ch.post_invalidate();
    ch.post_invalidate();
    ch.post_invalidate();
    let mut callbacks = 0;
    ch.dispatch_pending(|_frame_time_nanos| callbacks += 1);
    assert_eq!(
        callbacks, 1,
        "three invalidates in same frame must yield exactly one draw callback"
    );
    // 派发后无残留：再次派发零回调。
    ch.dispatch_pending(|_| callbacks += 1);
    assert_eq!(callbacks, 1, "no pending invalidate must yield no callback");
}

/// 未 invalidate 时派发零回调。
#[test]
fn no_invalidate_no_callback() {
    let mut ch = Choreographer::new();
    let mut callbacks = 0;
    ch.dispatch_pending(|_| callbacks += 1);
    assert_eq!(callbacks, 0);
}

/// requestLayout 同帧合并一次（与 invalidate 独立计数）。
#[test]
fn request_layout_coalesces() {
    let mut ch = Choreographer::new();
    ch.post_layout_request();
    ch.post_layout_request();
    let mut layouts = 0;
    let mut draws = 0;
    ch.dispatch_pending(|_| {
        layouts += 1;
        draws += 1;
    });
    assert_eq!(layouts, 1);
    assert_eq!(draws, 1);
}

/// invalidate 与 requestLayout 同帧合并为一次帧回调。
#[test]
fn invalidate_and_layout_merge_into_one_frame() {
    let mut ch = Choreographer::new();
    ch.post_invalidate();
    ch.post_layout_request();
    let mut callbacks = 0;
    ch.dispatch_pending(|_| callbacks += 1);
    assert_eq!(callbacks, 1);
}
