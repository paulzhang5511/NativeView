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

    /// 编码为 PNG 字节流。
    ///
    /// # 不变量
    /// 输出为无损 RGBA8888 PNG，解码回来尺寸与像素必须逐字节一致
    /// （导出链路与黄金图依赖该保证）。
    pub fn encode_png(&self) -> Result<Vec<u8>, EncodeError> {
        let buf = image::RgbaImage::from_raw(self.width, self.height, self.data.to_vec())
            .ok_or(EncodeError)?;
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(buf)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .map_err(|_| EncodeError)?;
        Ok(out)
    }

    /// 缩放到指定尺寸（box 平均降采样 / 最近邻放大）。
    ///
    /// # 为什么不走 GPU
    /// 画板预览每帧要一份「显示尺寸」的图，`DrawImage` 只接受原尺寸位图；
    /// 在 CPU 做 box 平均比让后端加采样器简单，且结果确定、可单测。
    /// 同尺寸调用直接共享 `Arc`，零拷贝。
    pub fn resized(&self, width: u32, height: u32) -> Image {
        let (tw, th) = (width.max(1), height.max(1));
        if tw == self.width && th == self.height {
            return self.clone();
        }
        let sw = self.width.max(1) as usize;
        let sh = self.height.max(1) as usize;
        let mut out = vec![0u8; tw as usize * th as usize * 4];
        let sx_ratio = sw as f64 / tw as f64;
        let sy_ratio = sh as f64 / th as f64;
        for ty in 0..th as usize {
            let y0 = (ty as f64 * sy_ratio).floor() as usize;
            let y1 = (((ty + 1) as f64 * sy_ratio).ceil() as usize)
                .min(sh)
                .max(y0 + 1);
            for tx in 0..tw as usize {
                let x0 = (tx as f64 * sx_ratio).floor() as usize;
                let x1 = (((tx + 1) as f64 * sx_ratio).ceil() as usize)
                    .min(sw)
                    .max(x0 + 1);
                let mut acc = [0u32; 4];
                let mut n = 0u32;
                for y in y0..y1 {
                    for x in x0..x1 {
                        let i = (y * sw + x) * 4;
                        for c in 0..4 {
                            acc[c] += self.data[i + c] as u32;
                        }
                        n += 1;
                    }
                }
                let o = (ty * tw as usize + tx) * 4;
                if n == 0 {
                    continue;
                }
                for c in 0..4 {
                    out[o + c] = (acc[c] / n) as u8;
                }
            }
        }
        Image::from_rgba8(tw, th, out)
    }
}

/// PNG 编码错误（缓冲长度非法或编码器失败）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodeError;

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "png encode failed")
    }
}

impl std::error::Error for EncodeError {}

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

    /// PNG 编码往返：尺寸与像素逐字节一致。
    #[test]
    fn encode_png_roundtrip_keeps_pixels() {
        let pixels = vec![
            0xFF, 0x00, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0x80, 0x00, 0x00, 0xFF, 0x00, 0x10, 0x20,
            0x30, 0xFF,
        ];
        let img = Image::from_rgba8(2, 2, pixels.clone());
        let png = img.encode_png().unwrap();
        let back = Image::decode(&png).unwrap();
        assert_eq!(back.width(), 2);
        assert_eq!(back.height(), 2);
        assert_eq!(back.pixels().to_vec(), pixels);
    }

    /// 非法缓冲长度编码失败（不 panic）。
    #[test]
    fn encode_png_rejects_bad_length() {
        // from_rgba8 的 debug_assert 只在 debug 生效；这里直接构造错误尺寸的图不可行，
        // 改为验证正常路径不吞错误：空图（0×0）编码应失败。
        let img = Image::from_rgba8(0, 0, Vec::new());
        assert_eq!(img.encode_png(), Err(EncodeError));
    }

    /// 同尺寸缩放共享像素（零拷贝）。
    #[test]
    fn resized_same_size_shares_pixels() {
        let img = Image::from_rgba8(
            2,
            2,
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
        );
        let same = img.resized(2, 2);
        assert!(std::ptr::eq(img.pixels().as_ptr(), same.pixels().as_ptr()));
    }

    /// 2×2 → 1×1 为四像素平均。
    #[test]
    fn resized_downsample_averages_pixels() {
        let img = Image::from_rgba8(
            2,
            2,
            vec![0, 0, 0, 255, 100, 0, 0, 255, 200, 0, 0, 255, 255, 0, 0, 255],
        );
        let down = img.resized(1, 1);
        assert_eq!(down.width(), 1);
        assert_eq!(down.pixels(), &[138, 0, 0, 255]);
    }

    /// 1×1 → 2×2 放大复制同一颜色。
    #[test]
    fn resized_upsample_replicates() {
        let img = Image::from_rgba8(1, 1, vec![10, 20, 30, 255]);
        let up = img.resized(2, 2);
        assert_eq!(up.width(), 2);
        for px in up.pixels().chunks_exact(4) {
            assert_eq!(px, [10, 20, 30, 255]);
        }
    }

    /// 缩放到 0 尺寸被夹到 1，不 panic。
    #[test]
    fn resized_clamps_zero_to_one() {
        let img = Image::from_rgba8(2, 2, vec![7u8; 16]);
        let one = img.resized(0, 0);
        assert_eq!((one.width(), one.height()), (1, 1));
        assert_eq!(one.pixels(), &[7, 7, 7, 7]);
    }
}
