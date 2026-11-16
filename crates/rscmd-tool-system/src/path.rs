//! `path`：路径规范化、拼接、相对路径与绝对路径判定。
//!
//! 全部按纯字符串语义处理，不做文件系统访问；输出统一使用 `/` 分隔，
//! 输入可混用 `\` 与 `/`，因此 Windows 与 Unix 上的结果一致。

use std::path::Path;

use clap::{Arg, ArgMatches, Command};
use rscmd_core::{PythonExposure, RscmdError, SideEffects, ToolCtx, ToolDef, ToolMeta};
use rscmd_runtime::fs::{is_absolute_path, normalize_path};
use serde_json::{json, Value};

pub static META: ToolMeta = ToolMeta {
    name: "path",
    aliases: &["pathtool"],
    summary: "路径规范化、拼接、相对路径与绝对路径判定",
    side_effects: SideEffects::None,
    gui: false,
    python: PythonExposure::Structured,
    examples: &[
        "rscmd path normalize a/../b",
        "rscmd path join src lib/../bin",
        "rscmd path relative a/b/c --base a",
        "rscmd path is-abs C:/x",
    ],
};

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct PathInput {
    /// 操作：normalize/join/relative/is-abs
    pub op: String,
    /// 待处理的路径（normalize/relative/is-abs）
    #[serde(default)]
    pub path: Option<String>,
    /// relative 的基准路径
    #[serde(default)]
    pub base: Option<String>,
    /// join 的路径片段
    #[serde(default)]
    pub parts: Vec<String>,
}

pub fn build() -> Command {
    Command::new(META.name)
        .about(META.summary)
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("normalize")
                .about("规范化路径（统一分隔符、消除 . 与 ..）")
                .arg(Arg::new("path").value_name("PATH").required(true)),
        )
        .subcommand(
            Command::new("join").about("拼接多个路径片段并规范化").arg(
                Arg::new("parts")
                    .value_name("PART")
                    .num_args(1..)
                    .required(true),
            ),
        )
        .subcommand(
            Command::new("relative")
                .about("计算相对 base 的路径")
                .arg(Arg::new("path").value_name("PATH").required(true))
                .arg(
                    Arg::new("base")
                        .long("base")
                        .value_name("BASE")
                        .required(true),
                ),
        )
        .subcommand(
            Command::new("is-abs")
                .about("判断是否为绝对路径")
                .arg(Arg::new("path").value_name("PATH").required(true)),
        )
}

pub fn run_cli(ctx: &mut ToolCtx, matches: &ArgMatches) -> Result<Value, RscmdError> {
    let input = match matches.subcommand() {
        Some(("normalize", sub)) => PathInput {
            op: "normalize".to_string(),
            path: sub.get_one::<String>("path").cloned(),
            ..PathInput::default()
        },
        Some(("join", sub)) => PathInput {
            op: "join".to_string(),
            parts: sub
                .get_many::<String>("parts")
                .map(|values| values.cloned().collect())
                .unwrap_or_default(),
            ..PathInput::default()
        },
        Some(("relative", sub)) => PathInput {
            op: "relative".to_string(),
            path: sub.get_one::<String>("path").cloned(),
            base: sub.get_one::<String>("base").cloned(),
            ..PathInput::default()
        },
        Some(("is-abs", sub)) => PathInput {
            op: "is-abs".to_string(),
            path: sub.get_one::<String>("path").cloned(),
            ..PathInput::default()
        },
        _ => {
            return Err(RscmdError::coded(
                "INVALID_ARGS",
                "需要子命令: normalize/join/relative/is-abs",
            )
            .with_field("op")
            .with_expected("normalize/join/relative/is-abs"));
        }
    };
    run(ctx, input)
}

pub fn run_json(ctx: &mut ToolCtx, input: &Value) -> Result<Value, RscmdError> {
    let input: PathInput = serde_json::from_value(input.clone())
        .map_err(|err| RscmdError::coded("INVALID_ARGS", err.to_string()).with_field("args"))?;
    run(ctx, input)
}

