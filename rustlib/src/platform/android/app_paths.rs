//! 应用私有目录：由平台入口注入，供导出等落盘功能使用。
//!
//! # 为什么需要
//! Rust 侧拿不到 Android `Context`，而 `ANativeActivity.internal_data_path`
//! 在 onCreate 时可用；入口把它记进进程级单元，业务层只读路径。
//! 桌面单测没有注入时回退到系统临时目录，保证 `cargo test` 可跑。

use std::path::PathBuf;
use std::sync::Mutex;

/// 注入的私有根目录（Android 上即 `/data/data/<pkg>/files`）。
static FILES_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// 注入私有根目录（重复调用以最后一次为准）。
pub fn set_files_dir(dir: PathBuf) {
    if let Ok(mut guard) = FILES_DIR.lock() {
        *guard = Some(dir);
    }
}

/// 当前私有根目录；未注入返回 None。
pub fn files_dir() -> Option<PathBuf> {
    FILES_DIR.lock().ok().and_then(|g| g.clone())
}

/// 导出目录：私有根目录下的 `exports`，未注入时回退到临时目录。
pub fn exports_dir() -> PathBuf {
    match files_dir() {
        Some(dir) => dir.join("exports"),
        None => std::env::temp_dir().join("inkflow-exports"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 单测共享进程级静态，两个用例并行会互相污染，合并成一个顺序断言。
    #[test]
    fn set_and_read_files_dir() {
        // 未注入时回退临时目录，但仍以 exports 结尾。
        assert!(exports_dir().to_string_lossy().contains("export"));
        let dir = std::env::temp_dir().join("app-paths-test");
        set_files_dir(dir.clone());
        assert_eq!(files_dir(), Some(dir.clone()));
        assert_eq!(exports_dir(), dir.join("exports"));
        // 复原为未注入状态，避免影响其它用例（进程内共享）。
        *FILES_DIR.lock().unwrap() = None;
    }
}
