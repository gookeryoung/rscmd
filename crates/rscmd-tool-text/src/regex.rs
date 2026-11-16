//! `regex`：正则匹配、分组捕获与替换。
//!
//! 使用 Rust `regex` crate（线性时间、无回溯），并用 `size_limit` 约束自动机大小，
//! 避免复杂表达式占用过多内存。`start`/`end` 是 UTF-8 字节偏移，跨平台一致。

use std::path::PathBuf;

use clap::{ArgMatches, Command};
use regex::RegexBuilder;
use rscmd_core::{
    command_for, PythonExposure, RscmdError, SideEffects, ToolCtx, ToolDef, ToolMeta,
};
use serde_json::{json, Value};

pub static META: ToolMeta = ToolMeta {
    name: "regex",
    aliases: &["regextool"],
    summary: "正则匹配、分组捕获与替换（Rust regex 语法）",
    side_effects: SideEffects::Read,
    gui: false,
    python: PythonExposure::Structured,
    examples: &[
        "rscmd regex '\\d+' 'a1b22'",
        "rscmd regex '(\\w+)@(\\w+)' --file list.txt --replace '${2}:${1}'",
    ],
};

#[derive(Debug, clap::Args, serde::Deserialize, schemars::JsonSchema)]
pub struct RegexArgs {
    /// 正则表达式（Rust regex 语法）
    #[arg(value_name = "PATTERN")]
    pub pattern: String,
    /// 待匹配文本（与 --file 二选一）
    #[arg(value_name = "TEXT")]
    #[serde(default)]
    pub text: Option<String>,
    /// 从文件读取待匹配文本（与 TEXT 二选一）
    #[arg(long, value_name = "PATH")]
    #[serde(default)]
    pub file: Option<PathBuf>,
    /// 替换模板（支持 $1 / ${name}）；提供时额外输出替换后的文本
    #[arg(long, value_name = "REPLACEMENT")]
    #[serde(default)]
    pub replace: Option<String>,
    /// 自动机大小上限（字节），0 表示不限制
    #[arg(long, value_name = "BYTES", default_value_t = 10 * 1024 * 1024)]
    #[serde(default = "default_size_limit")]
    pub size_limit: usize,
}

fn default_size_limit() -> usize {
    10 * 1024 * 1024
}

pub fn build() -> Command {
    command_for::<RegexArgs>(&META)
}

pub fn run_cli(ctx: &mut ToolCtx, matches: &ArgMatches) -> Result<Value, RscmdError> {
    let args = <RegexArgs as clap::FromArgMatches>::from_arg_matches(matches)
        .map_err(RscmdError::from_clap)?;
    run(ctx, args)
}

pub fn run_json(ctx: &mut ToolCtx, input: &Value) -> Result<Value, RscmdError> {
    let args: RegexArgs = serde_json::from_value(input.clone())
        .map_err(|err| RscmdError::coded("INVALID_ARGS", err.to_string()).with_field("args"))?;
    run(ctx, args)
}

fn run(_ctx: &mut ToolCtx, args: RegexArgs) -> Result<Value, RscmdError> {
    let input = read_input(&args)?;
    let regex = RegexBuilder::new(&args.pattern)
        .size_limit(args.size_limit)
        .build()
        .map_err(|err| {
            RscmdError::coded("INVALID_REGEX", err.to_string())
                .with_field("pattern")
                .with_expected("Rust regex 语法")
        })?;

    let matches: Vec<Value> = regex
        .captures_iter(&input)
        .map(|caps| {
            let whole = caps.get(0).expect("第 0 组总是存在");
            let groups: Vec<Value> = caps
                .iter()
                .skip(1)
                .map(|group| match group {
                    Some(found) => Value::String(found.as_str().to_string()),
                    None => Value::Null,
                })
                .collect();
            json!({
                "text": whole.as_str(),
                "start": whole.start(),
                "end": whole.end(),
                "groups": groups,
            })
        })
        .collect();

    let mut out = json!({
        "pattern": args.pattern,
        "count": matches.len(),
        "matches": matches,
    });
    if let Some(replacement) = &args.replace {
        out["text"] = Value::String(regex.replace_all(&input, replacement.as_str()).into_owned());
    }
    Ok(out)
}

fn read_input(args: &RegexArgs) -> Result<String, RscmdError> {
    match (&args.text, &args.file) {
        (Some(text), None) => Ok(text.clone()),
        (None, Some(path)) => std::fs::read_to_string(path).map_err(|err| {
            RscmdError::coded("IO", format!("读取文件失败: {err}")).with_field("file")
        }),
        (Some(_), Some(_)) => Err(
            RscmdError::coded("INVALID_ARGS", "TEXT 与 --file 只能二选一")
                .with_field("file")
                .with_expected("只给 TEXT 或只给 --file"),
        ),
        (None, None) => Err(RscmdError::coded("INVALID_ARGS", "需要提供 TEXT 或 --file")
            .with_field("file")
            .with_expected("TEXT 或 --file")),
    }
}

pub fn def() -> ToolDef {
    ToolDef {
        meta: &META,
        build,
        run: run_cli,
        run_json: Some(run_json),
        schema: Some(regex_args_schema),
    }
}

