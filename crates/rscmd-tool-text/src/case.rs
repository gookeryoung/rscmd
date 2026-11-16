//! `case`：命名风格转换。

use clap::{ArgMatches, Command};
use rscmd_core::{
    command_for, PythonExposure, RscmdError, SideEffects, ToolCtx, ToolDef, ToolMeta,
};
use rscmd_runtime::text::{convert_case, CaseStyle};
use serde_json::{json, Value};

pub static META: ToolMeta = ToolMeta {
    name: "case",
    aliases: &["casetool"],
    summary: "命名风格转换（snake/camel/pascal/kebab）",
    side_effects: SideEffects::None,
    gui: false,
    python: PythonExposure::Structured,
    examples: &["rscmd case --mode snake HelloWorld"],
};

#[derive(Debug, Clone, Copy, Default)]
pub enum CaseMode {
    #[default]
    Snake,
    Camel,
    Pascal,
    Kebab,
}

impl From<CaseMode> for CaseStyle {
    fn from(value: CaseMode) -> Self {
        match value {
            CaseMode::Snake => Self::Snake,
            CaseMode::Camel => Self::Camel,
            CaseMode::Pascal => Self::Pascal,
            CaseMode::Kebab => Self::Kebab,
        }
    }
}

#[derive(Debug, clap::Args, serde::Deserialize, schemars::JsonSchema)]
pub struct CaseArgs {
    /// 待转换的字符串
    #[arg(value_name = "TEXT")]
    pub text: String,
    /// 转换目标：snake/camel/pascal/kebab
    #[arg(long, value_name = "MODE", default_value = "snake")]
    #[serde(default = "default_mode")]
    pub mode: String,
}

fn default_mode() -> String {
    "snake".to_string()
}

fn parse_mode(raw: &str) -> Result<CaseMode, RscmdError> {
    match raw {
        "snake" => Ok(CaseMode::Snake),
        "camel" => Ok(CaseMode::Camel),
        "pascal" => Ok(CaseMode::Pascal),
        "kebab" => Ok(CaseMode::Kebab),
        other => Err(
            RscmdError::coded("INVALID_MODE", format!("未知命名风格: {other}"))
                .with_field("mode")
                .with_expected("snake/camel/pascal/kebab"),
        ),
    }
}

pub fn build() -> Command {
    command_for::<CaseArgs>(&META)
}

pub fn run_cli(ctx: &mut ToolCtx, matches: &ArgMatches) -> Result<Value, RscmdError> {
    let args = <CaseArgs as clap::FromArgMatches>::from_arg_matches(matches)
        .map_err(RscmdError::from_clap)?;
    run(ctx, args)
}

pub fn run_json(ctx: &mut ToolCtx, input: &Value) -> Result<Value, RscmdError> {
    let args: CaseArgs = serde_json::from_value(input.clone())
        .map_err(|err| RscmdError::coded("INVALID_ARGS", err.to_string()).with_field("args"))?;
    run(ctx, args)
}

fn run(_ctx: &mut ToolCtx, args: CaseArgs) -> Result<Value, RscmdError> {
    let mode = parse_mode(&args.mode)?;
    let text = convert_case(&args.text, mode.into());
    Ok(json!({ "text": text }))
}

pub fn def() -> ToolDef {
    ToolDef {
        meta: &META,
        build,
        run: run_cli,
        run_json: Some(run_json),
        schema: Some(case_args_schema),
    }
}

fn case_args_schema() -> String {
    let schema = schemars::schema_for!(CaseArgs);
    serde_json::to_string_pretty(&schema).expect("CaseArgs schema 可序列化")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(args: &[&str]) -> ArgMatches {
        build().try_get_matches_from(args).expect("参数应可解析")
    }

    #[test]
    fn cli_default_mode_is_snake() {
        let out = run_cli(&mut ToolCtx::default(), &matches(&["case", "HelloWorld"])).unwrap();
        assert_eq!(out["text"], "hello_world");
    }

    #[test]
    fn cli_accepts_every_mode() {
        for (mode, expected) in [
            ("camel", "helloWorld"),
            ("pascal", "HelloWorld"),
            ("kebab", "hello-world"),
            ("snake", "hello_world"),
        ] {
            let out = run_cli(
                &mut ToolCtx::default(),
                &matches(&["case", "HelloWorld", "--mode", mode]),
            )
            .unwrap();
            assert_eq!(out["text"], expected, "mode={mode}");
        }
    }

    #[test]
    fn json_entry_defaults_to_snake() {
        let out = run_json(&mut ToolCtx::default(), &json!({"text": "HelloWorld"})).unwrap();
        assert_eq!(out["text"], "hello_world");
    }

    #[test]
    fn json_entry_rejects_missing_text() {
        let err = run_json(&mut ToolCtx::default(), &json!({})).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("args"));
    }

    #[test]
    fn unknown_mode_is_invalid_input() {
        let err = parse_mode("dot").unwrap_err();
        assert_eq!(err.code, "INVALID_MODE");
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn def_exposes_structured_json_entry() {
        let tool = def();
        assert_eq!(tool.meta.name, "case");
        assert!(tool.run_json.is_some());
    }
}
