//! 文本编解码基础件：base64 与 hex。
//!
//! 只处理标准字母表与带填充的 base64，不引入第三方依赖；
//! URL 编解码复用 [`crate::net`]（`urlencoding`）。
//! 解码失败统一返回 [`CodecError`]，由工具层映射为 `INVALID_ENCODING`。

use std::fmt;

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// 解码失败：非法字符、长度不符、填充位置错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodecError {
    message: String,
}

impl CodecError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CodecError {}

/// 标准 base64（RFC 4648，含 `=` 填充）编码。
pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let b2 = u32::from(chunk.get(2).copied().unwrap_or(0));
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(char::from(BASE64_ALPHABET[(triple >> 18) as usize & 0x3f]));
        out.push(char::from(BASE64_ALPHABET[(triple >> 12) as usize & 0x3f]));
        if chunk.len() > 1 {
            out.push(char::from(BASE64_ALPHABET[(triple >> 6) as usize & 0x3f]));
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(char::from(BASE64_ALPHABET[triple as usize & 0x3f]));
        } else {
            out.push('=');
        }
    }
    out
}

/// 标准 base64 解码。忽略 ASCII 空白，其余字符必须落在字母表或末尾填充内。
pub fn base64_decode(text: &str) -> Result<Vec<u8>, CodecError> {
    let cleaned: String = text
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect();
    let bytes = cleaned.as_bytes();
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if bytes.len() % 4 != 0 {
        return Err(CodecError::new(format!(
            "base64 长度必须是 4 的倍数（实际 {}）",
            bytes.len()
        )));
    }
    let chunks = bytes.len() / 4;
    let mut out = Vec::with_capacity(chunks * 3);
    for (index, chunk) in bytes.chunks(4).enumerate() {
        let mut values = [0u8; 4];
        let mut padding = 0usize;
        for (position, &byte) in chunk.iter().enumerate() {
            if byte == b'=' {
                // 填充只能出现在最后一块的最后两位，且一旦开始就不能再有数据。
                if position < 2 || index + 1 != chunks {
                    return Err(CodecError::new("base64 填充位置不合法"));
                }
                padding += 1;
            } else {
                if padding > 0 {
                    return Err(CodecError::new("base64 填充后不应再有字符"));
                }
                values[position] = base64_value(byte).ok_or_else(|| {
                    CodecError::new(format!("base64 含非法字符: {}", char::from(byte)))
                })?;
            }
        }
        let triple = (u32::from(values[0]) << 18)
            | (u32::from(values[1]) << 12)
            | (u32::from(values[2]) << 6)
            | u32::from(values[3]);
        out.push((triple >> 16) as u8);
        if padding < 2 {
            out.push((triple >> 8) as u8);
        }
        if padding < 1 {
            out.push(triple as u8);
        }
    }
    Ok(out)
}

fn base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// 小写 hex 编码。
pub fn hex_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len() * 2);
    for byte in data {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// hex 解码：接受大小写与可选的 `0x`/`0X` 前缀，空白按分隔忽略。
pub fn hex_decode(text: &str) -> Result<Vec<u8>, CodecError> {
    let cleaned: String = text
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect();
    let digits = cleaned
        .strip_prefix("0x")
        .or_else(|| cleaned.strip_prefix("0X"))
        .unwrap_or(&cleaned);
    if digits.len() % 2 != 0 {
        return Err(CodecError::new(format!(
            "hex 长度必须是偶数（实际 {}）",
            digits.len()
        )));
    }
    let bytes = digits.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks(2) {
        let high = hex_value(pair[0])
            .ok_or_else(|| CodecError::new(format!("hex 含非法字符: {}", char::from(pair[0]))))?;
        let low = hex_value(pair[1])
            .ok_or_else(|| CodecError::new(format!("hex 含非法字符: {}", char::from(pair[1]))))?;
        out.push((high << 4) | low);
    }
    Ok(out)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648_vectors() {
        for (input, expected) in [
            (b"".as_slice(), ""),
            (b"f", "Zg=="),
            (b"fo", "Zm8="),
            (b"foo", "Zm9v"),
            (b"foob", "Zm9vYg=="),
            (b"fooba", "Zm9vYmE="),
            (b"foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64_encode(input), expected);
        }
    }

    #[test]
    fn base64_roundtrips_every_byte() {
        let data: Vec<u8> = (0..=255).collect();
        let encoded = base64_encode(&data);
        assert_eq!(base64_decode(&encoded).unwrap(), data);
    }

    #[test]
    fn base64_decode_ignores_ascii_whitespace() {
        assert_eq!(base64_decode(" Zm9v\nYmFy ").unwrap(), b"foobar");
    }

    #[test]
    fn base64_decode_rejects_bad_length() {
        let err = base64_decode("abc").unwrap_err();
        assert!(err.to_string().contains("4 的倍数"), "{err}");
    }

    #[test]
    fn base64_decode_rejects_illegal_character() {
        let err = base64_decode("Zm9$").unwrap_err();
        assert!(err.to_string().contains("非法字符"), "{err}");
    }

    #[test]
    fn base64_decode_rejects_misplaced_padding() {
        for bad in ["Zg=a", "=Zm9", "Zm9vZg==Zm9v"] {
            assert!(base64_decode(bad).is_err(), "{bad} 应被拒绝");
        }
    }

    #[test]
    fn hex_matches_known_vectors() {
        assert_eq!(hex_encode(b"abc"), "616263");
        assert_eq!(hex_encode(b""), "");
        assert_eq!(hex_decode("616263").unwrap(), b"abc");
        assert_eq!(hex_decode("61 62 63").unwrap(), b"abc");
        assert_eq!(hex_decode("0x616263").unwrap(), b"abc");
        assert_eq!(
            hex_decode("616263").unwrap(),
            hex_decode("0X616263").unwrap()
        );
    }

    #[test]
    fn hex_roundtrips_every_byte() {
        let data: Vec<u8> = (0..=255).collect();
        assert_eq!(hex_decode(&hex_encode(&data)).unwrap(), data);
    }

    #[test]
    fn hex_decode_rejects_odd_length_and_bad_digit() {
        let err = hex_decode("616").unwrap_err();
        assert!(err.to_string().contains("偶数"), "{err}");
        let err = hex_decode("61zz").unwrap_err();
        assert!(err.to_string().contains("非法字符"), "{err}");
    }

    #[test]
    fn error_display_is_message() {
        let err = CodecError::new("坏输入");
        assert_eq!(err.to_string(), "坏输入");
    }
}
