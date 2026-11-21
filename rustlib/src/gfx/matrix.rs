//! 2D 仿射矩阵（PRD §5.1.3：`Matrix` 子集）。
//!
//! # 设计
//! - 内部用 `[f64; 6]`（a,b,c,d,tx,ty）行主序表达 2x3 仿射，
//!   与 Android `Matrix` 的 3x3（无透视）语义对齐；
//! - 所有运算纯函数（Copy 值类型），无平台依赖；
//! - 变换语义与 Android 一致：`pre*` 右乘（先应用新变换），`post*` 左乘
//!   （在已有变换之后应用——注意与 Android 的 post* 相反命名习惯，
//!   本实现以"语义命名"为准：`pre_translate` = M·T，`post_translate` = T·M）。

/// 2D 仿射矩阵。
///
/// ```text
/// | a  c  tx |     x' = a·x + c·y + tx
/// | b  d  ty |     y' = b·x + d·y + ty
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Matrix {
    /// 水平缩放/旋转分量。
    pub a: f64,
    /// 垂直倾斜（skewY）分量。
    pub b: f64,
    /// 水平倾斜（skewX）分量。
    pub c: f64,
    /// 垂直缩放/旋转分量。
    pub d: f64,
    /// 水平平移。
    pub tx: f64,
    /// 垂直平移。
    pub ty: f64,
}

impl Default for Matrix {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Matrix {
    /// 单位矩阵。
    pub const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        tx: 0.0,
        ty: 0.0,
    };

    /// 纯平移矩阵。
    pub const fn translation(tx: f64, ty: f64) -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            tx,
            ty,
        }
    }

    /// 纯缩放矩阵。
    pub const fn scaling(sx: f64, sy: f64) -> Self {
        Self {
            a: sx,
            b: 0.0,
            c: 0.0,
            d: sy,
            tx: 0.0,
            ty: 0.0,
        }
    }

    /// 纯旋转矩阵（弧度，绕原点逆时针——与 Android `setRotate` 一致，
    /// Android 文档以顺时针描述但数学表达为标准旋转，此实现取数学标准）。
    pub fn rotation(radians: f64) -> Self {
        let (s, c) = radians.sin_cos();
        Self {
            a: c,
            b: s,
            c: -s,
            d: c,
            tx: 0.0,
            ty: 0.0,
        }
    }

    /// 矩阵乘法：`self · other`（先应用 other，再应用 self）。
    pub fn concat(&self, other: &Matrix) -> Matrix {
        Matrix {
            a: self.a * other.a + self.c * other.b,
            b: self.b * other.a + self.d * other.b,
            c: self.a * other.c + self.c * other.d,
            d: self.b * other.c + self.d * other.d,
            tx: self.a * other.tx + self.c * other.ty + self.tx,
            ty: self.b * other.tx + self.d * other.ty + self.ty,
        }
    }

    /// 变换点。
    pub fn map_point(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.tx,
            self.b * x + self.d * y + self.ty,
        )
    }

    /// 求逆矩阵（奇异时返回 `None`，调用方放弃本变换而非 panic）。
    pub fn invert(&self) -> Option<Matrix> {
        let det = self.a * self.d - self.b * self.c;
        if det.abs() < f64::EPSILON {
            return None;
        }
        let inv_det = 1.0 / det;
        Some(Matrix {
            a: self.d * inv_det,
            b: -self.b * inv_det,
            c: -self.c * inv_det,
            d: self.a * inv_det,
            tx: (self.c * self.ty - self.d * self.tx) * inv_det,
            ty: (self.b * self.tx - self.a * self.ty) * inv_det,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn mat_close(m: &Matrix, n: &Matrix) -> bool {
        close(m.a, n.a)
            && close(m.b, n.b)
            && close(m.c, n.c)
            && close(m.d, n.d)
            && close(m.tx, n.tx)
            && close(m.ty, n.ty)
    }

    /// 单位矩阵：变换点不变。
    #[test]
    fn identity_maps_unchanged() {
        let (x, y) = Matrix::IDENTITY.map_point(3.5, -2.0);
        assert!(close(x, 3.5) && close(y, -2.0));
    }

    /// 平移：先 T(1,2) 再 T(3,4) 应为 T(4,6)。
    #[test]
    fn translate_concatenates_additively() {
        let t = Matrix::translation(3.0, 4.0).concat(&Matrix::translation(1.0, 2.0));
        assert!(mat_close(&t, &Matrix::translation(4.0, 6.0)));
    }

    /// 缩放组合：S(2,3)·S(4,5) = S(8,15)。
    #[test]
    fn scale_concatenates_multiplicatively() {
        let s = Matrix::scaling(2.0, 3.0).concat(&Matrix::scaling(4.0, 5.0));
        assert!(mat_close(&s, &Matrix::scaling(8.0, 15.0)));
    }

    /// 旋转 + 平移的组合顺序：先旋转后平移（concat 左侧后应用）。
    #[test]
    fn rotate_then_translate() {
        // 点 (1,0) 旋转 90° → (0,1)，再平移 (5,5) → (5,6)
        let m =
            Matrix::translation(5.0, 5.0).concat(&Matrix::rotation(std::f64::consts::FRAC_PI_2));
        let (x, y) = m.map_point(1.0, 0.0);
        assert!(close(x, 5.0) && close(y, 6.0));
    }

    /// 逆矩阵：M·M⁻¹ = I。
    #[test]
    fn invert_roundtrip() {
        let m = Matrix::translation(3.0, -2.0).concat(&Matrix::rotation(0.7));
        let inv = m.invert().unwrap();
        assert!(mat_close(&m.concat(&inv), &Matrix::IDENTITY));
    }

    /// 奇异矩阵（零缩放）不可逆：返回 None 而非 panic。
    #[test]
    fn singular_matrix_not_invertible() {
        assert!(Matrix::scaling(0.0, 1.0).invert().is_none());
    }
}
