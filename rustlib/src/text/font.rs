//! 字体加载：assets → fd → mmap → 共享视图（PRD §5.1.4）。
//!
//! # 设计
//! - Android：`AAsset_openFileDescriptor64` 取 fd，mmap 数据区间后
//!   立即关闭 fd（映射不依赖 fd 存续）。映射进程生命周期持有、不 munmap
//!   （PRD 约束"全进程仅一份内存拷贝"：字体随进程退出由内核归还）。
//! - 主机：直接文件路径读取堆缓冲，与 Android 路径共用后续共享逻辑，
//!   桌面 `cargo test` 可完整覆盖（回退链与 hash 逻辑同一实现）。
//! - 共享：`FontBytes` 统一两种形态（mmap 静态切片 / `Arc<[u8]>` 堆），
//!   skrifa 仅持有借用，不复制（PRD §6.4.6 双版本交接约束）。
//!
//! # 不变量
//! - fd 打开后必须被关闭（mmap 成功立即 close；失败路径也 close）；
//! - `FontData` 内容一经加载不可变（线程安全共享的前提）。

use std::io::Read;
use std::sync::Arc;
use std::sync::OnceLock;

#[cfg(target_os = "android")]
use log::debug;
use log::info;

/// 全进程字体缓存（Task 20：Demo/TextView 共享同一份 FontData）。
///
/// # 不变量
/// 仅注入一次（onCreate 回调侧）；此后只读。`FontData` 内容不可变，
/// `Arc` 共享保证"全进程仅一份内存拷贝"约束不被破坏。
static PROCESS_FONT: OnceLock<Arc<Option<FontData>>> = OnceLock::new();

/// 注入进程字体缓存（Android onCreate 侧调用；仅首次生效）。
pub fn set_process_font(font: FontData) {
    let _ = PROCESS_FONT.set(Arc::new(Some(font)));
}

/// 读取进程字体缓存（None = 未注入，调用方降级处理）。
pub fn cached_font() -> Option<FontData> {
    PROCESS_FONT.get().and_then(|f| f.as_ref().clone())
}

/// 字体字节载体：Android mmap 或主机堆缓冲，零拷贝统一视图。
#[derive(Debug, Clone)]
pub enum FontBytes {
    /// Android mmap 共享映射（进程生命周期，不主动 munmap）。
    Mmap(&'static [u8]),
    /// 主机/回退堆缓冲（`Arc` 共享）。
    Heap(Arc<[u8]>),
}

impl FontBytes {
    /// 原始字节视图（借用，不复制）。
    pub fn as_slice(&self) -> &[u8] {
        match self {
            FontBytes::Mmap(s) => s,
            FontBytes::Heap(a) => a,
        }
    }

    /// 字节长度。
    pub fn len(&self) -> usize {
        self.as_slice().len()
    }

    /// 是否为空（空字体应在上游被拒，恒 false 防御）。
    pub fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }
}

impl AsRef<[u8]> for FontBytes {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

/// 加载产物：字体原始字节 + 元信息。
///
/// # 所有权
/// `bytes` 持有唯一一份内存拷贝；所有消费者（skrifa 解析、glifo 渲染）
/// 经 `&FontData` 或克隆 `FontBytes` 共享，不复制。
#[derive(Debug, Clone)]
pub struct FontData {
    /// 字体原始字节（TTF/OTF 容器）。
    bytes: FontBytes,
    /// 加载来源标识（日志排查用；主机为路径，Android 为 assets 名）。
    source: String,
    /// 字节 sha256 前 8 hex 字符（黄金图可复现性锚点，Task 11 记录用）。
    hash8: String,
}

impl FontData {
    /// 字体原始字节（借用，不复制）。
    pub fn bytes(&self) -> &[u8] {
        self.bytes.as_slice()
    }

    /// 共享载体克隆（长期持有的消费者用；仍指向同一份内存）。
    pub fn shared(&self) -> FontBytes {
        self.bytes.clone()
    }

    /// 来源标识（日志/诊断）。
    pub fn source(&self) -> &str {
        &self.source
    }

    /// sha256 前 8 hex（黄金图记录与缓存键关联用）。
    pub fn hash8(&self) -> &str {
        &self.hash8
    }

    /// 主机路径加载：读文件到堆缓冲后统一组装。
    fn from_heap(bytes: FontBytes, source: String) -> Self {
        Self {
            hash8: Self::compute_hash8(bytes.as_slice()),
            bytes,
            source,
        }
    }

