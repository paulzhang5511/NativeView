//! 内置控件：FrameLayout / LinearLayout / ScrollView / TextView /
//! EditText / ImageView / Button（PRD §5.1.7，Task 16/23）。

pub mod button;
pub mod edit_text;
pub mod image_view;
pub mod layout;
pub mod scroll_view;
pub mod text_view;
pub mod widget;

pub use button::Button;
pub use edit_text::EditText;
pub use image_view::{ImageView, ScaleType};
pub use layout::{FrameLayout, LinearLayout, Orientation, ViewWidget};
pub use scroll_view::ScrollView;
pub use text_view::{Ellipsize, TextAlignment, TextView};
pub use widget::Widget;
