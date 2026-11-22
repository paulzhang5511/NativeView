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
    /// 自上次整体预乘以来被就地改写的区域（图片像素坐标，x0,y0,x1,y1）。
    ///
    /// `None` 表示「区域未知或整幅都变了」，下游缓存只能整体重算；
    /// `Some` 时下游可以只重算该窗口，省掉每帧几毫秒的整幅转换。
    patch: Option<(u32, u32, u32, u32)>,
}

thread_local! {
    /// 就地改写代际表：键 = 像素缓冲首地址，值 = 已改写次数。
    ///
    /// 只增不减、随进程存活；图片数量级很小（几十张），不做淘汰。
    static MUT_REV: std::cell::RefCell<std::collections::HashMap<usize, u32>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// 某像素缓冲的当前改写代际（从未就地改写则为 0）。
///
/// 供下游缓存（`gfx::render` 的预乘位图）与像素指针一起做缓存键。
pub fn mutation_rev(ptr: *const u8) -> u32 {
    let key = ptr as usize;
    MUT_REV.with(|m| m.borrow().get(&key).copied().unwrap_or(0))
}

fn bump_mutation(ptr: *const u8) {
    let key = ptr as usize;
    MUT_REV.with(|m| {
        let mut m = m.borrow_mut();
        let e = m.entry(key).or_insert(0);
        *e = e.wrapping_add(1);
    });
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
            patch: None,
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
            patch: None,
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

    /// 独占时可变的像素切片（`Arc` 唯一持有者才返回 `Some`）。
    ///
    /// # 为什么只给 `Option`
    /// 上一帧的 `Scene` 可能仍持有同一 `Arc` 的克隆；此时无法就地改写，
    /// 调用方必须退回「整帧重建」路径（结果一致，只是多一次分配）。
    pub fn pixels_mut(&mut self) -> Option<&mut [u8]> {
        let data = Arc::get_mut(&mut self.data)?;
        // 就地改写会把「同一 Arc 指针 + 不同内容」交给下游缓存，
        // 必须推进代际，否则预乘位图缓存会一直命中旧内容（表现为画板"像没画"）。
        bump_mutation(data.as_ptr());
        // 改写区域未知 → 让下游整体重算（保守但正确）。
        self.patch = None;
        Some(data)
    }

    /// 标记「这一次就地改写只动了 `rect` 覆盖的像素」（图片像素坐标）。
    ///
    /// 调用序必须是 `pixels_mut()` → 写入 → `note_patch(rect)`：
    /// `pixels_mut` 会先把区域清成 `None`，再由本方法收窄。
    pub fn note_patch(&mut self, rect: (u32, u32, u32, u32)) {
        self.patch = Some(rect);
    }

    /// 自上次整体预乘以来的就地改写区域（`None` = 未知/整幅）。
    pub fn patch(&self) -> Option<(u32, u32, u32, u32)> {
        self.patch
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
        // 同尺寸直接共享 `Arc`，不做任何拷贝（调用方可能依赖零拷贝语义）。
        if width.max(1) == self.width && height.max(1) == self.height {
            return self.clone();
        }
        Self::resized_from(&self.data, self.width, self.height, width, height)
    }

    /// 从**任意**源像素切片缩放到目标尺寸（box 平均降采样 / 最近邻放大）。
    ///
    /// # 为什么单独暴露
    /// 画板每帧只需「图层像素 → 显示尺寸」这一件事，若先构造 `Image`
    /// （深拷贝整层 8MB）再缩放，等于每帧多一次全图 memcpy（真机实测 ~4ms）。
    /// 直接吃源切片可以把这次拷贝彻底省掉。
    pub fn resized_from(
        src: &[u8],
        src_w: u32,
        src_h: u32,
        width: u32,
        height: u32,
    ) -> Image {
        let (sw, sh) = (src_w.max(1) as usize, src_h.max(1) as usize);
        let (tw, th) = (width.max(1), height.max(1));
        if tw == src_w && th == src_h {
            return Image::from_rgba8(src_w, src_h, src.to_vec());
        }
        debug_assert_eq!(src.len(), sw * sh * 4);
        let mut out = vec![0u8; tw as usize * th as usize * 4];
        let sx_ratio = sw as f64 / tw as f64;
        let sy_ratio = sh as f64 / th as f64;
        // 输出行互不依赖：按行带并行（大图降采样是落笔/缩放期的热点，单线程 ~20ms）。
        super::parallel::par_rows(&mut out, th as usize, 64, |band, first_row| {
        for ty in first_row..first_row + band.len() / (tw as usize * 4) {
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
                            acc[c] += src[i + c] as u32;
                        }
                        n += 1;
                    }
                }
                // band 只持有本带行，索引必须相对本带首行。
                let o = ((ty - first_row) * tw as usize + tx) * 4;
                if n == 0 {
                    continue;
                }
                for c in 0..4 {
                    band[o + c] = (acc[c] / n) as u8;
                }
            }
        }
        });
        Image::from_rgba8(tw, th, out)
    }

    /// 只重采样 `region`（源像素坐标，左闭右开）覆盖到的输出像素，就地写回 `out`。
    ///
    /// # 与 [`Self::resized`] 的一致性
    /// 采用完全相同的 box 均值公式，且每个输出像素只依赖自己的源像素盒，
    /// 因此「整帧 `resized`」与「先整帧再按脏区反复 `resample_region`」结果
    /// **逐字节相同**——这是能在不对画面做任何妥协的前提下把落笔期缩略图
    /// 更新从 ~20ms 降到几十微秒的前提。
    ///
    /// # 参数
    /// - `tw`/`th`：目标尺寸（与 `out` 对应，`out.len() == tw*th*4`）；
    /// - `out`：已有的目标像素（只覆盖脏区，其余保持不变）。
    pub fn resample_region(
        &self,
        tw: u32,
        th: u32,
        out: &mut [u8],
        region: (u32, u32, u32, u32),
    ) -> Option<(u32, u32, u32, u32)> {
        Self::resample_region_from(&self.data, self.width, self.height, tw, th, out, region)
    }

    /// 从任意源切片只重采样 `region` 覆盖的输出像素（就地写回 `out`）。
    ///
    /// # 语义
    /// 与 [`Self::resized_from`] 逐字节一致；供「持有源图层像素但不想
    /// 构造临时 `Image`」的调用方（画板落笔期的增量预览）直接使用。
    pub fn resample_region_from(
        src: &[u8],
        src_w: u32,
        src_h: u32,
        tw: u32,
        th: u32,
        out: &mut [u8],
        region: (u32, u32, u32, u32),
    ) -> Option<(u32, u32, u32, u32)> {
        let (tw, th) = (tw.max(1) as usize, th.max(1) as usize);
        let sw = src_w.max(1) as usize;
        let sh = src_h.max(1) as usize;
        debug_assert_eq!(out.len(), tw * th * 4);
        let sx_ratio = sw as f64 / tw as f64;
        let sy_ratio = sh as f64 / th as f64;
        let rx0 = region.0 as usize;
        let ry0 = region.1 as usize;
        let rx1 = (region.2 as usize).min(sw);
        let ry1 = (region.3 as usize).min(sh);
        if rx1 <= rx0 || ry1 <= ry0 {
            return None;
        }
        // 输出窗口：与脏矩形源行/列相交的输出像素必落在该窗口内（含 1 像素余量）。
        let ty_lo = ((ry0 as f64 / sy_ratio).floor() as usize).saturating_sub(1);
        let ty_hi = ((ry1 as f64 / sy_ratio).ceil() as usize + 1).min(th);
        let tx_lo = ((rx0 as f64 / sx_ratio).floor() as usize).saturating_sub(1);
        let tx_hi = ((rx1 as f64 / sx_ratio).ceil() as usize + 1).min(tw);
        for ty in ty_lo..ty_hi {
            let y0 = (ty as f64 * sy_ratio).floor() as usize;
            let y1 = (((ty + 1) as f64 * sy_ratio).ceil() as usize)
                .min(sh)
                .max(y0 + 1);
            if y1 <= ry0 || y0 >= ry1 {
                continue;
            }
            for tx in tx_lo..tx_hi {
                let x0 = (tx as f64 * sx_ratio).floor() as usize;
                let x1 = (((tx + 1) as f64 * sx_ratio).ceil() as usize)
                    .min(sw)
                    .max(x0 + 1);
                if x1 <= rx0 || x0 >= rx1 {
                    continue;
                }
                let px = box_average_at(src, sw, x0..x1, y0..y1);
                let o = (ty * tw + tx) * 4;
                out[o..o + 4].copy_from_slice(&px);
            }
        }
        // 返回真正被写入的目标窗口（含 1 像素余量），供下游缓存做局部重算。
        Some((
            tx_lo as u32,
            ty_lo as u32,
            tx_hi as u32,
            ty_hi as u32,
        ))
    }

}

