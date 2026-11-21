//! 文本模块：字体加载与文字栈（PRD §5.1.4）。
//!
//! - `font`：assets → fd → mmap/`Arc<[u8]>` 共享，全进程仅一份内存拷贝；
//! - `layout`/`render`/`metrics`（Task 13）：parley 排版 + glifo 渲染。

pub mod font;
pub mod layout;
pub mod metrics;
pub mod render;
