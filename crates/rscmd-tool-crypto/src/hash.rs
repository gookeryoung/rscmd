//! `hash`：计算文本或文件的 SHA-256。

use std::path::PathBuf;

use clap::{ArgMatches, Command};
use rscmd_core::{
    command_for, PythonExposure, RscmdError, SideEffects, ToolCtx, ToolDef, ToolMeta,
};
use rscmd_runtime::net::sha256_hex;
use serde_json::{json, Value};

pub static META: ToolMeta = ToolMeta {
    name: "hash",
    aliases: &["hashtool"],
    summary: "计算文本或文件的 SHA-256（hex）",
    side_effects: SideEffects::Read,
    gui: false,
    python: PythonExposure::Structured,
    examples: &["rscmd hash abc", "rscmd hash --file README.md"],
};

#[derive(Debug, clap::Args, serde::Deserialize, schemars::JsonSchema)]
pub struct HashArgs {
    /// 待计算摘要的文本（与 --file 二选一）
    #[arg(value_name = "TEXT")]
    #[serde(default)]
    pub text: Option<String>,
    /// 读取文件内容计算摘要（与 TEXT 二选一）
    #[arg(long, value_name = "PATH")]
    #[serde(default)]
    pub file: Option<PathBuf>,
}

pub fn build() -> Command {
    command_for::<HashArgs>(&META)
}

pub fn run_cli(ctx: &mut ToolCtx, matches: &ArgMatches) -> Result<Value, RscmdError> {
    let args = <HashArgs as clap::FromArgMatches>::from_arg_matches(matches)
        .map_err(RscmdError::from_clap)?;
    run(ctx, args)
}

pub fn run_json(ctx: &mut ToolCtx, input: &Value) -> Result<Value, RscmdError> {
    let args: HashArgs = serde_json::from_value(input.clone())
        .map_err(|err| RscmdError::coded("INVALID_ARGS", err.to_string()).with_field("args"))?;
    run(ctx, args)
}

fn run(_ctx: &mut ToolCtx, args: HashArgs) -> Result<Value, RscmdError> {
    let (bytes, source) = match (args.text, args.file) {
        (Some(text), None) => {
            let chars = text.chars().count();
            (text.into_bytes(), json!({ "kind": "text", "chars": chars }))
        }
        (None, Some(path)) => {
            let bytes = std::fs::read(&path).map_err(|err| {
                RscmdError::coded("IO", format!("读取文件失败: {err}")).with_field("file")
            })?;
            let source = json!({ "kind": "file", "path": path.display().to_string() });
            (bytes, source)
        }
        (Some(_), Some(_)) => {
            return Err(
                RscmdError::coded("INVALID_ARGS", "TEXT 与 --file 只能二选一")
                    .with_field("file")
                    .with_expected("只给 TEXT 或只给 --file"),
            );
        }
        (None, None) => {
            return Err(RscmdError::coded("INVALID_ARGS", "需要提供 TEXT 或 --file")
                .with_field("file")
                .with_expected("TEXT 或 --file"));
        }
    };

    let bytes_len = bytes.len();
    Ok(json!({
        "algorithm": "sha256",
        "hex": sha256_hex(&bytes),
        "bytes": bytes_len,
        "source": source,
    }))
}

pub fn def() -> ToolDef {
    ToolDef {
        meta: &META,
        build,
        run: run_cli,
        run_json: Some(run_json),
        schema: Some(hash_args_schema),
    }
}

fn hash_args_schema() -> String {
    let schema = schemars::schema_for!(HashArgs);
    serde_json::to_string_pretty(&schema).expect("HashArgs schema 可序列化")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(args: &[&str]) -> ArgMatches {
        build().try_get_matches_from(args).expect("参数应可解析")
    }

    #[test]
    fn hashes_text_with_known_vector() {
        let out = run_cli(&mut ToolCtx::default(), &matches(&["hash", "abc"])).unwrap();
        assert_eq!(
            out["hex"],
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(out["bytes"], 3);
        assert_eq!(out["algorithm"], "sha256");
        assert_eq!(out["source"]["kind"], "text");
        assert_eq!(out["source"]["chars"], 3);
    }

    #[test]
    fn hashes_file_content_with_byte_count() {
        let path = std::env::temp_dir().join(format!("rscmd-hash-{}.txt", std::process::id()));
        std::fs::write(&path, "abc").unwrap();
        let out = run_cli(
            &mut ToolCtx::default(),
            &matches(&["hash", "--file", path.to_str().unwrap()]),
        )
        .unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(
            out["hex"],
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(out["bytes"], 3);
        assert_eq!(out["source"]["kind"], "file");
    }

    #[test]
    fn rejects_text_and_file_together() {
        let err = run_cli(
            &mut ToolCtx::default(),
            &matches(&["hash", "abc", "--file", "nope.txt"]),
        )
        .unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.exit_code(), 3);
        assert_eq!(err.field.as_deref(), Some("file"));
    }

    #[test]
    fn rejects_missing_input() {
        let err = run_cli(&mut ToolCtx::default(), &matches(&["hash"])).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn missing_file_is_runtime_error() {
        let path = std::env::temp_dir().join("rscmd-hash-definitely-missing.txt");
        let err = run_cli(
            &mut ToolCtx::default(),
            &matches(&["hash", "--file", path.to_str().unwrap()]),
        )
        .unwrap_err();
        assert_eq!(err.code, "IO");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(err.field.as_deref(), Some("file"));
    }

    #[test]
    fn json_entry_accepts_text() {
        let out = run_json(&mut ToolCtx::default(), &json!({"text": "abc"})).unwrap();
        assert_eq!(out["bytes"], 3);
        assert_eq!(out["source"]["kind"], "text");
    }

    #[test]
    fn json_entry_rejects_empty_input() {
        let err = run_json(&mut ToolCtx::default(), &json!({})).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("file"));
    }

    #[test]
    fn def_exposes_structured_json_entry() {
        let tool = def();
        assert_eq!(tool.meta.name, "hash");
        assert!(tool.run_json.is_some());
        let schema = (tool.schema.expect("应提供 schema"))();
        assert!(schema.contains("\"text\""), "{schema}");
        assert!(schema.contains("\"file\""), "{schema}");
    }
}
