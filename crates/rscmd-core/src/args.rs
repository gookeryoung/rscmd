//! CLI 全局参数的 Args 定义 + JsonSchema 导出。
//!
//! 工具参数 schema 由各工具 crate 的 `Args` 结构体各自 derive；
//! 全局参数独立一份，因为它们不属于任何具体工具，而是在 [`dispatch::root_command`]
//! 统一挂载到 root 上。

use clap::Args;
use schemars::JsonSchema;
use serde::Serialize;

/// CLI 全局参数（所有工具子命令共享）。
#[derive(Debug, Clone, Default, Args, JsonSchema, Serialize)]
pub struct GlobalArgs {
    /// 以 JSON 信封输出结果
    #[arg(long = "json", global = true)]
    pub json: bool,

    /// 人读模式下不输出成功结果
    #[arg(long = "quiet", global = true)]
    pub quiet: bool,

    /// 仅预演，不产生副作用
    #[arg(long = "dry-run", global = true)]
    pub dry_run: bool,

    /// 确认执行有副作用的操作
    #[arg(long = "confirm", global = true)]
    pub confirm: bool,

    /// 并行度，0 表示自动
    #[arg(long = "workers", global = true, default_value_t = 0)]
    pub workers: usize,

    /// 结果条数上限，0 表示不限制
    #[arg(long = "limit", global = true, default_value_t = 0)]
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
