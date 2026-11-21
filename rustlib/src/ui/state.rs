//! 视图状态保存（PRD §5.1.9：自定义轻量结构，无 Parcelable 体系）。
//!
//! # 范围（V1）
//! - `TextView`/`EditText` 文本（`input_text` / 普通 `text`）；
//! - `ScrollView` 滚动位置。
//!
//! # 设计
//! 框架不定义序列化格式：`SavedState` 是**语义容器**（键值均为语义
//! 类型），由平台层决定编码（onSaveInstanceState 的 `size_t` 出参
//! 约定为 UTF-8 JSON 长度）。键由控件在保存时自报，控件树结构不
//! 参与匹配——Activity 重建后宿主按业务语义取回，不做自动回填
//! （V1 无稳定控件 id 跨重建对应，PRD §5.1.9 明示仅保存两类状态）。

use std::collections::BTreeMap;

/// 跨重建保留的视图状态（语义容器）。
///
/// # 不变量
/// - 键唯一（后写覆盖先写，与 Android onSaveInstanceState 合并语义一致）；
/// - 值只含纯数据（字符串/浮点/整数），不含任何控件引用或像素。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SavedState {
    entries: BTreeMap<String, Entry>,
}

/// 单条状态项。
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    /// 文本内容（TextView/EditText）。
    Text(String),
    /// 浮点数值（ScrollView 滚动偏移等）。
    Float(f32),
    /// 整数值（页码/下标等）。
    Int(i64),
    /// 布尔值（焦点/选中态等）。
    Bool(bool),
}

impl SavedState {
    /// 空状态（冷启动）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 写入文本项。
    pub fn put_text(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.entries.insert(key.into(), Entry::Text(value.into()));
    }

    /// 写入浮点项（滚动位置等）。
    pub fn put_float(&mut self, key: impl Into<String>, value: f32) {
        self.entries.insert(key.into(), Entry::Float(value));
    }

    /// 写入整数项。
    pub fn put_int(&mut self, key: impl Into<String>, value: i64) {
        self.entries.insert(key.into(), Entry::Int(value));
    }

    /// 写入布尔项。
    pub fn put_bool(&mut self, key: impl Into<String>, value: bool) {
        self.entries.insert(key.into(), Entry::Bool(value));
    }

    /// 读取文本项（缺失返回 None）。
    pub fn get_text(&self, key: &str) -> Option<&str> {
        match self.entries.get(key) {
            Some(Entry::Text(s)) => Some(s),
            _ => None,
        }
    }

    /// 读取浮点项。
    pub fn get_float(&self, key: &str) -> Option<f32> {
        match self.entries.get(key) {
            Some(Entry::Float(v)) => Some(*v),
            _ => None,
        }
    }

    /// 读取整数项。
    pub fn get_int(&self, key: &str) -> Option<i64> {
        match self.entries.get(key) {
            Some(Entry::Int(v)) => Some(*v),
            _ => None,
        }
    }

