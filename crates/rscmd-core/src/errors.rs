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
