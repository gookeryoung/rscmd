//! 无头调度：CLI 与 Python 绑定共用同一套解析、执行与结果构造。
//!
//! ## 入口结构（v2026-11-15）
//!
//! 顶层平级：`run` / `doctor` / `version` / `info`（带短别名 `r/d/v/i`），
//! 工具统一走 `rscmd run <工具名> [参数]`。**同时保留直达形式** `rscmd <工具名> [参数]`
//! 作为兼容性别名，避免立即切断现有调用方。
//!
//! `rscmd` 不带参数 → 正常帮助（exit=0），不是 USAGE 错误。

use clap::{Arg, ArgAction, Args, Command};
use serde_json::{json, Value};

use crate::args::{
    GlobalArgs, GLOBAL_CONFIRM, GLOBAL_DRY_RUN, GLOBAL_JSON, GLOBAL_LIMIT, GLOBAL_QUIET,
    GLOBAL_WORKERS,
};
use crate::ctx::ToolCtx;
use crate::errors::RscmdError;
use crate::registry::BUILTIN_COMMANDS;
use crate::tool::{meta_json, ToolDef};

pub struct Outcome {
    pub tool: String,
    pub result: Result<Value, RscmdError>,
    pub json: bool,
    pub quiet: bool,
}

impl Outcome {
    pub fn exit_code(&self) -> i32 {
        match &self.result {
            Ok(_) => 0,
            Err(err) => err.exit_code(),
        }
    }
}

/// 解析并执行一次调用。`argv` 不包含程序名。
pub fn dispatch(tools: &'static [ToolDef], argv: &[String]) -> Outcome {
    let raw_json = argv.iter().any(|arg| arg == "--json");
    let raw_quiet = argv.iter().any(|arg| arg == "--quiet");
    let root = root_command(tools);
    let mut full = Vec::with_capacity(argv.len() + 1);
    full.push("rscmd".to_string());
    full.extend(argv.iter().cloned());

    match root.try_get_matches_from(full) {
        Ok(matches) => run_matches(tools, &matches),
        Err(err) => {
            // --help / --version 属于正常输出，不是调用错误。
            if matches!(
                err.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) {
                return Outcome {
                    tool: String::new(),
                    result: Ok(json!({ "text": err.to_string().trim_end() })),
                    json: raw_json,
                    quiet: raw_quiet,
                };
            }
            Outcome {
                tool: String::new(),
                result: Err(RscmdError::usage(
                    "USAGE",
                    err.to_string().trim_end().to_string(),
                )),
                json: raw_json,
                quiet: raw_quiet,
            }
        }
    }
}

fn root_command(tools: &'static [ToolDef]) -> Command {
    let mut root = Command::new("rscmd")
        .version(env!("CARGO_PKG_VERSION"))
        .about("面向小模型调用的工具集")
        .disable_help_subcommand(true)
        .subcommand_required(false) // 无参数 → 显示帮助（默认行为）
        .arg_required_else_help(false);

    // 全局参数统一由 GlobalArgs derive 注入，id 带 `_rscmd_` 前缀避免与工具参数冲突。
    root = GlobalArgs::augment_args(root);

    // 1. 平级一级命令：run / doctor / version / info（短别名 r/d/v/i）
    let mut run = Command::new("run")
        .alias("r")
        .about("运行指定工具（推荐入口）");
    for tool in tools {
        let mut tool_cmd = (tool.build)();
        for alias in tool.meta.aliases {
            tool_cmd = tool_cmd.alias(*alias);
        }
        run = run.subcommand(tool_cmd);
    }
    root = root.subcommand(run);

    // 2. 兼容：工具直接注册在 root 下（保留直达形式 `rscmd case`）
    for tool in tools {
        let mut tool_cmd = (tool.build)();
        for alias in tool.meta.aliases {
            tool_cmd = tool_cmd.alias(*alias);
        }
        root = root.subcommand(tool_cmd);
    }

    // 3. 其他平级命令
    root = root
        .subcommand(
            Command::new("tools").about("列出可用工具").arg(
                Arg::new("for-llm")
                    .long("for-llm")
                    .action(ArgAction::SetTrue)
                    .help("过滤掉 GUI 工具，供模型编排直接消费"),
            ),
        )
        .subcommand(
            Command::new("info")
                .alias("i")
                .about("查看单个工具的元数据")
                .arg(Arg::new("tool").required(true).value_name("TOOL")),
        )
        .subcommand(Command::new("schema").about("导出全部工具的元数据"))
        .subcommand(Command::new("version").alias("v").about("输出版本信息"))
        .subcommand(
            Command::new("doctor")
                .alias("d")
                .about("输出运行环境自检信息"),
        );

    root
}

