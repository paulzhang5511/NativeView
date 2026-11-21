//! 无障碍（TalkBack）影子树（SPEC-accessibility.md）。
//!
//! # 架构
//! ```text
//! Rust 视图树 ──snapshot()──→ Vec<A11yNode> ──JNI 批量──→ Kotlin
//!       ↑                                                    │
//!       └────────── ACTION_CLICK 回推 ───────────────────────┘
//! ```
//!
//! # 模块划分
//! - [`node`]：节点数据模型与角色映射（纯数据，跨 JNI 的最小单元）；
//! - [`snapshot`]：把视图树拍平为节点列表（主机可测，无 JNI）；
//! - [`tree`]：真正遍历容器树的入口（依赖 `Widget` 树结构）。
//!
//! # 铁律
//! - 快照只含几何/文本/语义，**绝不含像素**（PRD §6.1/§6.2）；
//! - 快照生成只在帧回调内发生，**禁止挂在 draw 路径**；
//! - TalkBack 关闭时 `snapshot` 不被调用，零 JNI、零分配。

pub mod node;
pub mod snapshot;
pub mod tree;

pub use node::{A11yNode, A11yRole};