    /// 计算 sha256 前 8 hex 字符（紧凑 FIPS 180-4 实现，仅诊断锚点用，
    /// 不承担安全职责）。
    pub(crate) fn compute_hash8(bytes: &[u8]) -> String {
        let mut h: [u32; 8] = [
            0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
            0x5be0cd19,
        ];
        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
            0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
            0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
            0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
            0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
            0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
            0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
            0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
            0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
            0xc67178f2,
        ];
        let bit_len = (bytes.len() as u64) * 8;
        // 分块压缩 + 填充尾部（55/56 边界决定 1 或 2 个尾部块）。
        let mut chunks = bytes.chunks_exact(64);
        let mut w = [0u32; 64];
        for chunk in &mut chunks {
            fill_words(chunk, &mut w);
            compress(&mut h, &mut w, &K);
        }
        let rem = chunks.remainder();
        let mut tail = [0u8; 128];
        tail[..rem.len()].copy_from_slice(rem);
        tail[rem.len()] = 0x80;
        let off = if rem.len() < 56 { 64 } else { 128 };
        tail[off - 8..off].copy_from_slice(&bit_len.to_be_bytes());
        for chunk in tail[..off].chunks_exact(64) {
            fill_words(chunk, &mut w);
            compress(&mut h, &mut w, &K);
        }
        h.iter()
            .take(4)
            .map(|v| format!("{v:08x}"))
            .collect::<String>()
            .chars()
            .take(8)
            .collect()
    }
}

/// 大端读取 16 个字进消息调度数组。
fn fill_words(chunk: &[u8], w: &mut [u32; 64]) {
    for (i, word) in w.iter_mut().take(16).enumerate() {
        *word = u32::from_be_bytes([
            chunk[i * 4],
            chunk[i * 4 + 1],
            chunk[i * 4 + 2],
            chunk[i * 4 + 3],
        ]);
    }
}

/// SHA-256 压缩函数（一轮 64 步）。
fn compress(h: &mut [u32; 8], w: &mut [u32; 64], k: &[u32; 64]) {
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = *h;
    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ (!e & g);
        let t1 = hh
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(k[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        hh = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    h[0] = h[0].wrapping_add(a);
    h[1] = h[1].wrapping_add(b);
    h[2] = h[2].wrapping_add(c);
    h[3] = h[3].wrapping_add(d);
    h[4] = h[4].wrapping_add(e);
    h[5] = h[5].wrapping_add(f);
    h[6] = h[6].wrapping_add(g);
    h[7] = h[7].wrapping_add(hh);
}

/// Android：经 assets fd mmap 加载。
///
/// # 失败语义
/// 任一步失败（打开/fd/mmap）退回 BUFFER 整读，仍保持"进程内仅一份拷贝"
/// 不变量；两条路径都失败才返回 `None` 并打英文日志。fd 不泄漏。
#[cfg(target_os = "android")]
pub fn load_from_assets(
    mgr: &crate::platform::android::asset::AssetManager,
    name: &str,
) -> Option<FontData> {
    let started = std::time::Instant::now();
    let asset = mgr.open(name)?;
    let (fd, offset, len) = match asset.open_fd() {
        Some(v) => v,
        None => {
            debug!("asset fd unavailable, falling back to full read: {name}");
            let bytes: Arc<[u8]> = asset.read_to_vec()?.into();
            let data = FontData::from_heap(FontBytes::Heap(bytes), format!("assets:{name}"));
            info!(
                "font loaded from assets (heap fallback): {} bytes in {:?}, hash {}",
                data.bytes().len(),
                started.elapsed(),
                data.hash8()
            );
            return Some(data);
        }
    };
    // fd 交接完成：mmap 之后（无论成败）立即关闭 fd，映射存活不依赖 fd。
    let mapped = map_fd(fd, offset, len);
    // SAFETY: fd 由本函数拥有且无其他使用者。
    unsafe { libc::close(fd) };
    let bytes = match mapped {
        Some(b) => b,
        None => {
            debug!("mmap failed, falling back to full read: {name}");
            let bytes: Arc<[u8]> = asset.read_to_vec()?.into();
            let data = FontData::from_heap(FontBytes::Heap(bytes), format!("assets:{name}"));
            info!(
                "font loaded from assets (heap fallback): {} bytes in {:?}, hash {}",
                data.bytes().len(),
                started.elapsed(),
                data.hash8()
            );
            return Some(data);
        }
    };
    info!(
        "font loaded from assets: {} bytes in {:?} (mmap, fd closed), hash {}",
        bytes.len(),
        started.elapsed(),
        FontData::compute_hash8(bytes)
    );
    Some(FontData {
        hash8: FontData::compute_hash8(bytes),
        source: format!("assets:{name}"),
        bytes: FontBytes::Mmap(bytes),
    })
}

/// Android：mmap 指定 fd 的 `[offset, offset+len)` 区间。
///
/// # 安全
/// fd 必须有效且指向可读文件；区间必须在文件内（`open_fd` 已保证）。
/// 映射按进程生命周期持有（字体只加载一次，不 munmap；PRD 内存约束）。
///
/// # 页对齐
/// mmap 的 offset 参数必须**按设备实际页大小**对齐（EINVAL 约束）。
/// 16KB 页设备（A11 目标）上写死 4096 会让 mmap 直接失败，静默退化为
/// 整读 14.8MB 到堆（破坏「进程内仅一份拷贝」并抬高 RSS）。
/// 因此页大小经 `sysconf(_SC_PAGESIZE)` 运行时读取，向下取整到页边界、
/// 长度补齐前导差，切片起点再前移回来。
#[cfg(target_os = "android")]
fn map_fd(fd: i32, offset: u64, len: u64) -> Option<&'static [u8]> {
    let len_usize = usize::try_from(len).ok()?;
    if len_usize == 0 {
        return None;
    }
    // SAFETY: sysconf 为无副作用查询；失败返回 -1 时回退 4096（最保守对齐）。
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    let page = if page > 0 { page as u64 } else { 4096 };
    let lead = offset % page; // 前导差：页边界到区间起点的距离
    let map_off = offset - lead; // 页对齐的映射起点
    let map_len = usize::try_from(len + lead).ok()?; // 覆盖整个区间的映射长度
    // SAFETY: fd 来自 open_fd 成功（>= 0）；map_off 已页对齐；
    // map_len 覆盖 [map_off, offset+len)（上溢由 u64 加法语义排除：
    // offset+len 不会超过 APK 文件大小）。
    let ptr = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            map_len,
            libc::PROT_READ,
            libc::MAP_SHARED,
            fd,
            map_off as libc::off_t,
        )
    };
    if ptr == libc::MAP_FAILED {
        return None;
    }
    // SAFETY: mmap 成功且 PROT_READ；映射覆盖 [map_off, map_off+map_len)，
    // 请求区间 [offset, offset+len) 是其子集（lead 补偿后指针前移）。
    // 进程生命周期持有（见函数文档），声明 'static 与之匹配。
    Some(unsafe { std::slice::from_raw_parts((ptr as *const u8).add(lead as usize), len_usize) })
}

