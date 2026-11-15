//! CLI 全局参数的 Args 定义 + JsonSchema 导出。
//!
//! 工具参数 schema 由各工具 crate 的 `Args` 结构体各自 derive；
//! 全局参数独立一份，在 [`crate::dispatch::root_command`] 通过 `augment_args` 注入 root。
//! 所有字段用 `_rscmd_` 前缀的显式 id，避免与工具自身的参数 id 冲突。

use clap::Args;
use schemars::JsonSchema;
use serde::Serialize;

pub const GLOBAL_JSON: &str = "_rscmd_json";
pub const GLOBAL_QUIET: &str = "_rscmd_quiet";
pub const GLOBAL_DRY_RUN: &str = "_rscmd_dry_run";
pub const GLOBAL_CONFIRM: &str = "_rscmd_confirm";
pub const GLOBAL_WORKERS: &str = "_rscmd_workers";
pub const GLOBAL_LIMIT: &str = "_rscmd_limit";

/// CLI 全局参数（所有工具子命令共享）。
#[derive(Debug, Clone, Default, Args, JsonSchema, Serialize)]
pub struct GlobalArgs {
    /// 以 JSON 信封输出结果
    #[arg(long = "json", global = true, id = GLOBAL_JSON)]
    pub json: bool,

    /// 人读模式下不输出成功结果
    #[arg(long = "quiet", global = true, id = GLOBAL_QUIET)]
    pub quiet: bool,

    /// 仅预演，不产生副作用
    #[arg(long = "dry-run", global = true, id = GLOBAL_DRY_RUN)]
    pub dry_run: bool,

    /// 确认执行有副作用的操作
    #[arg(long = "confirm", global = true, id = GLOBAL_CONFIRM)]
    pub confirm: bool,

    /// 并行度，0 表示自动
    #[arg(long = "workers", global = true, id = GLOBAL_WORKERS, default_value_t = 0)]
    pub workers: usize,

    /// 结果条数上限，0 表示不限制
    #[arg(long = "limit", global = true, id = GLOBAL_LIMIT, default_value_t = 0)]
    pub limit: usize,
}

/// 全局参数的 JSON Schema，供 `schema/global.json` 生成物使用。
pub fn global_schema() -> String {
    let schema = schemars::schema_for!(GlobalArgs);
    serde_json::to_string_pretty(&schema).expect("GlobalArgs schema 可序列化")
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Debug, Parser)]
    struct Cli {
        #[command(flatten)]
        global: GlobalArgs,
    }

    #[test]
    fn global_args_defaults() {
        let cli = Cli::try_parse_from(["x"]).unwrap();
        assert!(!cli.global.json);
        assert!(!cli.global.quiet);
        assert!(!cli.global.dry_run);
        assert!(!cli.global.confirm);
        assert_eq!(cli.global.workers, 0);
        assert_eq!(cli.global.limit, 0);
    }

    #[test]
    fn global_args_parses_flags() {
        let cli = Cli::try_parse_from(["x", "--json", "--workers", "4", "--limit", "10"]).unwrap();
        assert!(cli.global.json);
        assert_eq!(cli.global.workers, 4);
        assert_eq!(cli.global.limit, 10);
    }

    #[test]
    fn global_schema_is_valid_json() {
        let raw = global_schema();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed["title"], "GlobalArgs");
    }
}
