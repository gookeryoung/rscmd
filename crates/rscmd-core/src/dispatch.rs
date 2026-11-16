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

use std::time::{Duration, Instant};

use crate::args::{
    GlobalArgs, GLOBAL_CONFIRM, GLOBAL_DRY_RUN, GLOBAL_JSON, GLOBAL_LIMIT, GLOBAL_QUIET,
    GLOBAL_TIMEOUT, GLOBAL_VERBOSE, GLOBAL_WORKERS,
};
use crate::builtins;
use crate::ctx::ToolCtx;
use crate::envelope;
use crate::errors::RscmdError;
use crate::registry::Registry;
use crate::tool::{meta_json, ToolDef};

pub struct Outcome {
    pub tool: String,
    /// 工具自身的下级子命令；空串表示该工具没有子命令（大多数工具如此）。
    pub subcommand: String,
    pub result: Result<Value, RscmdError>,
    pub json: bool,
    pub quiet: bool,
    /// `--verbose`：在 stderr 输出执行跟踪，不影响 stdout 契约。
    pub verbose: bool,
    /// 从参数解析到工具执行完成的耗时（毫秒）。
    pub elapsed_ms: u128,
}

impl Outcome {
    /// 结果信封里的 `subcommand`：仅当工具带下级子命令时返回 `Some`。
    pub fn subcommand(&self) -> Option<&str> {
        (!self.subcommand.is_empty()).then_some(self.subcommand.as_str())
    }

    pub fn exit_code(&self) -> i32 {
        match &self.result {
            Ok(data) if builtins::check_failed(&self.tool, data) => {
                crate::errors::ErrorKind::CheckFailed.exit_code()
            }
            Ok(_) => 0,
            Err(err) => err.exit_code(),
        }
    }

    /// 用户按下 Ctrl+C 时的统一结果：`kind=interrupted`、退出码 130。
    /// 只保留 stdout 契约相关的 `--json`/`--quiet` 判定，不输出 verbose 跟踪。
    pub fn interrupted(flags: &RawFlags) -> Self {
        Self {
            tool: String::new(),
            subcommand: String::new(),
            result: Err(RscmdError::coded("INTERRUPTED", "操作被用户中断（Ctrl+C）")),
            json: flags.json,
            quiet: false,
            verbose: false,
            elapsed_ms: 0,
        }
    }
}

/// 解析前的原始全局标记：解析失败或信号中断时也要知道输出模式。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RawFlags {
    pub json: bool,
    pub quiet: bool,
    pub verbose: bool,
}

/// 从裸 argv 里粗判全局输出标记（`--json` / `--quiet` / `--verbose`）。
pub fn scan_flags(argv: &[String]) -> RawFlags {
    RawFlags {
        json: argv.iter().any(|arg| arg == "--json"),
        quiet: argv.iter().any(|arg| arg == "--quiet"),
        verbose: argv.iter().any(|arg| arg == "--verbose" || arg == "-v"),
    }
}

/// 解析并执行一次调用。`argv` 不包含程序名。
pub fn dispatch(tools: &'static [ToolDef], argv: &[String]) -> Outcome {
    let started = Instant::now();
    let flags = scan_flags(argv);
    let raw_json = flags.json;
    let raw_quiet = flags.quiet;
    let raw_verbose = flags.verbose;
    let root = root_command(tools);
    let mut full = Vec::with_capacity(argv.len() + 1);
    full.push("rscmd".to_string());
    full.extend(argv.iter().cloned());

    match root.try_get_matches_from(full) {
        Ok(matches) => run_matches(tools, &matches, started),
        Err(err) => {
            let elapsed_ms = started.elapsed().as_millis();
            // --help / --version 属于正常输出，不是调用错误。
            if matches!(
                err.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) {
                let text = parenthesize_aliases(&err.to_string());
                return Outcome {
                    tool: String::new(),
                    subcommand: String::new(),
                    result: Ok(json!({ "text": text.trim_end() })),
                    json: raw_json,
                    quiet: raw_quiet,
                    verbose: raw_verbose,
                    elapsed_ms,
                };
            }
            Outcome {
                tool: String::new(),
                subcommand: String::new(),
                result: Err(RscmdError::coded(
                    "USAGE",
                    err.to_string().trim_end().to_string(),
                )),
                json: raw_json,
                quiet: raw_quiet,
                verbose: raw_verbose,
                elapsed_ms,
            }
        }
    }
}