    /// 读取布尔项。
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        match self.entries.get(key) {
            Some(Entry::Bool(v)) => Some(*v),
            _ => None,
        }
    }

    /// 是否为空（冷启动跳过恢复）。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 条目数（日志/诊断用）。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 序列化为 UTF-8 JSON（onSaveInstanceState 出参载荷）。
    ///
    /// # 格式稳定性
    /// 键按 BTreeMap 字典序输出，同一状态序列化结果字节级一致，
    /// 便于真机日志比对与回归测试。
    pub fn to_json(&self) -> String {
        let mut out = String::with_capacity(64 + self.entries.len() * 24);
        out.push('{');
        for (i, (k, v)) in self.entries.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            Self::write_json_string(&mut out, k);
            out.push(':');
            match v {
                Entry::Text(s) => Self::write_json_string(&mut out, s),
                Entry::Float(v) => {
                    // f32 最短往返表示，避免恢复时丢精度。
                    out.push_str(&format!("{v}"));
                }
                Entry::Int(v) => out.push_str(&v.to_string()),
                Entry::Bool(v) => out.push_str(if *v { "true" } else { "false" }),
            }
        }
        out.push('}');
        out
    }

    /// 从 UTF-8 JSON 反序列化（Activity 重建恢复）。
    ///
    /// # 失败语义
    /// 任何格式错误返回 None（恢复失败按冷启动处理，绝不半恢复）。
    pub fn from_json(json: &str) -> Option<Self> {
        let mut parser = Parser {
            bytes: json.as_bytes(),
            pos: 0,
        };
        parser.skip_ws();
        if !parser.eat(b'{') {
            return None;
        }
        let mut state = SavedState::new();
        parser.skip_ws();
        if parser.eat(b'}') {
            return Some(state);
        }
        loop {
            parser.skip_ws();
            let key = parser.parse_string()?;
            parser.skip_ws();
            if !parser.eat(b':') {
                return None;
            }
            parser.skip_ws();
            let value = parser.parse_value()?;
            state.entries.insert(key, value);
            parser.skip_ws();
            if parser.eat(b',') {
                continue;
            }
            if parser.eat(b'}') {
                break;
            }
            return None;
        }
        parser.skip_ws();
        if parser.pos != parser.bytes.len() {
            return None;
        }
        Some(state)
    }

    /// JSON 字符串转义写入（双引号 + 控制字符转义）。
    fn write_json_string(out: &mut String, s: &str) {
        out.push('"');
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                c => out.push(c),
            }
        }
        out.push('"');
    }
}

/// 手写最小 JSON 解析器（无第三方依赖；只解析本类型生成的形状）。
struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn skip_ws(&mut self) {
        while let Some(b) = self.bytes.get(self.pos) {
            if matches!(b, b' ' | b'\t' | b'\n' | b'\r') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn eat(&mut self, expect: u8) -> bool {
        if self.bytes.get(self.pos) == Some(&expect) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn parse_string(&mut self) -> Option<String> {
        if !self.eat(b'"') {
            return None;
        }
        let mut out = String::new();
        loop {
            let b = self.peek()?;
            self.pos += 1;
            match b {
                b'"' => return Some(out),
                b'\\' => {
                    let esc = self.peek()?;
                    self.pos += 1;
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000C}'),
                        b'u' => {
                            // \uXXXX：十六进制 4 位。
                            if self.pos + 4 > self.bytes.len() {
                                return None;
                            }
                            let hex =
                                std::str::from_utf8(&self.bytes[self.pos..self.pos + 4]).ok()?;
                            let code = u32::from_str_radix(hex, 16).ok()?;
                            self.pos += 4;
                            // 代理对：本类型生成端不产生，遇到即拒绝。
                            let c = char::from_u32(code)?;
                            out.push(c);
                        }
                        _ => return None,
                    }
                }
                b if b < 0x20 => return None,
                _ => {
                    // 多字节 UTF-8：回退一字节后按字符消费。
                    self.pos -= 1;
                    let s = self.bytes[self.pos..].as_ref();
                    let c = char_from_utf8_first(s)?;
                    out.push(c);
                    self.pos += c.len_utf8();
                }
            }
        }
    }

    fn parse_value(&mut self) -> Option<Entry> {
        match self.peek()? {
            b'"' => self.parse_string().map(Entry::Text),
            b't' => {
                self.eat_word("true")?;
                Some(Entry::Bool(true))
            }
            b'f' => {
                self.eat_word("false")?;
                Some(Entry::Bool(false))
            }
            b'-' | b'0'..=b'9' => self.parse_number(),
            _ => None,
        }
    }

    fn eat_word(&mut self, word: &str) -> Option<()> {
        if self.bytes.len() >= self.pos + word.len()
            && &self.bytes[self.pos..self.pos + word.len()] == word.as_bytes()
        {
            self.pos += word.len();
            Some(())
        } else {
            None
        }
    }

    fn parse_number(&mut self) -> Option<Entry> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        let is_float = if self.peek() == Some(b'.') {
            self.pos += 1;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
            true
        } else {
            false
        };
        // 指数（本类型生成端不产生，但解析端容忍）。
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.pos += 1;
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
            // 有指数按浮点处理。
            let text = std::str::from_utf8(&self.bytes[start..self.pos]).ok()?;
            return text.parse::<f32>().ok().map(Entry::Float);
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos]).ok()?;
        if is_float {
            text.parse::<f32>().ok().map(Entry::Float)
        } else {
            text.parse::<i64>().ok().map(Entry::Int)
        }
    }
}

