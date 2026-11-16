//! 工具运行上下文：运行期依赖全部从这里读取，工具本身保持无状态。

use std::time::Duration;

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
}