fn root_command(tools: &'static [ToolDef]) -> Command {
    let mut root = Command::new("rscmd")
        .version(env!("CARGO_PKG_VERSION"))
        .disable_help_subcommand(true)
        .subcommand_required(false) // 无参数 → 显示帮助（默认行为）
        .arg_required_else_help(false);

    // 全局参数统一由 GlobalArgs derive 注入，id 带 `_rscmd_` 前缀避免与工具参数冲突。
    // derive 会把 GlobalArgs 的文档注释当作 about，因此注入后重新设置根命令简介。
    root = GlobalArgs::augment_args(root)
        .about("面向小模型调用的工具集")
        .after_help("示例:\n  rscmd run case --mode snake HelloWorld\n  rscmd doctor\n  rscmd version --json");

    // 1. 平级一级命令：run / tools / info / schema / version / doctor。
    //    短别名由 registry::BUILTINS 统一提供，帮助中显示为 `doctor(d)`。
    let mut run = builtin_command("run");
    for tool in tools {
        run = run.subcommand(tool_command(tool));
    }
    root = root.subcommand(run);

    // 2. 兼容：工具直接注册在 root 下（保留直达形式 `rscmd case`）
    for tool in tools {
        root = root.subcommand(tool_command(tool));
    }

    // 3. 其他平级命令
    root = root
        .subcommand(
            builtin_command("tools").arg(
                Arg::new("for-llm")
                    .long("for-llm")
                    .action(ArgAction::SetTrue)
                    .help("过滤掉 GUI 工具，供模型编排直接消费"),
            ),
        )
        .subcommand(builtin_command("info").arg(Arg::new("tool").required(true).value_name("TOOL")))
        .subcommand(builtin_command("schema"))
        .subcommand(builtin_command("version"))
        .subcommand(builtin_command("doctor"))
        .subcommand(
            builtin_command("completion").arg(
                Arg::new("shell")
                    .required(true)
                    .value_name("SHELL")
                    .value_parser(clap::value_parser!(clap_complete::Shell))
                    .help("目标 shell：bash / zsh / fish / powershell"),
            ),
        )
        .subcommand(builtin_command("mcp"));

    root
}

/// 按 `registry::BUILTINS` 构造内建子命令，别名以可见形式注册。
fn builtin_command(name: &str) -> Command {
    let meta = crate::registry::BUILTINS
        .iter()
        .find(|builtin| builtin.name == name)
        .unwrap_or_else(|| panic!("未登记的内建命令: {name}"));
    let mut cmd = Command::new(meta.name).about(meta.about);
    for alias in meta.aliases {
        cmd = cmd.visible_alias(*alias);
    }
    cmd
}

/// 工具子命令；工具别名同样以可见形式注册，便于帮助中展示。
fn tool_command(tool: &'static ToolDef) -> Command {
    let mut cmd = (tool.build)();
    for alias in tool.meta.aliases {
        cmd = cmd.visible_alias(*alias);
    }
    cmd
}

/// clap 把别名渲染成 `doctor  说明 [alias: d]`（追加在说明末尾）；
/// 这里按 `Commands:` 区块整体重排为 `doctor(d)  说明`，并重新对齐说明列；
/// 同时把内建命令与工具拆成 `Commands:` / `Tools:` 两组。
fn parenthesize_aliases(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut cursor = 0;

    while cursor < lines.len() {
        let line = lines[cursor];
        if line.trim() == "Commands:" {
            let start = cursor + 1;
            cursor = start;
            // 命令行固定缩进 2 格，续行缩进更深，遇空行或非缩进行结束。
            while cursor < lines.len()
                && !lines[cursor].trim().is_empty()
                && lines[cursor].starts_with("  ")
            {
                cursor += 1;
            }
            push_grouped_commands(&mut out, &lines[start..cursor]);
            continue;
        }
        out.push(line.to_string());
        cursor += 1;
    }

    out.join("\n")
}

fn push_grouped_commands(out: &mut Vec<String>, lines: &[&str]) {
    let (commands, tools): (Vec<_>, Vec<_>) = parse_command_block(lines)
        .into_iter()
        .partition(|entry| crate::registry::BUILTIN_COMMANDS.contains(&entry.name.as_str()));

    if !commands.is_empty() {
        out.push("Commands:".to_string());
        out.extend(render_command_entries(commands));
    }
    if !tools.is_empty() {
        if out.last().is_some_and(|line| !line.is_empty()) {
            out.push(String::new());
        }
        out.push("Tools:".to_string());
        out.extend(render_command_entries(tools));
    }
}

struct CommandEntry {
    name: String,
    aliases: Vec<String>,
    about: String,
}

