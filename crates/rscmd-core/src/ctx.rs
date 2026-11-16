//! 工具运行上下文：运行期依赖全部从这里读取，工具本身保持无状态。

use std::time::Duration;

use crate::errors::RscmdError;

#[derive(Debug, Clone, Default)]
pub struct ToolCtx {
    pub dry_run: bool,
    pub confirm: bool,
    pub quiet: bool,
    pub limit: Option<usize>,
    pub workers: usize,
    pub verbose: bool,
    /// 全局 `--timeout` 解析结果；`None` 表示未设置，工具使用自身默认超时。
    pub timeout: Option<Duration>,
}

impl ToolCtx {
    pub fn new(
        dry_run: bool,
        confirm: bool,
        quiet: bool,
        limit: Option<usize>,
        workers: usize,
        verbose: bool,
    ) -> Self {
        Self {
            dry_run,
            confirm,
            quiet,
            limit,
            workers,
            verbose,
            timeout: None,
        }
    }

    /// 注入全局超时。`None` 表示调用方未指定 `--timeout`。
    pub fn with_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.timeout = timeout;
        self
    }

    /// 全局超时优先，未设置时回退到工具自身的默认超时。
    /// 子进程（`rscmd_runtime::proc`）与网络（`rscmd_runtime::net`）调用统一走这里取超时。
    pub fn timeout_or(&self, fallback: Duration) -> Duration {
        self.timeout.unwrap_or(fallback)
    }

    /// 写入/破坏性动作的确认闸门：`--dry-run` 直接放行（工具不得真正改动），
    /// 否则必须显式 `--confirm`，缺失时返回 `CONFIRM_REQUIRED`（usage，退出码 2）。
    pub fn require_confirm(&self, action: &str) -> Result<(), RscmdError> {
        if self.dry_run || self.confirm {
            return Ok(());
        }
        Err(
            RscmdError::coded("CONFIRM_REQUIRED", format!("{action}需要确认"))
                .with_hint("确认无风险后加 --confirm；只想查看计划加 --dry-run"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_timeout_is_absent() {
        let ctx = ToolCtx::default();
        assert_eq!(ctx.timeout, None);
        assert_eq!(
            ctx.timeout_or(Duration::from_secs(30)),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn global_timeout_overrides_tool_default() {
        let ctx = ToolCtx::new(false, false, false, None, 0, false)
            .with_timeout(Some(Duration::from_secs(5)));
        assert_eq!(ctx.timeout, Some(Duration::from_secs(5)));
        assert_eq!(
            ctx.timeout_or(Duration::from_secs(30)),
            Duration::from_secs(5)
        );
    }

    #[test]
    fn confirm_gate_requires_confirm() {
        let err = ToolCtx::default().require_confirm("写入文件").unwrap_err();
        assert_eq!(err.code, "CONFIRM_REQUIRED");
        assert_eq!(err.kind, crate::errors::ErrorKind::Usage);
        assert_eq!(err.exit_code(), 2);
        assert!(err.hint.is_some());
    }

    #[test]
    fn confirm_gate_passes_with_confirm() {
        let ctx = ToolCtx {
            confirm: true,
            ..ToolCtx::default()
        };
        assert!(ctx.require_confirm("写入文件").is_ok());
    }

    #[test]
    fn confirm_gate_skipped_in_dry_run() {
        let ctx = ToolCtx {
            dry_run: true,
            ..ToolCtx::default()
        };
        assert!(!ctx.confirm);
        assert!(ctx.require_confirm("写入文件").is_ok());
    }
}
