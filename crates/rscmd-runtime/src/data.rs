//! 结构化数据基础操作。

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonFormatError {
    pub message: String,
    pub line: usize,
    pub column: usize,
}

/// 校验并按需格式化 JSON。
pub fn format_json(input: &str, minify: bool) -> Result<String, JsonFormatError> {
    let value: serde_json::Value = serde_json::from_str(input).map_err(|err| JsonFormatError {
        message: err.to_string(),
        line: err.line(),
        column: err.column(),
    })?;

    let rendered = if minify {
        serde_json::to_string(&value)
    } else {
        serde_json::to_string_pretty(&value)
    };
    rendered.map_err(|err| JsonFormatError {
        message: err.to_string(),
        line: 0,
        column: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_prints_json() {
        let out = format_json(r#"{"a":1}"#, false).unwrap();
        assert!(out.contains("\n"));
        assert!(out.contains("\"a\""));
    }

    #[test]
    fn minifies_json() {
        let out = format_json("{ \"a\" : 1 }", true).unwrap();
        assert_eq!(out, r#"{"a":1}"#);
    }

    #[test]
    fn reports_invalid_json_position() {
        let err = format_json("{", false).unwrap_err();
        assert!(err.line >= 1);
        assert!(!err.message.is_empty());
    }
}