fn regex_args_schema() -> String {
    let schema = schemars::schema_for!(RegexArgs);
    serde_json::to_string_pretty(&schema).expect("RegexArgs schema 可序列化")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(args: &[&str]) -> ArgMatches {
        build().try_get_matches_from(args).expect("参数应可解析")
    }

    #[test]
    fn finds_matches_with_groups_and_offsets() {
        let out = run_cli(
            &mut ToolCtx::default(),
            &matches(&["regex", "(\\d)(\\d)", "a12b34"]),
        )
        .unwrap();
        assert_eq!(out["count"], 2);
        assert_eq!(out["matches"][0]["text"], "12");
        assert_eq!(out["matches"][0]["start"], 1);
        assert_eq!(out["matches"][0]["end"], 3);
        assert_eq!(out["matches"][0]["groups"], json!(["1", "2"]));
        assert_eq!(out["matches"][1]["start"], 4);
    }

    #[test]
    fn reports_empty_result() {
        let out = run_cli(&mut ToolCtx::default(), &matches(&["regex", "z+", "abc"])).unwrap();
        assert_eq!(out["count"], 0);
        assert_eq!(out["matches"], json!([]));
        assert!(out.get("text").is_none());
    }

    #[test]
    fn offsets_are_utf8_bytes_for_cjk() {
        let out = run_cli(
            &mut ToolCtx::default(),
            &matches(&["regex", "b+", "你好bbb"]),
        )
        .unwrap();
        assert_eq!(out["matches"][0]["text"], "bbb");
        // 两个汉字各占 3 字节。
        assert_eq!(out["matches"][0]["start"], 6);
        assert_eq!(out["matches"][0]["end"], 9);
    }

    #[test]
    fn crlf_and_lf_are_matched_consistently() {
        for input in ["a\nb", "a\r\nb"] {
            let out = run_cli(&mut ToolCtx::default(), &matches(&["regex", "b", input])).unwrap();
            let start = out["matches"][0]["start"].as_u64().unwrap();
            assert_eq!(start, input.len() as u64 - 1, "input={input:?}");
        }
    }

    #[test]
    fn replacement_uses_groups() {
        let out = run_cli(
            &mut ToolCtx::default(),
            &matches(&["regex", "(\\w+)@(\\w+)", "a@b c@d", "--replace", "$2:$1"]),
        )
        .unwrap();
        assert_eq!(out["count"], 2);
        assert_eq!(out["text"], "b:a d:c");
    }

    #[test]
    fn replacement_supports_named_groups() {
        let out = run_cli(
            &mut ToolCtx::default(),
            &matches(&[
                "regex",
                "(?P<k>\\w+)=(?P<v>\\d+)",
                "a=1",
                "--replace",
                "${k}:${v}",
            ]),
        )
        .unwrap();
        assert_eq!(out["text"], "a:1");
        assert_eq!(out["matches"][0]["groups"], json!(["a", "1"]));
    }

    #[test]
    fn reads_file_input() {
        let path = std::env::temp_dir().join(format!("rscmd-regex-{}.txt", std::process::id()));
        std::fs::write(&path, "x1x2").unwrap();
        let out = run_cli(
            &mut ToolCtx::default(),
            &matches(&["regex", "\\d", "--file", path.to_str().unwrap()]),
        )
        .unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(out["count"], 2);
    }

    #[test]
    fn rejects_text_and_file_together() {
        let err = run_cli(
            &mut ToolCtx::default(),
            &matches(&["regex", "a", "abc", "--file", "nope.txt"]),
        )
        .unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("file"));
    }

    #[test]
    fn rejects_missing_input() {
        let err = run_cli(&mut ToolCtx::default(), &matches(&["regex", "a"])).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn invalid_pattern_is_invalid_regex() {
        let err =
            run_cli(&mut ToolCtx::default(), &matches(&["regex", "(?P<", "abc"])).unwrap_err();
        assert_eq!(err.code, "INVALID_REGEX");
        assert_eq!(err.exit_code(), 3);
        assert_eq!(err.field.as_deref(), Some("pattern"));
    }

    #[test]
    fn size_limit_rejects_oversized_program() {
        let err = run_cli(
            &mut ToolCtx::default(),
            &matches(&["regex", "a{1000}", "aaa", "--size-limit", "64"]),
        )
        .unwrap_err();
        assert_eq!(err.code, "INVALID_REGEX");
    }

    #[test]
    fn json_entry_matches_with_default_size_limit() {
        let out = run_json(
            &mut ToolCtx::default(),
            &json!({"pattern": "[a-z]+", "text": "AB cd"}),
        )
        .unwrap();
        assert_eq!(out["count"], 1);
        assert_eq!(out["matches"][0]["text"], "cd");
    }

    #[test]
    fn json_entry_rejects_bad_payload() {
        let err = run_json(&mut ToolCtx::default(), &json!({"text": "abc"})).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("args"));
    }

    #[test]
    fn def_exposes_structured_json_entry() {
        let tool = def();
        assert_eq!(tool.meta.name, "regex");
        assert_eq!(tool.meta.aliases, &["regextool"]);
        let schema = (tool.schema.expect("应提供 schema"))();
        assert!(schema.contains("\"pattern\""), "{schema}");
        assert!(schema.contains("\"size_limit\""), "{schema}");
    }
}
