//! 统一错误分类与退出码。

use std::fmt;

/// 错误大类，与退出码一一对应。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Runtime,
    Usage,
    InvalidInput,
    NotFound,
    CheckFailed,
    Interrupted,
}

/// 具体错误码 → 错误大类的中央登记表。新增错误码必须在此登记。
pub const ERROR_CODE_KINDS: &[(&str, ErrorKind)] = &[
    // 用法错误
    ("USAGE", ErrorKind::Usage),
    ("MISSING_TOOL", ErrorKind::Usage),
    ("CONFIRM_REQUIRED", ErrorKind::Usage),
    // 目标不存在
    ("TOOL_NOT_FOUND", ErrorKind::NotFound),
    ("RESOURCE_NOT_FOUND", ErrorKind::NotFound),
    // 参数值不合法
    ("INVALID_ARGS", ErrorKind::InvalidInput),
    ("INVALID_MODE", ErrorKind::InvalidInput),
    ("INVALID_JSON", ErrorKind::InvalidInput),
    ("INVALID_ENCODING", ErrorKind::InvalidInput),
    ("INVALID_REGEX", ErrorKind::InvalidInput),
    ("PARSE_FAILED", ErrorKind::InvalidInput),
    // GUI 前端（rscmd-gui）未构建或路径无效
    ("GUI_UNAVAILABLE", ErrorKind::Runtime),
    // 运行期失败
    ("TIMEOUT", ErrorKind::Runtime),
    ("IO", ErrorKind::Runtime),
    // 网络与下载（与 rscmd-runtime 的 DownloadError::code 对应）
    ("NET_TIMEOUT", ErrorKind::Runtime),
    ("NET_HTTP_STATUS", ErrorKind::Runtime),
    ("NET_TRANSPORT", ErrorKind::Runtime),
    ("UNSUPPORTED_SCHEME", ErrorKind::InvalidInput),
    ("RANGE_NOT_SUPPORTED", ErrorKind::Runtime),
    ("RANGE_NOT_SATISFIABLE", ErrorKind::InvalidInput),
    ("CHECKSUM_MISMATCH", ErrorKind::Runtime),
    // 跨进程目录锁（与 rscmd-runtime 的 LockError::code 对应）
    ("LOCK_BUSY", ErrorKind::Runtime),
    ("LOCK_TIMEOUT", ErrorKind::Runtime),
    ("LOCK_UNAVAILABLE", ErrorKind::Runtime),
    // 用户中断（Ctrl+C），退出码 130
    ("INTERRUPTED", ErrorKind::Interrupted),
];

impl ErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Runtime => "runtime",
            Self::Usage => "usage",
            Self::InvalidInput => "invalid_input",
            Self::NotFound => "not_found",
            Self::CheckFailed => "check_failed",
            Self::Interrupted => "interrupted",
        }
    }

    /// 按登记表推导错误码对应的大类；未登记码回退 `InvalidInput`（与 fcmd 口径一致）。
    pub fn for_code(code: &str) -> Self {
        ERROR_CODE_KINDS
            .iter()
            .find(|(name, _)| *name == code)
            .map(|(_, kind)| *kind)
            .unwrap_or(Self::InvalidInput)
    }

    pub fn exit_code(self) -> i32 {
        match self {
            Self::Runtime => 1,
            Self::Usage => 2,
            Self::InvalidInput => 3,
            Self::NotFound => 4,
            Self::CheckFailed => 5,
            Self::Interrupted => 130,
        }
    }
}

/// 面向调用方的结构化错误。
#[derive(Debug, Clone)]
pub struct RscmdError {
    pub kind: ErrorKind,
    pub code: String,
    pub message: String,
    pub field: Option<String>,
    pub expected: Option<String>,
    pub hint: Option<String>,
}

impl RscmdError {
    pub fn new(kind: ErrorKind, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind,
            code: code.into(),
            message: message.into(),
            field: None,
            expected: None,
            hint: None,
        }
    }

    /// 由错误码推导 kind 与退出码，避免同一错误码在不同调用点给出不同 `kind`。
    /// 未登记码回退 `InvalidInput`（退出码 3）。
    pub fn coded(code: impl Into<String>, message: impl Into<String>) -> Self {
        let code = code.into();
        Self::new(ErrorKind::for_code(&code), code, message)
    }

    pub fn runtime(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Runtime, code, message)
    }

    pub fn usage(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Usage, code, message)
    }

    pub fn invalid_input(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidInput, code, message)
    }

    pub fn not_found(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(ErrorKind::NotFound, code, message)
    }

    pub fn check_failed(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(ErrorKind::CheckFailed, code, message)
    }

    pub fn with_field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into());
        self
    }

    pub fn with_expected(mut self, expected: impl Into<String>) -> Self {
        self.expected = Some(expected.into());
        self
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// 把 clap 的解析错误转换为统一错误。
    pub fn from_clap(err: clap::Error) -> Self {
        Self::usage("USAGE", err.to_string().trim_end().to_string())
    }

    pub fn exit_code(&self) -> i32 {
        self.kind.exit_code()
    }
}

impl fmt::Display for RscmdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RscmdError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered_codes_map_to_their_kind() {
        for (code, kind) in ERROR_CODE_KINDS {
            assert_eq!(ErrorKind::for_code(code), *kind, "错误码 {code} 映射不一致");
            let err = RscmdError::coded(*code, "m");
            assert_eq!(err.kind, *kind);
            assert_eq!(err.code, *code);
            assert_eq!(err.exit_code(), kind.exit_code());
        }
    }

    #[test]
    fn registered_codes_are_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for (code, _) in ERROR_CODE_KINDS {
            assert!(seen.insert(*code), "错误码 {code} 重复登记");
        }
    }

    #[test]
    fn unregistered_code_falls_back_to_invalid_input() {
        let err = RscmdError::coded("SOMETHING_NEW", "m");
        assert_eq!(err.kind, ErrorKind::InvalidInput);
        assert_eq!(err.exit_code(), 3);
        assert_eq!(
            ErrorKind::for_code("SOMETHING_NEW").as_str(),
            "invalid_input"
        );
    }
}
