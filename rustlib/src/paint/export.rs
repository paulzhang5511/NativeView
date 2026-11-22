//! PNG 导出：合成全图 → 编码 → 落盘。
//!
//! # 不变量
//! - 导出内容恒为「纸色底 + 可见层合成」，与画板所见一致；
//! - 文件名只保留字母/数字/中文与 `-_`，其余一律替换为 `-`，避免路径穿越；
//! - 目录不存在时自动创建，成功返回真实字节数（UI 展示的体积不是估算）。

use std::path::{Path, PathBuf};

use super::Document;

/// 导出结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportInfo {
    /// 落盘路径。
    pub path: PathBuf,
    /// 实际文件字节数。
    pub bytes: u64,
    /// 图像宽度。
    pub width: u32,
    /// 图像高度。
    pub height: u32,
}

/// 导出失败原因。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExportError {
    /// 目录创建或写文件失败（含权限不足、磁盘满）。
    Io(String),
    /// PNG 编码失败。
    Encode,
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportError::Io(e) => write!(f, "io error: {e}"),
            ExportError::Encode => write!(f, "png encode failed"),
        }
    }
}

impl std::error::Error for ExportError {}

/// 清洗文件名：保留字母数字与 `-_`，其余替换为 `-`，压缩重复分隔符并截断。
///
/// # 边界
/// 空串或全非法字符返回 `untitled`；长度上限 64 个字符，防止超出文件系统限制。
pub fn sanitize_file_name(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    let clipped: String = trimmed.chars().take(64).collect();
    if clipped.is_empty() {
        "untitled".to_string()
    } else {
        clipped
    }
}

/// 导出 PNG 到 `dir`（文件名取 `name` 清洗后加 `.png`）。
pub fn export_png(doc: &Document, dir: &Path, name: &str) -> Result<ExportInfo, ExportError> {
    let file_name = format!("{}.png", sanitize_file_name(name));
    std::fs::create_dir_all(dir).map_err(|e| ExportError::Io(e.to_string()))?;
    let image = doc.composite();
    let bytes = image.encode_png().map_err(|_| ExportError::Encode)?;
    let path = dir.join(&file_name);
    std::fs::write(&path, &bytes).map_err(|e| ExportError::Io(e.to_string()))?;
    Ok(ExportInfo {
        path,
        bytes: bytes.len() as u64,
        width: doc.width,
        height: doc.height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::color::Color;
    use crate::paint::{Brush, BrushKind, StrokePoint};

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("inkflow-{tag}-{nanos}"))
    }

    fn doc_with_ink() -> Document {
        let mut d = Document::new(
            32,
            16,
            Color {
                r: 247,
                g: 245,
                b: 239,
                a: 255,
            },
            Brush::preset(BrushKind::Pen),
        );
        d.color = Color {
            r: 20,
            g: 20,
            b: 30,
            a: 255,
        };
        d.apply_stroke(&[
            StrokePoint::new(4.0, 8.0, 1.0, 0),
            StrokePoint::new(28.0, 8.0, 1.0, 5),
        ]);
        d
    }

    #[test]
    fn sanitize_keeps_legal_and_replaces_illegal() {
        assert_eq!(sanitize_file_name("Ink flow"), "Ink-flow");
        assert_eq!(sanitize_file_name("a/b\\c:d*?\"<>|"), "a-b-c-d");
        assert_eq!(sanitize_file_name("海报 2026"), "海报-2026");
        assert_eq!(sanitize_file_name("../../etc/passwd"), "etc-passwd");
    }

    #[test]
    fn sanitize_empty_falls_back_to_untitled() {
        assert_eq!(sanitize_file_name(""), "untitled");
        assert_eq!(sanitize_file_name("///"), "untitled");
        assert_eq!(sanitize_file_name("   "), "untitled");
    }

    #[test]
    fn sanitize_truncates_long_names() {
        let name = "x".repeat(200);
        assert_eq!(sanitize_file_name(&name).chars().count(), 64);
    }

    #[test]
    fn export_writes_decodable_png_matching_composite() {
        let d = doc_with_ink();
        let dir = temp_dir("export");
        let info = export_png(&d, &dir, "Ink flow").unwrap();
        assert_eq!(info.width, 32);
        assert_eq!(info.height, 16);
        assert!(info.path.ends_with("Ink-flow.png"));
        assert_eq!(info.bytes, std::fs::metadata(&info.path).unwrap().len());
        assert!(info.bytes > 0);

        let raw = std::fs::read(&info.path).unwrap();
        let decoded = crate::gfx::image::Image::decode(&raw).unwrap();
        assert_eq!(decoded.width(), d.width);
        assert_eq!(decoded.height(), d.height);
        assert_eq!(
            decoded.pixels(),
            d.composite().pixels(),
            "导出内容必须等于合成图"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn export_creates_missing_directory() {
        let d = doc_with_ink();
        let dir = temp_dir("nested").join("a").join("b");
        let info = export_png(&d, &dir, "nested").unwrap();
        assert!(info.path.exists());
        std::fs::remove_dir_all(dir.parent().unwrap().parent().unwrap()).ok();
    }

    #[test]
    fn export_fails_when_dir_is_a_file() {
        let d = doc_with_ink();
        let dir = temp_dir("conflict");
        std::fs::write(&dir, b"x").unwrap();
        let err = export_png(&d, &dir, "x").unwrap_err();
        assert!(matches!(err, ExportError::Io(_)));
        std::fs::remove_file(&dir).ok();
    }
}