fn run(_ctx: &mut ToolCtx, input: PathInput) -> Result<Value, RscmdError> {
    match input.op.as_str() {
        "normalize" => {
            let path = require(&input.path, "path", "normalize 需要 path")?;
            Ok(json!({
                "op": "normalize",
                "input": path,
                "path": as_slash(&path),
            }))
        }
        "join" => {
            if input.parts.is_empty() {
                return Err(
                    RscmdError::coded("INVALID_ARGS", "join 至少需要一个片段").with_field("parts")
                );
            }
            let joined = input.parts.join("/");
            Ok(json!({
                "op": "join",
                "parts": input.parts,
                "path": as_slash(&joined),
            }))
        }
        "relative" => {
            let path = require(&input.path, "path", "relative 需要 path")?;
            let base = require(&input.base, "base", "relative 需要 base")?;
            let relative = relative_path(&path, &base)?;
            Ok(json!({
                "op": "relative",
                "path": as_slash(&path),
                "base": as_slash(&base),
                "relative": relative,
            }))
        }
        "is-abs" => {
            let path = require(&input.path, "path", "is-abs 需要 path")?;
            Ok(json!({
                "op": "is-abs",
                "path": path,
                "absolute": is_absolute_path(&path),
            }))
        }
        other => Err(
            RscmdError::coded("INVALID_MODE", format!("未知路径操作: {other}"))
                .with_field("op")
                .with_expected("normalize/join/relative/is-abs"),
        ),
    }
}

fn require(value: &Option<String>, field: &str, message: &str) -> Result<String, RscmdError> {
    value
        .clone()
        .ok_or_else(|| RscmdError::coded("INVALID_ARGS", message).with_field(field))
}

/// 规范化并输出正斜杠字符串。
fn as_slash(raw: &str) -> String {
    normalize_path(Path::new(raw))
        .to_string_lossy()
        .into_owned()
}

fn relative_path(path: &str, base: &str) -> Result<String, RscmdError> {
    let path_norm = normalize_path(Path::new(path));
    let base_norm = normalize_path(Path::new(base));
    let path_abs = is_absolute_path(&path_norm.to_string_lossy());
    let base_abs = is_absolute_path(&base_norm.to_string_lossy());
    if path_abs != base_abs {
        return Err(
            RscmdError::coded("INVALID_ARGS", "path 与 base 必须同为绝对或同为相对路径")
                .with_field("base"),
        );
    }

    let path_parts = components(&path_norm);
    let base_parts = components(&base_norm);
    if path_abs {
        let path_root = root_of(&path_parts);
        let base_root = root_of(&base_parts);
        if !same_component(&path_root, &base_root) {
            return Err(RscmdError::coded(
                "INVALID_ARGS",
                format!("根目录不同，无法计算相对路径: {path_root} vs {base_root}"),
            )
            .with_field("base"));
        }
    }

    let common = path_parts
        .iter()
        .zip(base_parts.iter())
        .take_while(|(left, right)| same_component(left, right))
        .count();
    let mut parts = vec!["..".to_string(); base_parts.len() - common];
    parts.extend(path_parts[common..].iter().cloned());
    if parts.is_empty() {
        Ok(".".to_string())
    } else {
        Ok(parts.join("/"))
    }
}

fn components(path: &Path) -> Vec<String> {
    path.to_string_lossy()
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(String::from)
        .collect()
}

fn root_of(parts: &[String]) -> String {
    match parts.first() {
        Some(first) if first.ends_with(':') => first.clone(),
        _ => "/".to_string(),
    }
}

/// Windows 路径大小写不敏感，其余平台按字节比较。
fn same_component(left: &str, right: &str) -> bool {
    if cfg!(windows) {
        left.eq_ignore_ascii_case(right)
    } else {
        left == right
    }
}

pub fn def() -> ToolDef {
    ToolDef {
        meta: &META,
        build,
        run: run_cli,
        run_json: Some(run_json),
        schema: Some(path_args_schema),
    }
}

