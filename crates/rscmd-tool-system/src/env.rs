//! `env`：只读查询环境变量。
//!
//! 不提供 setenv/删除等写入能力。默认列出全部变量，`NAME` 做精确过滤，`--prefix`
//! 做前缀过滤。输出按变量名排序（同名再按值排序），保证跨平台结果稳定。
//!
//! Windows 的环境变量名大小写不敏感（`GetEnvironmentVariableW` 语义），因此精确匹配
//! 与前缀匹配在 Windows 上按 ASCII 大小写不敏感比较，Unix 上保持大小写敏感；输出里的
//! `case_insensitive` 字段显式说明当前平台口径。
//!
//! 非 UTF-8 的名称或值不会导致失败：用 U+FFFD 替换后输出，并在 `non_utf8` 中列出
//! 对应变量名，调用方可据此判断值是否完整。

use std::env;
use std::ffi::OsString;

use clap::{ArgMatches, Command};
use rscmd_core::{
    command_for, PythonExposure, RscmdError, SideEffects, ToolCtx, ToolDef, ToolMeta,
};
use serde_json::{json, Value};

pub static META: ToolMeta = ToolMeta {
    name: "env",
    aliases: &["envtool"],
    summary: "只读查询环境变量，支持按名字与前缀过滤",
    side_effects: SideEffects::Read,
    gui: false,
    python: PythonExposure::Structured,
    examples: &[
        "rscmd env",
        "rscmd env PATH",
        "rscmd env --prefix RSCMD_ --json",
    ],
};

#[derive(Debug, clap::Args, serde::Deserialize, schemars::JsonSchema)]
pub struct EnvArgs {
    /// 变量名（Windows 下不区分大小写）；不给出则列出全部
    #[arg(value_name = "NAME")]
    #[serde(default)]
    pub name: Option<String>,
    /// 变量名前缀过滤（Windows 下不区分大小写）
    #[arg(long, value_name = "PREFIX")]
    #[serde(default)]
    pub prefix: Option<String>,
}

pub fn build() -> Command {
    command_for::<EnvArgs>(&META)
}

pub fn run_cli(ctx: &mut ToolCtx, matches: &ArgMatches) -> Result<Value, RscmdError> {
    let args = <EnvArgs as clap::FromArgMatches>::from_arg_matches(matches)
        .map_err(RscmdError::from_clap)?;
    run(ctx, args)
}

pub fn run_json(ctx: &mut ToolCtx, input: &Value) -> Result<Value, RscmdError> {
    let args: EnvArgs = serde_json::from_value(input.clone())
        .map_err(|err| RscmdError::coded("INVALID_ARGS", err.to_string()).with_field("args"))?;
    run(ctx, args)
}

fn run(_ctx: &mut ToolCtx, args: EnvArgs) -> Result<Value, RscmdError> {
    render(
        env::vars_os(),
        args.name.as_deref(),
        args.prefix.as_deref(),
        cfg!(windows),
    )
}

fn render<I>(
    entries: I,
    name: Option<&str>,
    prefix: Option<&str>,
    case_insensitive: bool,
) -> Result<Value, RscmdError>
where
    I: IntoIterator<Item = (OsString, OsString)>,
{
    if let Some(name) = name {
        if name.is_empty() {
            return Err(RscmdError::coded("INVALID_ARGS", "NAME 不能为空")
                .with_field("name")
                .with_expected("非空变量名"));
        }
    }
    if let Some(prefix) = prefix {
        if prefix.is_empty() {
            return Err(RscmdError::coded("INVALID_ARGS", "PREFIX 不能为空")
                .with_field("prefix")
                .with_expected("非空前缀"));
        }
    }

    let mut matched: Vec<(String, String)> = Vec::new();
    let mut non_utf8: Vec<String> = Vec::new();
    for (raw_name, raw_value) in entries {
        let display_name = raw_name.to_string_lossy().into_owned();
        if let Some(wanted) = name {
            if !names_match(&display_name, wanted, case_insensitive) {
                continue;
            }
        }
        if let Some(wanted) = prefix {
            if !prefix_matches(&display_name, wanted, case_insensitive) {
                continue;
            }
        }
        if raw_name.to_str().is_none() || raw_value.to_str().is_none() {
            non_utf8.push(display_name.clone());
        }
        matched.push((display_name, raw_value.to_string_lossy().into_owned()));
    }

    matched.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    non_utf8.sort();
    non_utf8.dedup();

    let vars: Vec<Value> = matched
        .into_iter()
        .map(|(name, value)| json!({"name": name, "value": value}))
        .collect();
    Ok(json!({
        "count": vars.len(),
        "case_insensitive": case_insensitive,
        "vars": vars,
        "non_utf8": non_utf8,
    }))
}

