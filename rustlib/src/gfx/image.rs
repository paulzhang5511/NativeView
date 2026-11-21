//! 图片：解码后的 RGBA 位图（PRD §5.1.3 drawImage）。
//!
//! # 设计
//! - 解码由 `image` crate 承担（PNG/JPEG/WebP），像素全程留在 Rust 侧
//!   （约束 2.5-8：不跨 JNI）；
//! - `Image` 持有 `Arc<[u8]>` 共享像素：场景命令按值 Clone `Image`
//!   不复制像素缓冲，多帧重绘零拷贝；
//! - 行stride 等于 width*4（解码即紧凑），行对齐问题仅在窗口缓冲搬运
//!   （cpu 后端）存在。

use std::sync::Arc;

/// 解码后的 RGBA8888 图像。
///
/// # 不变量
/// `data.len() == width * height * 4`，行间无 padding；
/// 像素由 `Arc` 共享，`clone()` 仅增引用计数。
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    width: u32,
    height: u32,
    /// 紧凑 RGBA8888 像素（R 在前）。
    data: Arc<[u8]>,
}

/// 图片解码错误（格式不支持/数据损坏/解码失败）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeError;

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "image decode failed")
    }
}

impl std::error::Error for DecodeError {}

impl Image {
    /// 从内存字节解码（PNG/JPEG/WebP 自动探测格式）。
    ///
    /// # 失败语义
    /// 解码失败/非图片/维度为 0 均返回 `Err`，不 panic。
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let img = image::load_from_memory(bytes).map_err(|_| DecodeError)?;
        let rgba = img.to_rgba8();
        let (w, h) = (rgba.width(), rgba.height());
        if w == 0 || h == 0 {
            return Err(DecodeError);
        }
        Ok(Self {
            width: w,
            height: h,
            data: Arc::from(rgba.into_raw().into_boxed_slice()),
        })
    }

    /// 直接构造 RGBA8888（数据已被信任为紧凑布局）。
    ///
    /// # 不变量
    /// `data.len() == width * height * 4` 由调用方保证（渲染层零校验）。
    pub fn from_rgba8(width: u32, height: u32, data: Vec<u8>) -> Self {
        debug_assert_eq!(data.len(), width as usize * height as usize * 4);
        Self {
            width,
            height,
            data: Arc::from(data.into_boxed_slice()),
        }
    }

    /// 宽度（像素）。
    pub fn width(&self) -> u32 {
        self.width
    }

    /// 高度（像素）。
    pub fn height(&self) -> u32 {
        self.height
    }

    /// 紧凑像素切片（RGBA8888，行长 width*4）。
    pub fn pixels(&self) -> &[u8] {
        &self.data
    }

    /// 共享句柄（`Arc<[u8]>`）：后端持有的像素零拷贝视图。
    pub fn shared_pixels(&self) -> Arc<[u8]> {
        self.data.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1×1 红色 PNG 编解码往返 + 维度/像素断言。
    #[test]
    fn decode_png_roundtrip() {
        // 手工构造 1x1 RGBA PNG（image crate 编码）。
        let img = image::RgbaImage::from_raw(1, 1, vec![0xFF, 0x00, 0x00, 0xFF]).unwrap();
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let decoded = Image::decode(png.get_ref()).unwrap();
        assert_eq!(decoded.width(), 1);
        assert_eq!(decoded.height(), 1);
        assert_eq!(decoded.pixels(), &[0xFF, 0x00, 0x00, 0xFF]);
    }

    /// 非图片数据解码失败（不 panic）。
    #[test]
    fn decode_garbage_fails() {
        assert_eq!(Image::decode(b"not an image"), Err(DecodeError));
    }

    /// 所有权：clone 共享像素缓冲（Arc 引用计数，零拷贝）。
    #[test]
    fn clone_shares_pixels() {
        let img = image::RgbaImage::from_raw(2, 2, vec![0xAB; 2 * 2 * 4]).unwrap();
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let a = Image::decode(png.get_ref()).unwrap();
        let b = a.clone();
        // Arc 指针相等：同一份像素缓冲。
        assert!(std::ptr::eq(a.pixels().as_ptr(), b.pixels().as_ptr()));
        // drop 一个克隆不影响另一个的像素可读性。
        drop(b);
        assert_eq!(a.pixels().len(), 2 * 2 * 4);
    }

    /// 空数据/空图解码失败。
    #[test]
    fn decode_empty_fails() {
        assert_eq!(Image::decode(&[]), Err(DecodeError));
    }
}
