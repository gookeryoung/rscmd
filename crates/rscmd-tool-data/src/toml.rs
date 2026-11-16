//! `toml`：校验 TOML 并把内容转换为 JSON 或 YAML。
//!
//! 输入必须是 UTF-8 文件（可带 UTF-8 BOM，会被去掉）。日期时间按 TOML 原文渲染为字符串，
//! 不读系统时区、不做隐式本地转换，因此跨机器结果一致。
//!
//! 输出固定为 `{path, to, text}`；`--check` 只校验语法，输出 `{path, ok: true}`。
//! `--to yaml` 复用 `serde_norway`，不引入第二套 YAML 解析器。

use std::fs;
use std::path::Path;

use clap::{ArgMatches, Command};
use rscmd_core::{
    command_for, PythonExposure, RscmdError, SideEffects, ToolCtx, ToolDef, ToolMeta,
};
use serde_json::{json, Value};

pub static META: ToolMeta = ToolMeta {
    name: "toml",
    aliases: &["tomltool"],
    summary: "校验 TOML 并转换为 JSON 或 YAML",
    side_effects: SideEffects::Read,
    gui: false,
    python: PythonExposure::Structured,
    examples: &[
        "rscmd toml Cargo.toml",
        "rscmd toml Cargo.toml --check",
        "rscmd toml pyproject.toml --to yaml",
    ],
};

#[derive(Debug, clap::Args, serde::Deserialize, schemars::JsonSchema)]
pub struct TomlArgs {
    /// 待读取的 TOML 文件（UTF-8，可带 BOM）
    #[arg(value_name = "FILE")]
    pub file: String,
    /// 输出格式：json（默认）或 yaml
    #[arg(long, value_name = "FORMAT", value_parser = ["json", "yaml"])]
    #[serde(default)]
    pub to: Option<String>,
    /// 只校验语法，不输出转换结果
    #[arg(long, conflicts_with = "to")]
    #[serde(default)]
    pub check: bool,
}

pub fn build() -> Command {
    command_for::<TomlArgs>(&META)
}

pub fn run_cli(ctx: &mut ToolCtx, matches: &ArgMatches) -> Result<Value, RscmdError> {
    let args = <TomlArgs as clap::FromArgMatches>::from_arg_matches(matches)
        .map_err(RscmdError::from_clap)?;
    run(ctx, args)
}

pub fn run_json(ctx: &mut ToolCtx, input: &Value) -> Result<Value, RscmdError> {
    let args: TomlArgs = serde_json::from_value(input.clone())
        .map_err(|err| RscmdError::coded("INVALID_ARGS", err.to_string()).with_field("args"))?;
    run(ctx, args)
}

fn run(_ctx: &mut ToolCtx, args: TomlArgs) -> Result<Value, RscmdError> {
    let path = Path::new(&args.file);
    let text = read_utf8(path, &args.file)?;
    let parsed: toml::Value = toml::from_str(&text).map_err(|err| {
        parse_error(&args.file, format!("TOML 解析失败: {err}"))
            .with_hint("错误信息里的行列号指向文件中的位置")
    })?;
    if args.check {
        return Ok(json!({"path": args.file, "ok": true}));
    }

    let format = args.to.as_deref().unwrap_or("json");
    let converted = toml_to_json(parsed).map_err(|message| {
        parse_error(&args.file, message).with_hint("移除 TOML 中无法用 JSON 表示的取值")
    })?;
    let text = match format {
        "json" => serde_json::to_string_pretty(&converted).expect("JSON 值可序列化"),
        "yaml" => serde_norway::to_string(&converted).map_err(|err| {
            parse_error(&args.file, format!("YAML 渲染失败: {err}"))
                .with_hint("改用 --to json 查看可转换的字段")
        })?,
        other => {
            return Err(
                RscmdError::coded("INVALID_ARGS", format!("不支持的目标格式: {other}"))
                    .with_field("to")
                    .with_expected("json 或 yaml"),
            )
        }
    };
    Ok(json!({"path": args.file, "to": format, "text": text}))
}