impl CommandEntry {
    fn display(&self) -> String {
        if self.aliases.is_empty() {
            self.name.clone()
        } else {
            format!("{}({})", self.name, self.aliases.join(", "))
        }
    }
}

fn parse_command_block(lines: &[&str]) -> Vec<CommandEntry> {
    let mut entries: Vec<CommandEntry> = Vec::new();
    for line in lines {
        if let Some(raw) = line.strip_prefix("  ") {
            if !raw.starts_with(' ') {
                let (head, aliases) = split_alias(raw.trim_end());
                let name_end = head.find(char::is_whitespace).unwrap_or(head.len());
                entries.push(CommandEntry {
                    name: head[..name_end].to_string(),
                    aliases,
                    about: head[name_end..].trim().to_string(),
                });
                continue;
            }
        }
        // 说明换行后的续行，并入上一条命令。
        if let Some(last) = entries.last_mut() {
            if !last.about.is_empty() {
                last.about.push(' ');
            }
            last.about.push_str(line.trim());
        }
    }

    entries
}

fn render_command_entries(entries: Vec<CommandEntry>) -> Vec<String> {
    let width = entries
        .iter()
        .map(|entry| entry.display().chars().count())
        .max()
        .unwrap_or(0);
    entries
        .into_iter()
        .map(|entry| {
            let display = entry.display();
            if entry.about.is_empty() {
                format!("  {display}")
            } else {
                let pad = width.saturating_sub(display.chars().count()) + 2;
                format!("  {display}{}{}", " ".repeat(pad), entry.about)
            }
        })
        .collect()
}

/// 摘出 clap 追加在末尾的 `[alias: x]` / `[aliases: x, y]`。
fn split_alias(text: &str) -> (String, Vec<String>) {
    let Some(open) = text.find("[alias") else {
        return (text.to_string(), Vec::new());
    };
    let Some(close) = text[open..].find(']').map(|offset| open + offset) else {
        return (text.to_string(), Vec::new());
    };
    let Some(aliases) = text[open + 1..close]
        .strip_prefix("alias: ")
        .or_else(|| text[open + 1..close].strip_prefix("aliases: "))
    else {
        return (text.to_string(), Vec::new());
    };

    let cleaned = format!("{}{}", &text[..open], &text[close + 1..]);
    (
        cleaned.trim_end().to_string(),
        aliases.split(", ").map(str::to_string).collect(),
    )
}

