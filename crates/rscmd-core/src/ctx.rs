//! 工具运行上下文：运行期依赖全部从这里读取，工具本身保持无状态。

#[derive(Debug, Clone, Default)]
pub struct ToolCtx {
    pub dry_run: bool,
    pub confirm: bool,
    pub quiet: bool,
    pub limit: Option<usize>,
    pub workers: usize,
}

impl ToolCtx {
    pub fn new(
        dry_run: bool,
        confirm: bool,
        quiet: bool,
        limit: Option<usize>,
        workers: usize,
    ) -> Self {
        Self {
            dry_run,
            confirm,
            quiet,
            limit,
            workers,
        }
    }
}
