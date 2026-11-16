//! `which`：在 PATH 中查找可执行文件，返回绝对路径或 null。

use clap::{ArgMatches, Command};
use rscmd_core::{
    command_for, PythonExposure, RscmdError, SideEffects, ToolCtx, ToolDef, ToolMeta,
};
use rscmd_runtime::which::{find, WhichEnv};
use serde_json::{json, Value};

pub static META: ToolMeta = ToolMeta {
    name: "which",
    aliases: &["whichtool"],
    summary: "在 PATH 中查找可执行文件，返回绝对路径或 null",
    side_effects: SideEffects::Read,
    gui: false,
    python: PythonExposure::Structured,
    examples: &["rscmd which cargo", "rscmd which python3 --json"],
};

#[derive(Debug, clap::Args, serde::Deserialize, schemars::JsonSchema)]
pub struct WhichArgs {
    /// 要查找的程序名（可含相对或绝对路径）
    #[arg(value_name = "PROGRAM")]
    pub program: String,
}

pub fn build() -> Command {
    command_for::<WhichArgs>(&META)
}

pub fn run_cli(ctx: &mut ToolCtx, matches: &ArgMatches) -> Result<Value, RscmdError> {
    let args = <WhichArgs as clap::FromArgMatches>::from_arg_matches(matches)
        .map_err(RscmdError::from_clap)?;
    run(ctx, args)
}

pub fn run_json(ctx: &mut ToolCtx, input: &Value) -> Result<Value, RscmdError> {
    let args: WhichArgs = serde_json::from_value(input.clone())
        .map_err(|err| RscmdError::coded("INVALID_ARGS", err.to_string()).with_field("args"))?;
    run(ctx, args)
}

fn run(_ctx: &mut ToolCtx, args: WhichArgs) -> Result<Value, RscmdError> {
    if args.program.trim().is_empty() {
        return Err(RscmdError::coded("INVALID_ARGS", "PROGRAM 不能为空")
            .with_field("program")
            .with_expected("非空程序名"));
    }
    let found = find(&args.program, &WhichEnv::from_process())
        .map(|path| path.to_string_lossy().into_owned());
    Ok(json!({
        "program": args.program,
        "path": found,
    }))
}

pub fn def() -> ToolDef {
    ToolDef {
        meta: &META,
        build,
        run: run_cli,
        run_json: Some(run_json),
        schema: Some(which_args_schema),
    }
}

fn which_args_schema() -> String {
    let schema = schemars::schema_for!(WhichArgs);
    serde_json::to_string_pretty(&schema).expect("WhichArgs schema 可序列化")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(args: &[&str]) -> ArgMatches {
        build().try_get_matches_from(args).expect("参数应可解析")
    }

    #[test]
    fn missing_program_returns_null_path() {
        let out = run_cli(
            &mut ToolCtx::default(),
            &matches(&["which", "rscmd-definitely-missing-xyz"]),
        )
        .unwrap();
        assert_eq!(out["program"], "rscmd-definitely-missing-xyz");
        assert!(out["path"].is_null());
    }

    #[test]
    fn empty_program_is_invalid_args() {
        let err = run_cli(&mut ToolCtx::default(), &matches(&["which", " "])).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.exit_code(), 3);
        assert_eq!(err.field.as_deref(), Some("program"));
    }

    #[test]
    fn json_entry_matches_cli_shape() {
        let out = run_json(
            &mut ToolCtx::default(),
            &json!({"program": "rscmd-definitely-missing-xyz"}),
        )
        .unwrap();
        assert!(out["path"].is_null());
    }

    #[test]
    fn json_entry_rejects_missing_field() {
        let err = run_json(&mut ToolCtx::default(), &json!({})).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("args"));
    }

    #[test]
    fn def_exposes_structured_json_entry() {
        let tool = def();
        assert_eq!(tool.meta.name, "which");
        assert!(tool.run_json.is_some());
        let schema = (tool.schema.expect("应提供 schema"))();
        assert!(schema.contains("\"program\""), "{schema}");
    }
}
