//! `yaml`：校验 YAML 并把内容转换为 JSON 或 TOML。
//!
//! 输入必须是 UTF-8 文件（可带 UTF-8 BOM，会被去掉）。YAML 的键必须是字符串：
//! 非字符串键与重复键都会让结果无法无损转换为 JSON/TOML，统一返回 `PARSE_FAILED`。
//! 转换为 TOML 时，`null` 与顶层不是键值对同样无法表示，也返回 `PARSE_FAILED`。
//!
//! 输出固定为 `{path, to, text}`；`--check` 只校验语法，输出 `{path, ok: true}`。

use std::path::Path;

use clap::{ArgMatches, Command};
use rscmd_core::{
    command_for, PythonExposure, RscmdError, SideEffects, ToolCtx, ToolDef, ToolMeta,
};
use serde_json::{json, Value};
use serde_norway::Value as Yaml;

use crate::toml::{parse_error, read_utf8};

pub static META: ToolMeta = ToolMeta {
    name: "yaml",
    aliases: &["yamtool"],
    summary: "校验 YAML 并转换为 JSON 或 TOML",
    side_effects: SideEffects::Read,
    gui: false,
    python: PythonExposure::Structured,
    examples: &[
        "rscmd yaml config.yaml",
        "rscmd yaml config.yaml --check",
        "rscmd yaml config.yaml --to toml",
    ],
};

#[derive(Debug, clap::Args, serde::Deserialize, schemars::JsonSchema)]
pub struct YamlArgs {
    /// 待读取的 YAML 文件（UTF-8，可带 BOM）
    #[arg(value_name = "FILE")]
    pub file: String,
    /// 输出格式：json（默认）或 toml
    #[arg(long, value_name = "FORMAT", value_parser = ["json", "toml"])]
    #[serde(default)]
    pub to: Option<String>,
    /// 只校验语法，不输出转换结果
    #[arg(long, conflicts_with = "to")]
    #[serde(default)]
    pub check: bool,
}

/// 源值无法用目标格式表示：消息说明原因，hint 说明怎么改。
struct ConvertError {
    message: String,
    hint: &'static str,
}

impl ConvertError {
    fn new(message: String, hint: &'static str) -> Self {
        Self { message, hint }
    }

    fn into_rscmd(self, path: &str) -> RscmdError {
        parse_error(path, self.message).with_hint(self.hint)
    }
}

pub fn build() -> Command {
    command_for::<YamlArgs>(&META)
}

pub fn run_cli(ctx: &mut ToolCtx, matches: &ArgMatches) -> Result<Value, RscmdError> {
    let args = <YamlArgs as clap::FromArgMatches>::from_arg_matches(matches)
        .map_err(RscmdError::from_clap)?;
    run(ctx, args)
}

pub fn run_json(ctx: &mut ToolCtx, input: &Value) -> Result<Value, RscmdError> {
    let args: YamlArgs = serde_json::from_value(input.clone())
        .map_err(|err| RscmdError::coded("INVALID_ARGS", err.to_string()).with_field("args"))?;
    run(ctx, args)
}

fn run(_ctx: &mut ToolCtx, args: YamlArgs) -> Result<Value, RscmdError> {
    let path = Path::new(&args.file);
    let text = read_utf8(path, &args.file)?;
    let parsed: Yaml = serde_norway::from_str(&text).map_err(|err| {
        let duplicate = err.to_string().to_lowercase().contains("duplicate");
        let hint = if duplicate {
            "删除重复键，或保留一个键并把其余取值放进数组"
        } else {
            "错误信息里的行列号指向文件中的位置"
        };
        parse_error(&args.file, format!("YAML 解析失败: {err}")).with_hint(hint)
    })?;
    if args.check {
        return Ok(json!({"path": args.file, "ok": true}));
    }

    let format = args.to.as_deref().unwrap_or("json");
    let text = match format {
        "json" => {
            let converted = yaml_to_json(parsed).map_err(|err| err.into_rscmd(&args.file))?;
            serde_json::to_string_pretty(&converted).expect("JSON 值可序列化")
        }
        "toml" => {
            let converted = yaml_to_toml(parsed).map_err(|err| err.into_rscmd(&args.file))?;
            toml::to_string(&converted).map_err(|err| {
                parse_error(&args.file, format!("TOML 渲染失败: {err}"))
                    .with_hint("改用 --to json 查看该文件")
            })?
        }
        other => {
            return Err(
                RscmdError::coded("INVALID_ARGS", format!("不支持的目标格式: {other}"))
                    .with_field("to")
                    .with_expected("json 或 toml"),
            )
        }
    };
    Ok(json!({"path": args.file, "to": format, "text": text}))
}