fn path_args_schema() -> String {
    let schema = schemars::schema_for!(PathInput);
    serde_json::to_string_pretty(&schema).expect("PathInput schema 可序列化")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(args: &[&str]) -> ArgMatches {
        build().try_get_matches_from(args).expect("参数应可解析")
    }

    #[test]
    fn normalize_unifies_separators_and_dots() {
        let out = run_cli(
            &mut ToolCtx::default(),
            &matches(&["path", "normalize", "a\\b\\..\\c"]),
        )
        .unwrap();
        assert_eq!(out["op"], "normalize");
        assert_eq!(out["path"], "a/c");
        assert_eq!(out["input"], "a\\b\\..\\c");
    }

    #[test]
    fn normalize_keeps_absolute_root() {
        let out = run_cli(
            &mut ToolCtx::default(),
            &matches(&["path", "normalize", "/a/../b"]),
        )
        .unwrap();
        assert_eq!(out["path"], "/b");
    }

    #[test]
    fn join_concatenates_and_normalizes() {
        let out = run_cli(
            &mut ToolCtx::default(),
            &matches(&["path", "join", "src", "lib/../bin"]),
        )
        .unwrap();
        assert_eq!(out["path"], "src/bin");
        assert_eq!(out["parts"], json!(["src", "lib/../bin"]));
    }

    #[test]
    fn relative_within_tree() {
        let out = run_cli(
            &mut ToolCtx::default(),
            &matches(&["path", "relative", "a/b/c", "--base", "a"]),
        )
        .unwrap();
        assert_eq!(out["relative"], "b/c");
    }

    #[test]
    fn relative_across_siblings() {
        let out = run_cli(
            &mut ToolCtx::default(),
            &matches(&["path", "relative", "a/b", "--base", "a/c/d"]),
        )
        .unwrap();
        assert_eq!(out["relative"], "../../b");
    }

    #[test]
    fn relative_same_path_is_dot() {
        let out = run_cli(
            &mut ToolCtx::default(),
            &matches(&["path", "relative", "a/b", "--base", "a/b/"]),
        )
        .unwrap();
        assert_eq!(out["relative"], ".");
    }

    #[test]
    fn relative_absolute_paths() {
        let out = run_cli(
            &mut ToolCtx::default(),
            &matches(&["path", "relative", "C:/work/src", "--base", "C:/work"]),
        )
        .unwrap();
        assert_eq!(out["relative"], "src");
        assert_eq!(out["path"], "C:/work/src");
    }

    #[test]
    fn relative_rejects_mixed_absolute_and_relative() {
        let err = run_cli(
            &mut ToolCtx::default(),
            &matches(&["path", "relative", "/a", "--base", "a"]),
        )
        .unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("base"));
    }

    #[test]
    fn relative_rejects_different_roots() {
        let err = run_cli(
            &mut ToolCtx::default(),
            &matches(&["path", "relative", "D:/x", "--base", "C:/y"]),
        )
        .unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn is_abs_is_platform_independent() {
        for (path, expected) in [
            ("/a", true),
            ("\\a", true),
            ("C:/a", true),
            ("c:\\a", true),
            ("a/b", false),
            ("", false),
        ] {
            let out =
                run_cli(&mut ToolCtx::default(), &matches(&["path", "is-abs", path])).unwrap();
            assert_eq!(out["absolute"], expected, "path={path:?}");
        }
    }

    #[test]
    fn json_entry_supports_every_op() {
        let mut ctx = ToolCtx::default();
        let out = run_json(&mut ctx, &json!({"op": "normalize", "path": "a/./b/../c"})).unwrap();
        assert_eq!(out["path"], "a/c");

        let out = run_json(&mut ctx, &json!({"op": "join", "parts": ["a", "b/../c"]})).unwrap();
        assert_eq!(out["path"], "a/c");

        let out = run_json(
            &mut ctx,
            &json!({"op": "relative", "path": "/x/y", "base": "/x"}),
        )
        .unwrap();
        assert_eq!(out["relative"], "y");

        let out = run_json(&mut ctx, &json!({"op": "is-abs", "path": "C:/x"})).unwrap();
        assert_eq!(out["absolute"], true);
    }

    #[test]
    fn json_entry_rejects_unknown_op() {
        let err = run_json(
            &mut ToolCtx::default(),
            &json!({"op": "resolve", "path": "a"}),
        )
        .unwrap_err();
        assert_eq!(err.code, "INVALID_MODE");
        assert_eq!(err.exit_code(), 3);
        assert_eq!(err.field.as_deref(), Some("op"));
    }

    #[test]
    fn json_entry_rejects_missing_fields() {
        let err = run_json(&mut ToolCtx::default(), &json!({"op": "normalize"})).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("path"));

        let err = run_json(
            &mut ToolCtx::default(),
            &json!({"op": "relative", "path": "a"}),
        )
        .unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("base"));

        let err = run_json(&mut ToolCtx::default(), &json!({"op": "join"})).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("parts"));

        let err = run_json(&mut ToolCtx::default(), &json!({})).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
    }

    #[test]
    fn output_always_uses_forward_slashes() {
        let out = run_cli(
            &mut ToolCtx::default(),
            &matches(&["path", "join", "a\\b", "c"]),
        )
        .unwrap();
        let path = out["path"].as_str().unwrap();
        assert!(!path.contains('\\'), "{path}");
    }

    #[cfg(windows)]
    #[test]
    fn relative_is_case_insensitive_on_windows() {
        let out = run_cli(
            &mut ToolCtx::default(),
            &matches(&["path", "relative", "C:/Work/SRC", "--base", "c:/work"]),
        )
        .unwrap();
        assert_eq!(out["relative"], "SRC");
    }

    #[test]
    fn def_exposes_structured_json_entry() {
        let tool = def();
        assert_eq!(tool.meta.name, "path");
        assert_eq!(tool.meta.aliases, &["pathtool"]);
        let schema = (tool.schema.expect("应提供 schema"))();
        assert!(schema.contains("\"op\""), "{schema}");
        assert!(schema.contains("\"parts\""), "{schema}");
    }
}