/// 单个输出像素的 box 均值（`resized_from` 与 `resample_region_from` 共用同一实现）。
///
/// # 不变量
/// `src` 为紧凑 RGBA8888，行宽 `sw * 4`；调用方保证 `xs`/`ys` 落在源尺寸内。
fn box_average_at(
    src: &[u8],
    sw: usize,
    xs: std::ops::Range<usize>,
    ys: std::ops::Range<usize>,
) -> [u8; 4] {
    let mut acc = [0u32; 4];
    let mut n = 0u32;
    for y in ys {
        for x in xs.clone() {
            let i = (y * sw + x) * 4;
            for c in 0..4 {
                acc[c] += src[i + c] as u32;
            }
            n += 1;
        }
    }
    if n == 0 {
        return [0; 4];
    }
    [
        (acc[0] / n) as u8,
        (acc[1] / n) as u8,
        (acc[2] / n) as u8,
        (acc[3] / n) as u8,
    ]
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

    /// 增量重采样与整帧重采样必须逐字节一致（60fps 增量预览的正确性前提）。
    /// 直接吃源切片的缩放/补丁必须与 `Image` 版本逐字节一致。
    ///
    /// # 为什么
    /// 画板为省掉 8MB 深拷贝改走源切片 API；一旦两条路径公式漂移，
    /// 会出现「滚动条式的预览条纹」这类只在真机上可见的画质缺陷。
    #[test]
    fn resized_from_matches_image_api() {
        let (w, h) = (37u32, 23u32);
        let data: Vec<u8> = (0..(w * h * 4)).map(|i| (i % 251) as u8).collect();
        let img = Image::from_rgba8(w, h, data.clone());
        let a = img.resized(19, 11);
        let b = Image::resized_from(&data, w, h, 19, 11);
        assert_eq!(a.pixels(), b.pixels());

        let mut out_a = a.pixels().to_vec();
        let mut out_b = b.pixels().to_vec();
        img.resample_region(19, 11, &mut out_a, (5, 3, 12, 9));
        Image::resample_region_from(&data, w, h, 19, 11, &mut out_b, (5, 3, 12, 9));
        assert_eq!(out_a, out_b);
    }

    #[test]
    fn resample_region_matches_full_resize() {
        // 200×150 源图：每像素一个确定性的伪随机色（含不同 alpha，覆盖预乘分支）。
        let (sw, sh) = (200u32, 150u32);
        let mut data = vec![0u8; (sw * sh * 4) as usize];
        for y in 0..sh {
            for x in 0..sw {
                let i = ((y * sw + x) * 4) as usize;
                data[i] = ((x * 7 + y * 13) % 251) as u8;
                data[i + 1] = ((x * 31 + y * 5) % 241) as u8;
                data[i + 2] = ((x ^ y) % 256) as u8;
                data[i + 3] = if (x + y) % 5 == 0 { 0 } else { 200 + (x % 56) as u8 };
            }
        }
        let src = Image::from_rgba8(sw, sh, data);
        let (tw, th) = (77u32, 58u32);
        let full = src.resized(tw, th);
        // 取一块源脏区，先整帧再局部重采样：必须与整帧一次重采样一致。
        let region = (40u32, 20u32, 90u32, 70u32);
        let mut patched = full.pixels().to_vec();
        src.resample_region(tw, th, &mut patched, region);
        assert_eq!(patched, full.pixels(), "region resample must be byte-identical");
        // 换一块跨边界（贴右下角）的脏区再验一次。
        let region2 = (150u32, 100u32, 200u32, 150u32);
        let mut patched2 = full.pixels().to_vec();
        src.resample_region(tw, th, &mut patched2, region2);
        assert_eq!(patched2, full.pixels());
    }

    /// 空脏区不写任何像素（避免调用方误以为「改了」而做无谓上传）。
    #[test]
    fn resample_region_empty_is_noop() {
        let src = Image::from_rgba8(4, 4, vec![9u8; 64]);
        let full = src.resized(2, 2);
        let mut out = vec![0u8; 2 * 2 * 4];
        src.resample_region(2, 2, &mut out, (3, 3, 3, 3));
        assert!(out.iter().all(|v| *v == 0));
        assert_eq!(full.width(), 2);
    }

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