fn run_matches(tools: &'static [ToolDef], matches: &clap::ArgMatches) -> Outcome {
    let (name, sub) = match matches.subcommand() {
        Some(pair) => pair,
        None => {
            // rscmd 不带任何参数 → 显示帮助（正常输出，exit=0）
            let mut cmd = root_command(tools);
            let _ = cmd.print_help();
            return Outcome {
                tool: String::new(),
                result: Ok(json!({ "text": "" })),
                json: matches.get_flag(GLOBAL_JSON),
                quiet: matches.get_flag(GLOBAL_QUIET),
            };
        }
    };

    // root 级 + sub 级同名 flag 都算激活（兼容 `--json` 放在子命令后）
    let json = matches.get_flag(GLOBAL_JSON) || flag_in(sub, GLOBAL_JSON);
    let quiet = matches.get_flag(GLOBAL_QUIET) || flag_in(sub, GLOBAL_QUIET);
    let dry_run = matches.get_flag(GLOBAL_DRY_RUN) || flag_in(sub, GLOBAL_DRY_RUN);
    let confirm = matches.get_flag(GLOBAL_CONFIRM) || flag_in(sub, GLOBAL_CONFIRM);
    let workers = usize_in(matches, GLOBAL_WORKERS).max(usize_in(sub, GLOBAL_WORKERS));
    let limit_raw = usize_in(matches, GLOBAL_LIMIT).max(usize_in(sub, GLOBAL_LIMIT));
    let limit = if limit_raw == 0 {
        None
    } else {
        Some(limit_raw)
    };

    // 构造 ToolCtx（供后续工具 invoke 共用）
    let mut ctx = ToolCtx::new(dry_run, confirm, quiet, limit, workers);

    // run 嵌套入口：再取一层子命令拿到真正的工具
    if name == "run" {
        if let Some((tool_name, tool_matches)) = sub.subcommand() {
            let result = invoke_tool(tools, tool_name, tool_matches, &mut ctx);
            return Outcome {
                tool: tool_name.to_string(),
                result,
                json,
                quiet,
            };
        }
        // rscmd run 不带工具名 → USAGE 错误
        return Outcome {
            tool: "run".to_string(),
            result: Err(RscmdError::usage(
                "MISSING_TOOL",
                "rscmd run 后需要一个工具名，例如 `rscmd run case --text hello`",
            )),
            json,
            quiet,
        };
    }

    let result = match name {
        "tools" => list_tools(tools, sub.get_flag("for-llm")),
        "info" => {
            let wanted = sub
                .get_one::<String>("tool")
                .map(String::as_str)
                .unwrap_or("");
            match find_tool(tools, wanted) {
                Some(tool) => Ok(meta_json(tool)),
                None => Err(
                    RscmdError::not_found("TOOL_NOT_FOUND", format!("未知工具: {wanted}"))
                        .with_hint("使用 `rscmd tools` 查看可用工具"),
                ),
            }
        }
        "schema" => Ok(Value::Array(
            tools.iter().map(meta_json).collect::<Vec<_>>(),
        )),
        "version" => Ok(json!({
            "name": "rscmd",
            "version": env!("CARGO_PKG_VERSION"),
        })),
        "doctor" => Ok(json!({
            "version": env!("CARGO_PKG_VERSION"),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "tool_count": tools.len(),
            "builtins": BUILTIN_COMMANDS,
        })),
        _ => invoke_tool(tools, name, sub, &mut ctx),
    };

    Outcome {
        tool: name.to_string(),
        result,
        json,
        quiet,
    }
}

fn invoke_tool(
    tools: &'static [ToolDef],
    name: &str,
    sub: &clap::ArgMatches,
    ctx: &mut ToolCtx,
) -> Result<Value, RscmdError> {
    match find_tool(tools, name) {
        Some(tool) => (tool.run)(ctx, sub),
        None => Err(RscmdError::not_found(
            "TOOL_NOT_FOUND",
            format!("未知工具: {name}"),
        )),
    }
}

fn list_tools(tools: &'static [ToolDef], for_llm: bool) -> Result<Value, RscmdError> {
    let items: Vec<Value> = tools
        .iter()
        .filter(|tool| !for_llm || !tool.meta.gui)
        .map(meta_json)
        .collect();
    Ok(Value::Array(items))
}

fn find_tool<'a>(tools: &'a [ToolDef], name: &str) -> Option<&'a ToolDef> {
    tools
        .iter()
        .find(|tool| tool.meta.name == name || tool.meta.aliases.contains(&name))
}

fn flag_in(matches: &clap::ArgMatches, name: &str) -> bool {
    matches.contains_id(name) && matches.get_flag(name)
}

fn usize_in(matches: &clap::ArgMatches, name: &str) -> usize {
    matches
        .try_get_one::<usize>(name)
        .ok()
        .flatten()
        .copied()
        .unwrap_or(0)
}
