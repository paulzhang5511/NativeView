//! 图形原语层（PRD §7.1 `gfx-scene` 模块）。
//!
//! # 分层铁律（PRD §5.5.4）
//! - `ui-core` 不得直接依赖 `peniko`/`vello_*`：一切绘制经本模块抽象；
//! - 后端只消费 [`scene::Scene`]，不感知 Canvas/Paint 的存在。

pub mod canvas;
pub mod color;
pub mod image;
pub mod matrix;
pub mod parallel;
pub mod paint;
pub mod path;
pub mod render;
pub mod scene;
pub mod shader;