/// 读取 UTF-8 文本并去掉可选的 BOM；非 UTF-8 单独报 `INVALID_ENCODING`。
pub(crate) fn read_utf8(path: &Path, display: &str) -> Result<String, RscmdError> {
    let bytes = fs::read(path).map_err(|err| {
        RscmdError::coded("IO", format!("无法读取文件: {err}"))
            .with_field("path")
            .with_expected("存在且可读的文件")
    })?;
    let text = String::from_utf8(bytes).map_err(|err| {
        RscmdError::coded("INVALID_ENCODING", format!("文件不是有效 UTF-8: {err}"))
            .with_field("path")
            .with_expected(format!("UTF-8 编码的文件: {display}"))
    })?;
    Ok(text
        .strip_prefix('\u{feff}')
        .map(str::to_string)
        .unwrap_or(text))
}

pub(crate) fn parse_error(path: &str, message: String) -> RscmdError {
    RscmdError::coded("PARSE_FAILED", message)
        .with_field("path")
        .with_expected(format!("可解析的格式: {path}"))
}

/// TOML 值转 JSON 值：日期时间写成 TOML 原文，浮点 `inf`/`nan` 无法用 JSON 表示。
fn toml_to_json(value: toml::Value) -> Result<Value, String> {
    match value {
        toml::Value::String(text) => Ok(Value::String(text)),
        toml::Value::Integer(number) => Ok(json!(number)),
        toml::Value::Float(number) => serde_json::Number::from_f64(number)
            .map(Value::Number)
            .ok_or_else(|| format!("浮点数 {number} 无法用 JSON 表示")),
        toml::Value::Boolean(flag) => Ok(json!(flag)),
        toml::Value::Datetime(datetime) => Ok(Value::String(datetime.to_string())),
        toml::Value::Array(items) => {
            let mut converted = Vec::with_capacity(items.len());
            for item in items {
                converted.push(toml_to_json(item)?);
            }
            Ok(Value::Array(converted))
        }
        toml::Value::Table(table) => {
            let mut converted = serde_json::Map::new();
            for (key, item) in table {
                converted.insert(key, toml_to_json(item)?);
            }
            Ok(Value::Object(converted))
        }
    }
}

pub fn def() -> ToolDef {
    ToolDef {
        meta: &META,
        build,
        run: run_cli,
        run_json: Some(run_json),
        schema: Some(toml_args_schema),
    }
}