fn run_matches(tools: &'static [ToolDef], matches: &clap::ArgMatches, started: Instant) -> Outcome {
    let (name, sub) = match matches.subcommand() {
        Some(pair) => pair,
        None => {
            // rscmd 不带任何参数 → 输出完整帮助（正常输出，exit=0）。
            // 帮助文本经 Outcome 返回，保证 CLI / Python 绑定 / --json 行为一致。
            let mut cmd = root_command(tools);
            let help = parenthesize_aliases(&cmd.render_help().to_string());
            return Outcome {
                tool: String::new(),
                subcommand: String::new(),
                result: Ok(json!({ "text": help })),
                json: matches.get_flag(GLOBAL_JSON),
                quiet: matches.get_flag(GLOBAL_QUIET),
                verbose: matches.get_flag(GLOBAL_VERBOSE),
                elapsed_ms: started.elapsed().as_millis(),
            };
        }
    };

    // root 级 + sub 级同名 flag 都算激活（兼容 `--json` 放在子命令后）
    let json = matches.get_flag(GLOBAL_JSON) || flag_in(sub, GLOBAL_JSON);
    let quiet = matches.get_flag(GLOBAL_QUIET) || flag_in(sub, GLOBAL_QUIET);
    let verbose = matches.get_flag(GLOBAL_VERBOSE) || flag_in(sub, GLOBAL_VERBOSE);
    let dry_run = matches.get_flag(GLOBAL_DRY_RUN) || flag_in(sub, GLOBAL_DRY_RUN);
    let confirm = matches.get_flag(GLOBAL_CONFIRM) || flag_in(sub, GLOBAL_CONFIRM);
    let workers = usize_in(matches, GLOBAL_WORKERS).max(usize_in(sub, GLOBAL_WORKERS));
    let limit_raw = usize_in(matches, GLOBAL_LIMIT).max(usize_in(sub, GLOBAL_LIMIT));
    let limit = if limit_raw == 0 {
        None
    } else {
        Some(limit_raw)
    };
    let timeout_secs = u64_in(matches, GLOBAL_TIMEOUT).max(u64_in(sub, GLOBAL_TIMEOUT));
    let timeout = if timeout_secs == 0 {
        None
    } else {
        Some(Duration::from_secs(timeout_secs))
    };

    // 构造 ToolCtx（供后续工具 invoke 共用）
    let mut ctx =
        ToolCtx::new(dry_run, confirm, quiet, limit, workers, verbose).with_timeout(timeout);

    // run 嵌套入口：再取一层子命令拿到真正的工具
    if name == "run" {
        if let Some((tool_name, tool_matches)) = sub.subcommand() {
            let result = invoke_tool(tools, tool_name, tool_matches, &mut ctx);
            return Outcome {
                tool: tool_name.to_string(),
                subcommand: nested_subcommand(tool_matches),
                result,
                json,
                quiet,
                verbose,
                elapsed_ms: started.elapsed().as_millis(),
            };
        }
        // rscmd run 不带工具名 → USAGE 错误
        return Outcome {
            tool: "run".to_string(),
            subcommand: String::new(),
            result: Err(RscmdError::coded(
                "MISSING_TOOL",
                "rscmd run 后需要一个工具名，例如 `rscmd run case --text hello`",
            )),
            json,
            quiet,
            verbose,
            elapsed_ms: started.elapsed().as_millis(),
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
                    RscmdError::coded("TOOL_NOT_FOUND", format!("未知工具: {wanted}"))
                        .with_hint("使用 `rscmd tools` 查看可用工具"),
                ),
            }
        }
        "schema" => Ok(schema_data(tools)),
        "completion" => Ok(completion_data(tools, sub)),
        // MCP 是阻塞式 stdio 服务，只能由 CLI 可执行文件进入；这里给出明确指引。
        "mcp" => Err(RscmdError::coded(
            "USAGE",
            "MCP server 需直接运行 `rscmd mcp`（stdin/stdout 协议）",
        )
        .with_hint("Python 绑定不支持 MCP 服务模式")),
        "version" => Ok(builtins::version_data()),
        "doctor" => Ok(builtins::doctor_data(
            tools,
            Registry::new(tools).validate(),
        )),
        _ => invoke_tool(tools, name, sub, &mut ctx),
    };

    Outcome {
        tool: name.to_string(),
        subcommand: nested_subcommand(sub),
        result,
        json,
        quiet,
        verbose,
        elapsed_ms: started.elapsed().as_millis(),
    }
}

/// 工具自身的下级子命令名；没有下级子命令（含全部内建命令）时返回空串。
fn nested_subcommand(matches: &clap::ArgMatches) -> String {
    matches.subcommand_name().unwrap_or_default().to_string()
}

/// `rscmd completion <SHELL>`：按完整命令树生成补全脚本，脚本原文放在 `text` 字段。
fn completion_data(tools: &'static [ToolDef], sub: &clap::ArgMatches) -> Value {
    let shell = sub
        .get_one::<clap_complete::Shell>("shell")
        .copied()
        .unwrap_or(clap_complete::Shell::Bash);
    let mut cmd = root_command(tools);
    let mut buf: Vec<u8> = Vec::new();
    clap_complete::generate(shell, &mut cmd, "rscmd", &mut buf);
    json!({ "text": String::from_utf8_lossy(&buf) })
}

fn invoke_tool(
    tools: &'static [ToolDef],
    name: &str,
    sub: &clap::ArgMatches,
    ctx: &mut ToolCtx,
) -> Result<Value, RscmdError> {
    match find_tool(tools, name) {
        Some(tool) => (tool.run)(ctx, sub),
        None => Err(RscmdError::coded(
            "TOOL_NOT_FOUND",
            format!("未知工具: {name}"),
        )),
    }
}

/// `rscmd schema` 的自描述输出：契约版本 + 工具 + 内建命令 + 全局参数 schema。
pub fn schema_data(tools: &'static [ToolDef]) -> Value {
    let global = serde_json::from_str(&crate::args::global_schema()).unwrap_or(Value::Null);
    json!({
        "schema": envelope::SCHEMA_CONTRACT,
        "version": env!("CARGO_PKG_VERSION"),
        "tools": tools.iter().map(meta_json).collect::<Vec<_>>(),
        "builtins": crate::registry::BUILTIN_COMMANDS,
        "global": global,
    })
}