/// 从 UTF-8 切片解析首个字符（多字节安全）。
fn char_from_utf8_first(bytes: &[u8]) -> Option<char> {
    // 按最长前缀尝试（UTF-8 序列长度由首字节确定）。
    let len = match bytes.first()? {
        b if b & 0x80 == 0x00 => 1,
        b if b & 0xE0 == 0xC0 => 2,
        b if b & 0xF0 == 0xE0 => 3,
        b if b & 0xF8 == 0xF0 => 4,
        _ => return None,
    };
    if bytes.len() < len {
        return None;
    }
    std::str::from_utf8(&bytes[..len]).ok()?.chars().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 空状态序列化为 "{}"，且可往返。
    #[test]
    fn empty_roundtrip() {
        let s = SavedState::new();
        assert_eq!(s.to_json(), "{}");
        let back = SavedState::from_json("{}").unwrap();
        assert!(back.is_empty());
    }

    /// 各类型条目往返一致。
    #[test]
    fn roundtrip_all_types() {
        let mut s = SavedState::new();
        s.put_text("input_text", "你好 world\n\"quoted\" \\slash");
        s.put_float("scroll_y", 123.5);
        s.put_int("page", 2);
        s.put_bool("focused", true);
        let json = s.to_json();
        let back = SavedState::from_json(&json).unwrap();
        assert_eq!(
            back.get_text("input_text"),
            Some("你好 world\n\"quoted\" \\slash")
        );
        assert_eq!(back.get_float("scroll_y"), Some(123.5));
        assert_eq!(back.get_int("page"), Some(2));
        assert_eq!(back.get_bool("focused"), Some(true));
    }

    /// 字典序输出保证序列化稳定（同状态字节级一致）。
    #[test]
    fn deterministic_output() {
        let mut a = SavedState::new();
        a.put_text("b", "1");
        a.put_text("a", "0");
        let mut b = SavedState::new();
        b.put_text("a", "0");
        b.put_text("b", "1");
        assert_eq!(a.to_json(), b.to_json());
    }

    /// 畸形输入一律 None（绝不半恢复）。
    #[test]
    fn malformed_input_rejected() {
        assert!(SavedState::from_json("").is_none());
        assert!(SavedState::from_json("{").is_none());
        assert!(SavedState::from_json("{\"a\":}").is_none());
        assert!(SavedState::from_json("{\"a\":1}垃圾").is_none());
        assert!(SavedState::from_json("[1,2]").is_none());
        // 控制字符未转义（生成端不会产生）。
        assert!(SavedState::from_json("{\"a\":\"x\ty\"}").is_none());
    }

    /// 控制字符经转义可往返。
    #[test]
    fn control_chars_escaped() {
        let mut s = SavedState::new();
        s.put_text("t", "a\u{1}b");
        let json = s.to_json();
        assert!(json.contains("\\u0001"));
        let back = SavedState::from_json(&json).unwrap();
        assert_eq!(back.get_text("t"), Some("a\u{1}b"));
    }

    /// 负数与浮点边界。
    #[test]
    fn negative_and_float_edge() {
        let mut s = SavedState::new();
        s.put_float("neg", -0.5);
        s.put_int("negi", -42);
        let back = SavedState::from_json(&s.to_json()).unwrap();
        assert_eq!(back.get_float("neg"), Some(-0.5));
        assert_eq!(back.get_int("negi"), Some(-42));
    }

    /// 类型不匹配返回 None（浮点键当文本读）。
    #[test]
    fn type_mismatch_returns_none() {
        let mut s = SavedState::new();
        s.put_float("f", 1.0);
        assert_eq!(s.get_text("f"), None);
        assert_eq!(s.get_int("f"), None);
    }

    /// 同键后写覆盖（合并语义）。
    #[test]
    fn last_write_wins() {
        let mut s = SavedState::new();
        s.put_text("k", "old");
        s.put_text("k", "new");
        assert_eq!(s.get_text("k"), Some("new"));
        assert_eq!(s.len(), 1);
    }
}
