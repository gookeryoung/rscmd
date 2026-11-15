//! 无头调度：CLI 与 Python 绑定共用同一套解析、执行与结果构造。

use clap::{Arg, ArgAction, ArgMatches, Command};
use serde_json::{json, Value};

use crate::ctx::ToolCtx;
use crate::errors::RscmdError;
use crate::registry::BUILTIN_COMMANDS;
use crate::tool::{meta_json, ToolDef};

/// 全局参数的内部 id 统一加前缀，避免与工具自身的参数 id 冲突。
const GLOBAL_JSON: &str = "_rscmd_json";
const GLOBAL_QUIET: &str = "_rscmd_quiet";
const GLOBAL_DRY_RUN: &str = "_rscmd_dry_run";
const GLOBAL_CONFIRM: &str = "_rscmd_confirm";
const GLOBAL_WORKERS: &str = "_rscmd_workers";
const GLOBAL_LIMIT: &str = "_rscmd_limit";

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
        .arg(flag(GLOBAL_JSON, "json", "以 JSON 信封输出"))
        .arg(flag(GLOBAL_QUIET, "quiet", "人读模式下不输出成功结果"))
        .arg(flag(GLOBAL_DRY_RUN, "dry-run", "仅预演，不产生副作用"))
        .arg(flag(GLOBAL_CONFIRM, "confirm", "确认执行有副作用的操作"))
        .arg(
            Arg::new(GLOBAL_WORKERS)
                .long("workers")
                .global(true)
                .value_name("N")
                .value_parser(clap::value_parser!(usize))
                .default_value("0")
                .help("并行度，0 表示自动"),
        )
        .arg(
            Arg::new(GLOBAL_LIMIT)
                .long("limit")
                .global(true)
                .value_name("N")
                .value_parser(clap::value_parser!(usize))
                .default_value("0")
                .help("结果条数上限，0 表示不限制"),
        );

    for tool in tools {
        let mut command = (tool.build)();
        for alias in tool.meta.aliases {
            command = command.alias(*alias);
        }
        root = root.subcommand(command);
    }

    root.subcommand(
        Command::new("tools").about("列出可用工具").arg(
            Arg::new("for-llm")
                .long("for-llm")
                .action(ArgAction::SetTrue)
                .help("过滤掉 GUI 工具，供模型编排直接消费"),
        ),
    )
    .subcommand(
        Command::new("info")
            .about("查看单个工具的元数据")
            .arg(Arg::new("tool").required(true).value_name("TOOL")),
    )
    .subcommand(Command::new("schema").about("导出全部工具的元数据"))
    .subcommand(Command::new("version").about("输出版本信息"))
    .subcommand(Command::new("doctor").about("输出运行环境自检信息"))
}

fn flag(id: &'static str, long: &'static str, help: &'static str) -> Arg {
    Arg::new(id)
        .long(long)
        .action(ArgAction::SetTrue)
        .global(true)
        .help(help)
}

fn run_matches(tools: &'static [ToolDef], matches: &ArgMatches) -> Outcome {
    let (name, sub) = match matches.subcommand() {
        Some(pair) => pair,
        None => {
            return Outcome {
                tool: String::new(),
                result: Err(RscmdError::usage(
                    "MISSING_COMMAND",
                    "请指定要运行的工具；使用 `rscmd tools` 查看可用工具",
                )),
                json: matches.get_flag(GLOBAL_JSON),
                quiet: matches.get_flag(GLOBAL_QUIET),
            }
        }
    };

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
        _ => match find_tool(tools, name) {
            Some(tool) => {
                let mut ctx = ToolCtx::new(dry_run, confirm, quiet, limit, workers);
                (tool.run)(&mut ctx, sub)
            }
            None => Err(RscmdError::not_found(
                "TOOL_NOT_FOUND",
                format!("未知工具: {name}"),
            )),
        },
    };

    Outcome {
        tool: name.to_string(),
        result,
        json,
        quiet,
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

fn flag_in(matches: &ArgMatches, name: &str) -> bool {
    matches.contains_id(name) && matches.get_flag(name)
}

fn usize_in(matches: &ArgMatches, name: &str) -> usize {
    matches
        .try_get_one::<usize>(name)
        .ok()
        .flatten()
        .copied()
        .unwrap_or(0)
}