/// `rscmd tools` 的 `data`：版本信封 + 工具元数据列表（每项含 `aliases` 与 `side_effects`）。
fn list_tools(tools: &'static [ToolDef], for_llm: bool) -> Result<Value, RscmdError> {
    let items: Vec<Value> = tools
        .iter()
        .filter(|tool| !for_llm || !tool.meta.gui)
        .map(meta_json)
        .collect();
    Ok(json!({
        "schema": envelope::TOOLS_CONTRACT,
        "version": env!("CARGO_PKG_VERSION"),
        "tools": items,
    }))
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

fn u64_in(matches: &clap::ArgMatches, name: &str) -> u64 {
    matches
        .try_get_one::<u64>(name)
        .ok()
        .flatten()
        .copied()
        .unwrap_or(0)
}

#[cfg(test)]
mod alias_render_tests {
    use super::*;

    #[test]
    fn alias_moves_next_to_name_and_realigns() {
        let input = "Commands:\n  run       运行指定工具 [alias: r]\n  doctor    输出运行环境自检信息 [alias: d]\n  tools     列出可用工具\n\nOptions:\n";
        let out = parenthesize_aliases(input);
        assert!(out.contains("Commands:\n"), "{out}");
        assert!(out.contains("run(r)"), "{out}");
        assert!(out.contains("doctor(d)"), "{out}");
        assert!(!out.contains("[alias"), "{out}");
        // 说明列对齐：两条带别名的命令说明起始列一致。
        let about_col = |command: &str, about: &str| {
            out.lines()
                .find(|line| line.contains(command))
                .unwrap()
                .find(about)
                .unwrap()
        };
        assert_eq!(
            about_col("run(r)", "运行指定工具"),
            about_col("doctor(d)", "输出运行环境自检信息")
        );
    }

    #[test]
    fn multiple_aliases_are_comma_separated() {
        let input = "Commands:\n  x    做某事 [aliases: a, b]\n";
        let out = parenthesize_aliases(input);
        assert!(out.contains("x(a, b)"), "{out}");
    }

    #[test]
    fn wrapped_about_is_merged_into_one_line() {
        let input = "Commands:\n  doctor    第一行说明 [alias: d]\n            第二行说明\n";
        let out = parenthesize_aliases(input);
        assert!(out.contains("doctor(d)  第一行说明 第二行说明"), "{out}");
    }

    #[test]
    fn split_alias_ignores_malformed_specs() {
        assert_eq!(
            split_alias("no alias here"),
            ("no alias here".to_string(), Vec::new())
        );
        // 缺少右括号。
        assert_eq!(
            split_alias("x 说明 [alias: d"),
            ("x 说明 [alias: d".to_string(), Vec::new())
        );
        // 不是 alias 前缀。
        assert_eq!(
            split_alias("x 说明 [aliasx: d]"),
            ("x 说明 [aliasx: d]".to_string(), Vec::new())
        );
        let (head, aliases) = split_alias("x 说明 [aliases: a, b]");
        assert_eq!(head, "x 说明");
        assert_eq!(aliases, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn command_without_about_gets_no_trailing_padding() {
        // `x` 不是内建命令，归入 Tools 组。
        let input = "Commands:\n  x\n";
        assert_eq!(parenthesize_aliases(input), "Tools:\n  x\n");
    }

    #[test]
    fn commands_and_tools_are_grouped() {
        let input = "Commands:\n  run       运行指定工具 [alias: r]\n  case      转换大小写 [alias: casetool]\n  doctor    环境自检 [alias: d]\n  wc        统计文本\n";
        let out = parenthesize_aliases(input);
        let commands_at = out.find("Commands:").expect(&out);
        let tools_at = out.find("Tools:").expect(&out);
        assert!(commands_at < tools_at, "{out}");
        assert!(out.find("run(r)").unwrap() < tools_at, "{out}");
        assert!(out.find("doctor(d)").unwrap() < tools_at, "{out}");
        assert!(out.find("case(casetool)").unwrap() > tools_at, "{out}");
        assert!(out.find("wc").unwrap() > tools_at, "{out}");
    }

    #[test]
    fn tools_only_block_uses_tools_heading() {
        let input = "Commands:\n  case    转换 [alias: casetool]\n";
        let out = parenthesize_aliases(input);
        assert!(!out.contains("Commands:"), "{out}");
        assert!(out.contains("Tools:"), "{out}");
    }

    #[test]
    fn builtins_metadata_matches_reserved_names() {
        let mut reserved = crate::registry::BUILTIN_COMMANDS.to_vec();
        reserved.sort_unstable();
        let mut declared: Vec<&str> = crate::registry::BUILTINS
            .iter()
            .map(|builtin| builtin.name)
            .collect();
        declared.sort_unstable();
        assert_eq!(reserved, declared);
    }
}