/// 主机：从本地文件路径加载（桌面 `cargo test` 覆盖同一组装逻辑）。
pub fn load_from_path(path: &std::path::Path) -> Option<FontData> {
    let started = std::time::Instant::now();
    let mut file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    if bytes.is_empty() {
        return None;
    }
    let len = bytes.len();
    let data = FontData::from_heap(
        FontBytes::Heap(bytes.into()),
        format!("path:{}", path.display()),
    );
    info!(
        "font loaded from path: {} bytes in {:?} (heap), hash {}",
        len,
        started.elapsed(),
        data.hash8()
    );
    Some(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 空输入的 sha256 前 8 hex（FIPS 180-4 官方向量）。
    #[test]
    fn hash_empty_vector() {
        assert_eq!(FontData::compute_hash8(&[]), "e3b0c442");
    }

    /// "abc" 的 sha256 前 8 hex（FIPS 180-4 官方向量）。
    #[test]
    fn hash_abc_vector() {
        assert_eq!(FontData::compute_hash8(b"abc"), "ba7816bf");
    }

    /// 448bit 消息（56 字节，单块尾部直接放长度）官方向量。
    #[test]
    fn hash_56byte_vector() {
        let data = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        assert_eq!(FontData::compute_hash8(data), "248d6a61");
    }

    /// 896bit 消息（112 字节，触发双尾部块）官方向量。
    #[test]
    fn hash_112byte_vector() {
        let data = b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu";
        assert_eq!(FontData::compute_hash8(data), "cf5b16a7");
    }

    /// 64 字节整块边界（恰好一块，尾部另起一块）。
    #[test]
    fn hash_64byte_exact_block() {
        let data = vec![0x61u8; 64];
        // sha256("a"*64) = ffe054fe...（离线独立计算校验）
        assert_eq!(FontData::compute_hash8(&data), "ffe054fe");
    }

    /// 主机路径加载：字段语义与共享零拷贝。
    #[test]
    fn load_from_path_host() {
        let tmp =
            std::env::temp_dir().join(format!("nativeview_font_test_{}.ttf", std::process::id()));
        let content: Vec<u8> = (0..7000u32).map(|i| (i * 7 + 13) as u8).collect();
        std::fs::write(&tmp, &content).unwrap();
        let fd = load_from_path(&tmp).expect("fixture font must load");
        assert_eq!(fd.bytes().len(), 7000);
        assert_eq!(fd.bytes(), content.as_slice());
        assert!(fd.source().starts_with("path:"));
        assert_eq!(fd.hash8().len(), 8);
        // 共享克隆与原视图指向同一内存（零拷贝）。
        let shared = fd.shared();
        assert_eq!(shared.as_slice().as_ptr(), fd.bytes().as_ptr());
        std::fs::remove_file(&tmp).ok();
    }

    /// 不存在的路径必须返回 None（不 panic）。
    #[test]
    fn load_from_missing_path() {
        assert!(load_from_path(std::path::Path::new("Z:/nonexistent_dir/font.ttf")).is_none());
    }

    /// 空文件必须被拒（None），不留半初始化状态。
    #[test]
    fn load_from_empty_file() {
        let tmp =
            std::env::temp_dir().join(format!("nativeview_font_empty_{}.ttf", std::process::id()));
        std::fs::write(&tmp, []).unwrap();
        assert!(load_from_path(&tmp).is_none());
        std::fs::remove_file(&tmp).ok();
    }
}
