//! 行带并行工具：把逐像素的纯 CPU 循环切到多核。
//!
//! # 为什么手写而不引 rayon
//! 需要并行的只有「整块像素缓冲按行切分」这一种模式，`std::thread::scope`
//! 就够用；引入 rayon 只是为了一处调用，却要额外背一个依赖与线程池。
//!
//! # 使用前提
//! - 回调必须**只写自己那一段行**，段与段之间无交叠、无先后依赖；
//! - 小任务不并行：线程创建约 30–60µs，行数不足 `min_rows` 时退化为单线程，
//!   避免小图反而变慢（缩略图重建常见 64×64 这类尺寸）。

/// 把 `buf` 按行切成若干带，多线程执行 `f(band, 起始行号)`。
///
/// # 参数
/// - `rows`：总行数（`buf` 必须能被行数整除）；
/// - `min_rows`：每带最少行数，小于它就直接单线程跑（省线程开销）；
/// - `f`：`(本带数据, 本带起始行)`，只会写本带。
pub fn par_rows<T, F>(buf: &mut [T], rows: usize, min_rows: usize, f: F)
where
    T: Send,
    F: Fn(&mut [T], usize) + Sync,
{
    if rows == 0 || buf.is_empty() {
        return;
    }
    debug_assert_eq!(buf.len() % rows, 0, "buffer must be row-exact");
    let per_row = buf.len() / rows;
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(rows / min_rows.max(1))
        .max(1);
    if threads <= 1 {
        f(buf, 0);
        return;
    }
    crate::platform::perf::add_par_threads(threads as u64);
    let band_rows = rows.div_ceil(threads);
    std::thread::scope(|scope| {
        for (i, band) in buf.chunks_mut(band_rows * per_row).enumerate() {
            let f = &f;
            scope.spawn(move || f(band, i * band_rows));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 多带写入顺序无关且覆盖完整：每个元素按行号 + 列号编码，写后逐元素校验。
    #[test]
    fn par_rows_writes_every_element() {
        let rows = 600;
        let cols = 8;
        let mut buf = vec![0u32; rows * cols];
        par_rows(&mut buf, rows, 16, |band, first_row| {
            for (i, v) in band.iter_mut().enumerate() {
                let row = first_row + i / cols;
                let col = i % cols;
                *v = (row as u32) * 1000 + col as u32;
            }
        });
        for r in 0..rows {
            for c in 0..cols {
                assert_eq!(buf[r * cols + c], (r as u32) * 1000 + c as u32);
            }
        }
    }

    /// 行数低于阈值时也必须是完整覆盖（单线程分支）。
    #[test]
    fn par_rows_small_input_single_thread() {
        let mut buf = vec![7u8; 8];
        par_rows(&mut buf, 2, 16, |band, _| {
            for v in band.iter_mut() {
                *v = 1;
            }
        });
        assert!(buf.iter().all(|v| *v == 1));
    }
}
