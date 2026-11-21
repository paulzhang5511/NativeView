//! 平台安全封装层：资产读取（借用语义，对上不暴露裸指针）。

use super::sys::asset::{AAsset, AAssetManager};

/// AssetManager 的安全借用。
#[derive(Debug)]
pub struct AssetManager {
    /// 原生 AAssetManager 指针；借用不拥有，activity 存活期间有效。
    /// （Task 12 字体加载时经此打开 assets。）
    #[allow(dead_code)]
    pub(crate) raw: *mut AAssetManager,
}

/// 已打开资产的安全持有。
///
/// # 不变量
/// - 拥有 `AAsset`：`drop` 时调用 `AAsset_close` 归还；
/// - `data` 借用仅在 `self` 存活期间有效（BUFFER 模式下内容常驻内存，
///   可重复读取）。
pub struct Asset {
    /// 原始 AAsset 指针；不暴露，drop 时 AAsset_close 归还。
    /// （运行时仅经 `data()` 间接使用，故无独立读取路径。）
    #[allow(dead_code)]
    raw: *mut AAsset,
    /// 原始字节长度（fd 交接与 mmap 共享视图用，PRD §12.R1）。
    /// Task 12 字体加载起经 `data()` 读取。
    #[allow(dead_code)]
    pub(crate) len: usize,
}

impl AssetManager {
    /// 从 activity 字段交出的原生指针构造借用。
    ///
    /// # 安全
    /// `raw` 必须来自 `ANativeActivity.asset_manager` 且在 activity 存活期间有效。
    pub fn from_raw(raw: *mut AAssetManager) -> Option<Self> {
        if raw.is_null() {
            None
        } else {
            Some(Self { raw })
        }
    }

    /// 按文件名打开资产（STREAMING 模式，fd 与整读两种消费路径都支持）。
    ///
    /// # 模式选择
    /// STREAMING（而非 BUFFER）：BUFFER 模式在较新 Android 上会整体
    /// 解压到内部缓冲，`openFileDescriptor` 恒失败；STREAMING 保持
    /// 文件底层 seek 语义，fd 路径（noCompress 资产）可用，整读路径
    /// 退化为顺序读（V1 字体为一次性消费，无重复读需求）。
    ///
    /// # 失败语义
    /// 文件不存在返回 `None`；调用方记录英文日志后降级处理。
    pub fn open(&self, filename: &str) -> Option<Asset> {
        // 文件名必须能无 NUL 地转成 C 字符串，否则无法通过 FFI（静默失败不 panic）。
        let Ok(name) = std::ffi::CString::new(filename) else {
            return None;
        };
        #[cfg(target_os = "android")]
        // SAFETY: raw 已校验非空；name 为合法 C 字符串；返回空指针表示未找到。
        let raw = unsafe {
            super::sys::asset::AAssetManager_open(
                self.raw,
                name.as_ptr(),
                super::sys::asset::AASSET_MODE_STREAMING,
            )
        };
        #[cfg(not(target_os = "android"))]
        // 主机桩：无真实资产，恒返回未找到。
        let raw: *mut AAsset = core::ptr::null_mut();
        if raw.is_null() {
            return None;
        }
        #[cfg(target_os = "android")]
        // SAFETY: raw 来自 open 成功，长度为非负资产大小。
        let len = unsafe { super::sys::asset::AAsset_getLength(raw) } as usize;
        #[cfg(not(target_os = "android"))]
        let len = 0usize;
        if len == 0 {
            // SAFETY: 打开成功但为空文件，立即归还避免句柄泄漏。
            #[cfg(not(target_os = "android"))]
            let _ = name;
            #[cfg(target_os = "android")]
            unsafe {
                super::sys::asset::AAsset_close(raw)
            };
            return None;
        }
        Some(Asset { raw, len })
    }
}

