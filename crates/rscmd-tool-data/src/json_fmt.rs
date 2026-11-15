//! `json_fmt`：校验并格式化 JSON。

use clap::{ArgMatches, Command};
use rscmd_core::{
    command_for, PythonExposure, RscmdError, SideEffects, ToolCtx, ToolDef, ToolMeta,
};
use rscmd_runtime::data::format_json;
use serde_json::{json, Value};

pub static META: ToolMeta = ToolMeta {
    name: "json_fmt",
    aliases: &["jsonfmt"],
    summary: "校验并格式化 JSON；非法输入返回可自纠的错误",
    side_effects: SideEffects::None,
    gui: false,
    python: PythonExposure::Structured,
    examples: &["rscmd json_fmt '{\"a\":1}'"],
};

#[derive(Debug, clap::Args, serde::Deserialize, schemars::JsonSchema)]
pub struct JsonFmtArgs {
    /// 待处理的 JSON 文本
    #[arg(value_name = "JSON")]
    pub json: String,
    /// 压缩为单行
    #[arg(long)]
    #[serde(default)]
    pub minify: bool,
}

pub fn build() -> Command {
    command_for::<JsonFmtArgs>(&META)
}

pub fn run_cli(ctx: &mut ToolCtx, matches: &ArgMatches) -> Result<Value, RscmdError> {
    let args = <JsonFmtArgs as clap::FromArgMatches>::from_arg_matches(matches)
        .map_err(RscmdError::from_clap)?;
    run(ctx, args)
}

pub fn run_json(ctx: &mut ToolCtx, input: &Value) -> Result<Value, RscmdError> {
    let args: JsonFmtArgs = serde_json::from_value(input.clone()).map_err(|err| {
        RscmdError::invalid_input("INVALID_ARGS", err.to_string()).with_field("args")
    })?;
    run(ctx, args)
}

fn run(_ctx: &mut ToolCtx, args: JsonFmtArgs) -> Result<Value, RscmdError> {
    match format_json(&args.json, args.minify) {
        Ok(rendered) => Ok(json!({ "json": rendered })),
        Err(err) => Err(RscmdError::invalid_input("INVALID_JSON", err.message)
            .with_field("json")
            .with_expected("合法的 JSON 文本")
            .with_hint(format!("第 {} 行第 {} 列附近", err.line, err.column))),
    }
}

pub fn def() -> ToolDef {
    ToolDef {
        meta: &META,
        build,
        run: run_cli,
        run_json: Some(run_json),
        schema: Some(json_fmt_args_schema),
    }
}

fn json_fmt_args_schema() -> String {
    let schema = schemars::schema_for!(JsonFmtArgs);
    serde_json::to_string_pretty(&schema).expect("JsonFmtArgs schema 可序列化")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(args: &[&str]) -> ArgMatches {
        build().try_get_matches_from(args).expect("参数应可解析")
    }

    #[test]
    fn cli_formats_json() {
        let out = run_cli(
            &mut ToolCtx::default(),
            &matches(&["json_fmt", "{\"a\":1}"]),
        )
        .unwrap();
        assert!(out["json"].as_str().unwrap().contains('\n'));
    }

    #[test]
    fn cli_minifies_json() {
        let out = run_cli(
            &mut ToolCtx::default(),
            &matches(&["json_fmt", "{ \"a\": 1 }", "--minify"]),
        )
        .unwrap();
        assert_eq!(out["json"], "{\"a\":1}");
    }

    #[test]
    fn cli_invalid_json_reports_position() {
        let err = run_cli(&mut ToolCtx::default(), &matches(&["json_fmt", "{"])).unwrap_err();
        assert_eq!(err.code, "INVALID_JSON");
        assert_eq!(err.field.as_deref(), Some("json"));
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn json_entry_rejects_missing_json() {
        let err = run_json(&mut ToolCtx::default(), &json!({})).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
    }
}