/// YAML 值转 JSON 值：键必须是字符串，非有限浮点无法用 JSON 表示。
fn yaml_to_json(value: Yaml) -> Result<Value, ConvertError> {
    match value {
        Yaml::Null => Ok(Value::Null),
        Yaml::Bool(flag) => Ok(json!(flag)),
        Yaml::Number(number) => number_to_json(number),
        Yaml::String(text) => Ok(Value::String(text)),
        Yaml::Sequence(items) => {
            let mut converted = Vec::with_capacity(items.len());
            for item in items {
                converted.push(yaml_to_json(item)?);
            }
            Ok(Value::Array(converted))
        }
        Yaml::Mapping(mapping) => {
            let mut converted = serde_json::Map::new();
            for (key, item) in mapping {
                let key = mapping_key(key)?;
                converted.insert(key, yaml_to_json(item)?);
            }
            Ok(Value::Object(converted))
        }
        Yaml::Tagged(tagged) => Err(ConvertError::new(
            format!("不支持带标签的 YAML 值: {}", tagged.tag),
            "去掉 !tag 标记，或改用其它格式的源文件",
        )),
    }
}

/// YAML 值转 TOML 值：`null` 无法表示，整数受 64 位有符号范围限制。
fn yaml_to_toml(value: Yaml) -> Result<toml::Value, ConvertError> {
    match value {
        Yaml::Null => Err(ConvertError::new(
            "null 无法用 TOML 表示".to_string(),
            "删除该键，或改用 --to json",
        )),
        Yaml::Bool(flag) => Ok(toml::Value::Boolean(flag)),
        Yaml::Number(number) => number_to_toml(number),
        Yaml::String(text) => Ok(toml::Value::String(text)),
        Yaml::Sequence(items) => {
            let mut converted = Vec::with_capacity(items.len());
            for item in items {
                converted.push(yaml_to_toml(item)?);
            }
            Ok(toml::Value::Array(converted))
        }
        Yaml::Mapping(mapping) => {
            let mut converted = toml::map::Map::new();
            for (key, item) in mapping {
                let key = mapping_key(key)?;
                converted.insert(key, yaml_to_toml(item)?);
            }
            Ok(toml::Value::Table(converted))
        }
        Yaml::Tagged(tagged) => Err(ConvertError::new(
            format!("不支持带标签的 YAML 值: {}", tagged.tag),
            "去掉 !tag 标记，或改用其它格式的源文件",
        )),
    }
}

fn mapping_key(key: Yaml) -> Result<String, ConvertError> {
    match key {
        Yaml::String(text) => Ok(text),
        _ => Err(ConvertError::new(
            "YAML 键不是字符串".to_string(),
            "把键写成带引号的字符串，例如 \"1\"",
        )),
    }
}

fn number_to_json(number: serde_norway::Number) -> Result<Value, ConvertError> {
    if let Some(value) = number.as_i64() {
        return Ok(json!(value));
    }
    if let Some(value) = number.as_u64() {
        return Ok(json!(value));
    }
    let float = number.as_f64().expect("YAML 数值总能表示为 f64");
    serde_json::Number::from_f64(float)
        .map(Value::Number)
        .ok_or_else(|| {
            ConvertError::new(
                format!("浮点数 {float} 无法用 JSON 表示"),
                "删除 .inf/.nan 等非有限取值",
            )
        })
}