impl Asset {
    /// 资产完整内容（只读借用，仅 BUFFER 模式打开的资产可用）。
    ///
    /// # 约束
    /// STREAMING 模式打开的资产内部无整体缓冲，本方法返回空切片；
    /// 需要完整内容时用 [`read_to_vec`] 或 fd 路径。
    pub fn data(&self) -> &[u8] {
        #[cfg(target_os = "android")]
        // SAFETY: BUFFER 模式下 getBuffer 返回常驻内存指针，长度在 open 时
        // 已获取且大于 0（空资产在 open 阶段被过滤）。
        unsafe {
            let ptr = super::sys::asset::AAsset_getBuffer(self.raw) as *const u8;
            if ptr.is_null() {
                return &[];
            }
            std::slice::from_raw_parts(ptr, self.len)
        }
        #[cfg(not(target_os = "android"))]
        // 主机桩：构造不出非空 Asset，本分支不可达。
        &[]
    }

    /// 顺序读完整资产到新分配缓冲（STREAMING 模式下的整读回退路径）。
    ///
    /// # 失败语义
    /// 部分读（IO 错误/EOF 提前）返回 `None`，调用方降级处理。
    pub fn read_to_vec(&self) -> Option<Vec<u8>> {
        #[cfg(target_os = "android")]
        {
            let mut out = vec![0u8; self.len];
            let mut filled = 0usize;
            while filled < self.len {
                // SAFETY: raw 来自 open 成功；buf 指向合法未越界区间。
                let n = unsafe {
                    super::sys::asset::AAsset_read(
                        self.raw,
                        out[filled..].as_mut_ptr() as *mut core::ffi::c_void,
                        self.len - filled,
                    )
                };
                if n <= 0 {
                    return None;
                }
                filled += n as usize;
            }
            Some(out)
        }
        #[cfg(not(target_os = "android"))]
        None
    }

    /// 打开资产底层文件描述符（fd 交接语义，PRD §5.5）。
    ///
    /// # 返回
    /// `(fd, offset, len)`：fd 由调用方拥有，**必须**由调用方 `close`；
    /// 字体 mmap 定位区间为 `[offset, offset+len)`（第二出参是长度，
    /// NDK 头文件语义，非区间终点）。
    ///
    /// # 失败语义
    /// 底层非可 seek 文件（压缩资产）或系统失败时返回 `None`。
    /// assets 配置 `noCompress` 后本调用在真机上恒成功（Task 1 已配置）。
    pub fn open_fd(&self) -> Option<(i32, u64, u64)> {
        #[cfg(target_os = "android")]
        unsafe {
            // off64_t 在 Linux 用户空间 ABI 上为 i64（bionic 与 glibc 一致）。
            let mut start: i64 = 0;
            let mut end: i64 = 0;
            // SAFETY: raw 来自 open 成功；out 指针均为合法栈变量。
            let fd = super::sys::asset::AAsset_openFileDescriptor64(self.raw, &mut start, &mut end);
            if fd < 0 {
                log::debug!("AAsset_openFileDescriptor64 failed: fd={fd}");
                return None;
            }
            // 头文件语义：第二出参是 *长度*（start + length 定位区间终点）。
            let len64 = end;
            if start < 0 || len64 <= 0 {
                log::debug!("AAsset_openFileDescriptor64 bad range: start={start} len={len64}");
                return None;
            }
            Some((fd, start as u64, len64 as u64))
        }
        #[cfg(not(target_os = "android"))]
        None
    }
}

impl Drop for Asset {
    fn drop(&mut self) {
        // SAFETY: raw 来自 open 成功且未 close；drop 即归还。
        #[cfg(target_os = "android")]
        unsafe {
            super::sys::asset::AAsset_close(self.raw)
        };
        #[cfg(not(target_os = "android"))]
        let _ = self.raw;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// NULL 指针必须被拒绝。
    #[test]
    fn null_manager_rejected() {
        assert!(AssetManager::from_raw(core::ptr::null_mut()).is_none());
    }

    /// 含 NUL 的文件名必须被拒绝，不进入 FFI。
    #[test]
    fn nul_filename_rejected() {
        let mgr = AssetManager::from_raw(0x3000 as *mut AAssetManager).unwrap();
        assert!(mgr.open("fo\0o").is_none());
    }
}
