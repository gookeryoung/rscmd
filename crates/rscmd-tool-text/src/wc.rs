//! `wc`：行 / 词 / 字符 / 字节统计。

use clap::{ArgMatches, Command};
use rscmd_core::{
    command_for, PythonExposure, RscmdError, SideEffects, ToolCtx, ToolDef, ToolMeta,
};
use rscmd_runtime::text::stats;
use serde_json::{json, Value};

pub static META: ToolMeta = ToolMeta {
    name: "wc",
    aliases: &[],
    summary: "统计文本的行数、词数、字符数与字节数",
    side_effects: SideEffects::None,
    gui: false,
    python: PythonExposure::Structured,
    examples: &["rscmd wc \"hello world\""],
};

#[derive(Debug, clap::Args, serde::Deserialize, schemars::JsonSchema)]
pub struct WcArgs {
    /// 待统计的文本
    #[arg(value_name = "TEXT")]
    pub text: String,
}

pub fn build() -> Command {
    command_for::<WcArgs>(&META)
}

pub fn run_cli(ctx: &mut ToolCtx, matches: &ArgMatches) -> Result<Value, RscmdError> {
    let args = <WcArgs as clap::FromArgMatches>::from_arg_matches(matches)
        .map_err(RscmdError::from_clap)?;
    run(ctx, args)
}

pub fn run_json(ctx: &mut ToolCtx, input: &Value) -> Result<Value, RscmdError> {
    let args: WcArgs = serde_json::from_value(input.clone())
        .map_err(|err| RscmdError::coded("INVALID_ARGS", err.to_string()).with_field("args"))?;
    run(ctx, args)
}

fn run(_ctx: &mut ToolCtx, args: WcArgs) -> Result<Value, RscmdError> {
    let stats = stats(&args.text);
    Ok(json!({
        "lines": stats.lines,
        "words": stats.words,
        "chars": stats.chars,
        "bytes": stats.bytes,
    }))
}

pub fn def() -> ToolDef {
    ToolDef {
        meta: &META,
        build,
        run: run_cli,
        run_json: Some(run_json),
        schema: Some(wc_args_schema),
    }
}

fn wc_args_schema() -> String {
    let schema = schemars::schema_for!(WcArgs);
    serde_json::to_string_pretty(&schema).expect("WcArgs schema 可序列化")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(args: &[&str]) -> ArgMatches {
        build().try_get_matches_from(args).expect("参数应可解析")
    }

    #[test]
    fn cli_counts_text() {
        let out = run_cli(&mut ToolCtx::default(), &matches(&["wc", "a b\nc"])).unwrap();
        assert_eq!(out["lines"], 2);
        assert_eq!(out["words"], 3);
    }

    #[test]
    fn json_entry_counts_text() {
        let out = run_json(&mut ToolCtx::default(), &json!({"text": "hello"})).unwrap();
        assert_eq!(out["chars"], 5);
        assert_eq!(out["bytes"], 5);
    }

    #[test]
    fn json_entry_rejects_missing_text() {
        let err = run_json(&mut ToolCtx::default(), &json!({})).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
    }
}