fn number_to_toml(number: serde_norway::Number) -> Result<toml::Value, ConvertError> {
    if let Some(value) = number.as_i64() {
        return Ok(toml::Value::Integer(value));
    }
    if let Some(value) = number.as_u64() {
        return i64::try_from(value).map(toml::Value::Integer).map_err(|_| {
            ConvertError::new(
                format!("整数 {value} 超出 TOML 的 64 位有符号范围"),
                "改用 --to json",
            )
        });
    }
    let float = number.as_f64().expect("YAML 数值总能表示为 f64");
    if !float.is_finite() {
        return Err(ConvertError::new(
            format!("浮点数 {float} 无法用 TOML 表示"),
            "删除 .inf/.nan 等非有限取值，或改用 --to json",
        ));
    }
    Ok(toml::Value::Float(float))
}

pub fn def() -> ToolDef {
    ToolDef {
        meta: &META,
        build,
        run: run_cli,
        run_json: Some(run_json),
        schema: Some(yaml_args_schema),
    }
}

fn yaml_args_schema() -> String {
    let schema = schemars::schema_for!(YamlArgs);
    serde_json::to_string_pretty(&schema).expect("YamlArgs schema 可序列化")
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
        let path = dir.path().join("sample.yaml");
        std::fs::write(&path, text).expect("应写入临时文件");
        (dir, path.to_string_lossy().into_owned())
    }

    #[test]
    fn converts_yaml_to_json() {
        let (_dir, path) = write_file(
            "title: demo\ncount: 2\nlist:\n  - a\n  - b\ndb:\n  port: 5432\nactive: true\n",
        );
        let out = run_cli(&mut ToolCtx::default(), &matches(&["yaml", &path])).unwrap();
        assert_eq!(out["path"], path);
        assert_eq!(out["to"], "json");
        let parsed: Value = serde_json::from_str(out["text"].as_str().unwrap()).unwrap();
        assert_eq!(parsed["title"], "demo");
        assert_eq!(parsed["list"], json!(["a", "b"]));
        assert_eq!(parsed["db"]["port"], 5432);
        assert_eq!(parsed["active"], json!(true));
    }

    #[test]
    fn converts_yaml_to_toml() {
        let (_dir, path) = write_file("title: demo\ndb:\n  port: 5432\n");
        let out = run_cli(
            &mut ToolCtx::default(),
            &matches(&["yaml", &path, "--to", "toml"]),
        )
        .unwrap();
        let text = out["text"].as_str().unwrap();
        assert!(text.contains("title = \"demo\""), "{text}");
        assert!(text.contains("[db]"), "{text}");
        assert!(text.contains("port = 5432"), "{text}");
    }

    #[test]
    fn check_only_validates() {
        let (_dir, path) = write_file("a: 1\n");
        let out = run_cli(
            &mut ToolCtx::default(),
            &matches(&["yaml", &path, "--check"]),
        )
        .unwrap();
        assert_eq!(out, json!({"path": path, "ok": true}));
    }

    #[test]
    fn check_conflicts_with_to() {
        let (_dir, path) = write_file("a: 1\n");
        let err = build()
            .try_get_matches_from(["yaml", &path, "--check", "--to", "json"])
            .unwrap_err();
        assert!(err.to_string().contains("--check"), "{err}");
    }

    #[test]
    fn duplicate_keys_are_parse_failed() {
        let (_dir, path) = write_file("a: 1\na: 2\n");
        let err = run_cli(&mut ToolCtx::default(), &matches(&["yaml", &path])).unwrap_err();
        assert_eq!(err.code, "PARSE_FAILED");
        assert_eq!(err.exit_code(), 3);
        assert_eq!(err.field.as_deref(), Some("path"));
        assert!(err.hint.is_some());
    }

    #[test]
    fn non_string_keys_are_parse_failed() {
        let (_dir, path) = write_file("1: a\n");
        let err = run_cli(&mut ToolCtx::default(), &matches(&["yaml", &path])).unwrap_err();
        assert_eq!(err.code, "PARSE_FAILED");
        assert_eq!(err.field.as_deref(), Some("path"));
        assert!(err.hint.unwrap().contains("字符串"));
    }

    #[test]
    fn null_to_toml_is_parse_failed() {
        let (_dir, path) = write_file("a: null\n");
        let err = run_cli(
            &mut ToolCtx::default(),
            &matches(&["yaml", &path, "--to", "toml"]),
        )
        .unwrap_err();
        assert_eq!(err.code, "PARSE_FAILED");
        assert!(err.message.contains("null"), "{}", err.message);
    }

    #[test]
    fn scalar_root_to_toml_is_parse_failed() {
        let (_dir, path) = write_file("- a\n- b\n");
        let err = run_cli(
            &mut ToolCtx::default(),
            &matches(&["yaml", &path, "--to", "toml"]),
        )
        .unwrap_err();
        assert_eq!(err.code, "PARSE_FAILED");
        assert!(err.hint.is_some());
    }

    #[test]
    fn infinite_float_is_parse_failed() {
        let (_dir, path) = write_file("value: .inf\n");
        let err = run_cli(&mut ToolCtx::default(), &matches(&["yaml", &path])).unwrap_err();
        assert_eq!(err.code, "PARSE_FAILED");

        let err = run_cli(
            &mut ToolCtx::default(),
            &matches(&["yaml", &path, "--to", "toml"]),
        )
        .unwrap_err();
        assert_eq!(err.code, "PARSE_FAILED");
    }

    #[test]
    fn tagged_value_is_parse_failed() {
        let (_dir, path) = write_file("a: !Tag 1\n");
        let err = run_cli(&mut ToolCtx::default(), &matches(&["yaml", &path])).unwrap_err();
        assert_eq!(err.code, "PARSE_FAILED");
    }

    #[test]
    fn unsigned_integer_out_of_toml_range_is_parse_failed() {
        let (_dir, path) = write_file("a: 18446744073709551615\n");
        let err = run_cli(
            &mut ToolCtx::default(),
            &matches(&["yaml", &path, "--to", "toml"]),
        )
        .unwrap_err();
        assert_eq!(err.code, "PARSE_FAILED");
    }

    #[test]
    fn bom_is_stripped() {
        let (_dir, path) = write_file("\u{feff}a: 1\n");
        let out = run_cli(&mut ToolCtx::default(), &matches(&["yaml", &path])).unwrap();
        let parsed: Value = serde_json::from_str(out["text"].as_str().unwrap()).unwrap();
        assert_eq!(parsed["a"], 1);
    }

    #[test]
    fn non_utf8_file_is_invalid_encoding() {
        let dir = temp_dir().unwrap();
        let path = dir.path().join("bad.yaml");
        std::fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
        let display = path.to_string_lossy().into_owned();
        let err = run_cli(&mut ToolCtx::default(), &matches(&["yaml", &display])).unwrap_err();
        assert_eq!(err.code, "INVALID_ENCODING");
        assert_eq!(err.field.as_deref(), Some("path"));
    }

    #[test]
    fn missing_file_is_io_error() {
        let (_dir, path) = write_file("a: 1\n");
        let missing = format!("{path}.missing");
        let err = run_cli(&mut ToolCtx::default(), &matches(&["yaml", &missing])).unwrap_err();
        assert_eq!(err.code, "IO");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(err.field.as_deref(), Some("path"));
    }

    #[test]
    fn cli_and_json_paths_agree() {
        let (_dir, path) = write_file("a: 1\n");
        let cli = run_cli(&mut ToolCtx::default(), &matches(&["yaml", &path])).unwrap();
        let json_out = run_json(&mut ToolCtx::default(), &json!({"file": path})).unwrap();
        assert_eq!(cli, json_out);
    }

    #[test]
    fn json_entry_rejects_unknown_format() {
        let (_dir, path) = write_file("a: 1\n");
        let err =
            run_json(&mut ToolCtx::default(), &json!({"file": path, "to": "xml"})).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("to"));
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
        assert_eq!(tool.meta.name, "yaml");
        assert_eq!(tool.meta.aliases, &["yamtool"]);
        assert_eq!(tool.meta.side_effects, SideEffects::Read);
        assert!(tool.run_json.is_some());
        let schema = (tool.schema.expect("应提供 schema"))();
        for field in ["\"file\"", "\"to\"", "\"check\""] {
            assert!(schema.contains(field), "{field} 不在 schema: {schema}");
        }
    }
}