fn toml_args_schema() -> String {
    let schema = schemars::schema_for!(TomlArgs);
    serde_json::to_string_pretty(&schema).expect("TomlArgs schema 可序列化")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rscmd_runtime::fs::temp_dir;

    fn matches(args: &[&str]) -> ArgMatches {
        build().try_get_matches_from(args).expect("参数应可解析")
    }

    fn write_file(text: &str) -> (rscmd_runtime::fs::TempDir, String) {
        let dir = temp_dir().expect("应创建临时目录");
        let path = dir.path().join("sample.toml");
        fs::write(&path, text).expect("应写入临时文件");
        (dir, path.to_string_lossy().into_owned())
    }

    #[test]
    fn converts_toml_to_json() {
        let (_dir, path) = write_file("title = \"demo\"\n[db]\nport = 5432\n");
        let out = run_cli(&mut ToolCtx::default(), &matches(&["toml", &path])).unwrap();
        assert_eq!(out["path"], path);
        assert_eq!(out["to"], "json");
        let text = out["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["title"], "demo");
        assert_eq!(parsed["db"]["port"], 5432);
    }

    #[test]
    fn converts_toml_to_yaml() {
        let (_dir, path) = write_file("title = \"demo\"\ncount = 2\n");
        let out = run_cli(
            &mut ToolCtx::default(),
            &matches(&["toml", &path, "--to", "yaml"]),
        )
        .unwrap();
        let text = out["text"].as_str().unwrap();
        assert!(text.contains("title: demo"), "{text}");
        assert!(text.contains("count: 2"), "{text}");
    }

    #[test]
    fn datetime_is_rendered_as_source_text() {
        let (_dir, path) = write_file("when = 2024-02-29T12:00:00Z\n");
        let out = run_json(&mut ToolCtx::default(), &json!({"file": path})).unwrap();
        let parsed: Value = serde_json::from_str(out["text"].as_str().unwrap()).unwrap();
        assert_eq!(parsed["when"], "2024-02-29T12:00:00Z");
    }

    #[test]
    fn inf_float_is_parse_failed() {
        let (_dir, path) = write_file("value = inf\n");
        let err = run_cli(&mut ToolCtx::default(), &matches(&["toml", &path])).unwrap_err();
        assert_eq!(err.code, "PARSE_FAILED");
        assert_eq!(err.exit_code(), 3);
        assert_eq!(err.field.as_deref(), Some("path"));
        assert!(err.hint.is_some());
    }

    #[test]
    fn invalid_toml_is_parse_failed() {
        let (_dir, path) = write_file("key = [1,\n");
        let err = run_cli(&mut ToolCtx::default(), &matches(&["toml", &path])).unwrap_err();
        assert_eq!(err.code, "PARSE_FAILED");
        assert_eq!(err.field.as_deref(), Some("path"));
    }

    #[test]
    fn check_only_validates() {
        let (_dir, path) = write_file("a = 1\n");
        let out = run_cli(
            &mut ToolCtx::default(),
            &matches(&["toml", &path, "--check"]),
        )
        .unwrap();
        assert_eq!(out, json!({"path": path, "ok": true}));
    }

    #[test]
    fn check_conflicts_with_to() {
        let (_dir, path) = write_file("a = 1\n");
        let err = build()
            .try_get_matches_from(["toml", &path, "--check", "--to", "json"])
            .unwrap_err();
        assert!(err.to_string().contains("--check"), "{err}");
    }

    #[test]
    fn json_entry_rejects_unknown_format() {
        let (_dir, path) = write_file("a = 1\n");
        let err =
            run_json(&mut ToolCtx::default(), &json!({"file": path, "to": "xml"})).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("to"));
    }

    #[test]
    fn bom_is_stripped() {
        let (_dir, path) = write_file("\u{feff}a = 1\n");
        let out = run_cli(&mut ToolCtx::default(), &matches(&["toml", &path])).unwrap();
        let parsed: Value = serde_json::from_str(out["text"].as_str().unwrap()).unwrap();
        assert_eq!(parsed["a"], 1);
    }

    #[test]
    fn non_utf8_file_is_invalid_encoding() {
        let dir = temp_dir().unwrap();
        let path = dir.path().join("bad.toml");
        fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
        let display = path.to_string_lossy().into_owned();
        let err = run_cli(&mut ToolCtx::default(), &matches(&["toml", &display])).unwrap_err();
        assert_eq!(err.code, "INVALID_ENCODING");
        assert_eq!(err.field.as_deref(), Some("path"));
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn missing_file_is_io_error() {
        let (_dir, path) = write_file("a = 1\n");
        let missing = format!("{path}.missing");
        let err = run_cli(&mut ToolCtx::default(), &matches(&["toml", &missing])).unwrap_err();
        assert_eq!(err.code, "IO");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(err.field.as_deref(), Some("path"));
    }

    #[test]
    fn cli_and_json_paths_agree() {
        let (_dir, path) = write_file("a = 1\n");
        let cli = run_cli(&mut ToolCtx::default(), &matches(&["toml", &path])).unwrap();
        let json_out = run_json(&mut ToolCtx::default(), &json!({"file": path})).unwrap();
        assert_eq!(cli, json_out);
    }

    #[test]
    fn json_entry_rejects_missing_file_field() {
        let err = run_json(&mut ToolCtx::default(), &json!({})).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("args"));
    }

    #[test]
    fn def_exposes_schema_and_metadata() {
        let tool = def();
        assert_eq!(tool.meta.name, "toml");
        assert_eq!(tool.meta.aliases, &["tomltool"]);
        assert_eq!(tool.meta.side_effects, SideEffects::Read);
        assert!(tool.run_json.is_some());
        let schema = (tool.schema.expect("应提供 schema"))();
        for field in ["\"file\"", "\"to\"", "\"check\""] {
            assert!(schema.contains(field), "{field} 不在 schema: {schema}");
        }
    }
}