fn normalize(text: &str, case_insensitive: bool) -> String {
    if case_insensitive {
        text.to_ascii_lowercase()
    } else {
        text.to_string()
    }
}

fn names_match(name: &str, wanted: &str, case_insensitive: bool) -> bool {
    normalize(name, case_insensitive) == normalize(wanted, case_insensitive)
}

fn prefix_matches(name: &str, prefix: &str, case_insensitive: bool) -> bool {
    normalize(name, case_insensitive).starts_with(&normalize(prefix, case_insensitive))
}

pub fn def() -> ToolDef {
    ToolDef {
        meta: &META,
        build,
        run: run_cli,
        run_json: Some(run_json),
        schema: Some(env_args_schema),
    }
}

fn env_args_schema() -> String {
    let schema = schemars::schema_for!(EnvArgs);
    serde_json::to_string_pretty(&schema).expect("EnvArgs schema 可序列化")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(args: &[&str]) -> ArgMatches {
        build().try_get_matches_from(args).expect("参数应可解析")
    }

    fn entries() -> Vec<(OsString, OsString)> {
        vec![
            (OsString::from("Path"), OsString::from("/bin")),
            (OsString::from("PATHEXT"), OsString::from(".EXE")),
            (OsString::from("HOME"), OsString::from("/home/u")),
        ]
    }

    fn names(out: &Value) -> Vec<&str> {
        out["vars"]
            .as_array()
            .expect("应有 vars")
            .iter()
            .map(|item| item["name"].as_str().expect("应有 name"))
            .collect()
    }

    #[test]
    fn lists_all_vars_sorted() {
        let out = render(entries(), None, None, false).unwrap();
        assert_eq!(out["count"], 3);
        assert_eq!(out["case_insensitive"], false);
        assert_eq!(names(&out), vec!["HOME", "PATHEXT", "Path"]);
    }

    #[test]
    fn exact_name_filter_is_case_sensitive_on_unix_semantics() {
        let out = render(entries(), Some("path"), None, false).unwrap();
        assert_eq!(out["count"], 0);

        let out = render(entries(), Some("Path"), None, false).unwrap();
        assert_eq!(out["count"], 1);
        assert_eq!(out["vars"][0]["value"], "/bin");
    }

    #[test]
    fn exact_name_filter_is_case_insensitive_on_windows_semantics() {
        let out = render(entries(), Some("path"), None, true).unwrap();
        assert_eq!(out["count"], 1);
        assert_eq!(out["vars"][0]["value"], "/bin");
        let out = render(entries(), Some("PATH"), None, true).unwrap();
        assert_eq!(out["count"], 1);
        assert_eq!(out["vars"][0]["name"], "Path");
    }

    #[test]
    fn prefix_filter_follows_platform_case_rules() {
        let out = render(entries(), None, Some("path"), false).unwrap();
        assert_eq!(out["count"], 0);

        let out = render(entries(), None, Some("path"), true).unwrap();
        assert_eq!(out["count"], 2);
        assert_eq!(names(&out), vec!["PATHEXT", "Path"]);

        let out = render(entries(), None, Some("PATH"), true).unwrap();
        assert_eq!(out["count"], 2);
    }

    #[test]
    fn name_and_prefix_are_combined() {
        let out = render(entries(), Some("Path"), Some("PATHEXT"), true).unwrap();
        assert_eq!(out["count"], 0);

        let out = render(entries(), Some("Path"), Some("path"), true).unwrap();
        assert_eq!(out["count"], 1);
    }

    #[test]
    fn duplicates_are_sorted_by_value() {
        let input = vec![
            (OsString::from("A"), OsString::from("2")),
            (OsString::from("A"), OsString::from("1")),
        ];
        let out = render(input, None, None, false).unwrap();
        assert_eq!(out["count"], 2);
        assert_eq!(out["vars"][0]["value"], "1");
        assert_eq!(out["vars"][1]["value"], "2");
    }

    #[cfg(unix)]
    fn non_utf8_entry() -> (OsString, OsString) {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        (
            OsStr::from_bytes(b"BAD_\xff").to_os_string(),
            OsStr::from_bytes(b"v\xff").to_os_string(),
        )
    }

    #[cfg(windows)]
    fn non_utf8_entry() -> (OsString, OsString) {
        use std::os::windows::ffi::OsStringExt;

        // 孤立代理项 0xD800 不是合法 UTF-16 标量序列。
        (
            OsString::from_wide(&[0xD800]),
            OsString::from_wide(&[0xD800]),
        )
    }

    #[test]
    fn non_utf8_entries_are_marked_not_rejected() {
        let mut input = entries();
        input.push(non_utf8_entry());
        let out = render(input, None, None, false).unwrap();
        assert_eq!(out["count"], 4);
        let flagged = out["non_utf8"].as_array().expect("应有 non_utf8");
        assert_eq!(flagged.len(), 1);
        let sorted = names(&out);
        assert!(sorted.contains(&flagged[0].as_str().unwrap()));
    }

    #[test]
    fn empty_filters_are_invalid_args() {
        let err = render(entries(), Some(""), None, false).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("name"));

        let err = render(entries(), None, Some(""), false).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("prefix"));
    }

    #[test]
    fn process_env_is_readable() {
        let out = run_json(&mut ToolCtx::default(), &json!({})).unwrap();
        let listed = out["vars"].as_array().expect("应有 vars").len();
        assert_eq!(out["count"], listed);
        assert!(listed > 0);
        assert_eq!(out["case_insensitive"], cfg!(windows));
    }

    #[test]
    fn path_lookup_works_on_both_platforms() {
        let out = run_json(&mut ToolCtx::default(), &json!({"name": "PATH"})).unwrap();
        assert!(out["count"].as_u64().unwrap() >= 1);
    }

    #[test]
    fn cli_and_json_paths_agree() {
        let cli = run_cli(
            &mut ToolCtx::default(),
            &matches(&["env", "--prefix", "RSCMD_NO_SUCH_PREFIX_"]),
        )
        .unwrap();
        let json_out = run_json(
            &mut ToolCtx::default(),
            &json!({"prefix": "RSCMD_NO_SUCH_PREFIX_"}),
        )
        .unwrap();
        assert_eq!(cli, json_out);
        assert_eq!(cli["count"], 0);
    }

    #[test]
    fn json_entry_rejects_wrong_types() {
        let err = run_json(&mut ToolCtx::default(), &json!({"name": 1})).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("args"));
    }

    #[test]
    fn def_exposes_schema_and_metadata() {
        let tool = def();
        assert_eq!(tool.meta.name, "env");
        assert_eq!(tool.meta.aliases, &["envtool"]);
        assert_eq!(tool.meta.side_effects, SideEffects::Read);
        assert!(tool.run_json.is_some());
        let schema = (tool.schema.expect("应提供 schema"))();
        for field in ["\"name\"", "\"prefix\""] {
            assert!(schema.contains(field), "{field} 不在 schema: {schema}");
        }
    }
}
