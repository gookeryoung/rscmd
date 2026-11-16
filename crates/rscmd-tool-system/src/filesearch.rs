//! `filesearch`：按文件名 glob 与内容子串检索文件。
//!
//! 遍历一次目录树，先按 `--name` 过滤文件名，再按 `--content` 逐行做子串匹配。
//! glob 由 [`rscmd_runtime::fs::glob_match`] 手写实现（`*` / `?` / `**`），不引入额外依赖；
//! 结果路径统一用 `/` 分隔且相对 `ROOT`，`line` 从 1 开始，行尾 `\r` 会被去掉。

use std::path::{Path, PathBuf};

use clap::{ArgMatches, Command};
use rscmd_core::{
    command_for, PythonExposure, RscmdError, SideEffects, ToolCtx, ToolDef, ToolMeta,
};
use rscmd_runtime::fs::{glob_match, is_text_file, read_text, walk_files};
use serde_json::{json, Value};

pub static META: ToolMeta = ToolMeta {
    name: "filesearch",
    aliases: &["filesearchtool"],
    summary: "按文件名 glob 与内容子串检索文件，支持 --limit 截断",
    side_effects: SideEffects::Read,
    gui: false,
    python: PythonExposure::Structured,
    examples: &[
        "rscmd filesearch . --name '*.rs' --limit 20",
        "rscmd filesearch src --content 'TODO' --limit 5 --json",
    ],
};

#[derive(Debug, clap::Args, serde::Deserialize, schemars::JsonSchema)]
pub struct FilesearchArgs {
    /// 检索根目录
    #[arg(value_name = "ROOT")]
    pub root: PathBuf,
    /// 文件名 glob：`*`/`?` 不跨目录，`**` 跨目录；含分隔符时匹配相对路径，否则只匹配文件名
    #[arg(long, value_name = "GLOB")]
    #[serde(default)]
    pub name: Option<String>,
    /// 内容子串（逐行匹配、大小写敏感）
    #[arg(long, value_name = "TEXT")]
    #[serde(default)]
    pub content: Option<String>,
    /// 结果条数上限，0 或不提供表示不限制；CLI 走全局 `--limit`（ToolCtx.limit），
    /// 此字段只服务 JSON 入口
    #[arg(skip)]
    #[serde(default)]
    pub limit: Option<usize>,
    /// 内容检索时单文件字节上限，超过则跳过
    #[arg(long, value_name = "BYTES", default_value_t = 1024 * 1024)]
    #[serde(default = "default_max_bytes")]
    pub max_bytes: u64,
}

fn default_max_bytes() -> u64 {
    1024 * 1024
}

pub fn build() -> Command {
    command_for::<FilesearchArgs>(&META)
}

pub fn run_cli(ctx: &mut ToolCtx, matches: &ArgMatches) -> Result<Value, RscmdError> {
    let args = <FilesearchArgs as clap::FromArgMatches>::from_arg_matches(matches)
        .map_err(RscmdError::from_clap)?;
    run(ctx, args)
}

pub fn run_json(ctx: &mut ToolCtx, input: &Value) -> Result<Value, RscmdError> {
    let args: FilesearchArgs = serde_json::from_value(input.clone())
        .map_err(|err| RscmdError::coded("INVALID_ARGS", err.to_string()).with_field("args"))?;
    run(ctx, args)
}

fn run(ctx: &mut ToolCtx, args: FilesearchArgs) -> Result<Value, RscmdError> {
    let limit = args.limit.or(ctx.limit);
    let root = args.root.clone();
    if root.as_os_str().is_empty() {
        return Err(RscmdError::coded("INVALID_ARGS", "ROOT 不能为空")
            .with_field("root")
            .with_expected("已存在的目录"));
    }
    if args
        .name
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(RscmdError::coded("INVALID_ARGS", "--name 不能为空")
            .with_field("name")
            .with_expected("非空 glob"));
    }
    if args.content.as_deref().is_some_and(str::is_empty) {
        return Err(
            RscmdError::coded("INVALID_ARGS", "--content 不能为空字符串")
                .with_field("content")
                .with_expected("非空子串"),
        );
    }
    let name = args.name.as_deref().map(str::trim);
    let content = args.content.as_deref();
    if name.is_none() && content.is_none() {
        return Err(
            RscmdError::coded("INVALID_ARGS", "至少需要 --name 或 --content 之一")
                .with_field("name")
                .with_expected("--name 与 --content 至少一个"),
        );
    }

    let metadata = std::fs::metadata(&root).map_err(|err| {
        RscmdError::coded("IO", format!("无法访问检索根目录: {err}")).with_field("root")
    })?;
    if !metadata.is_dir() {
        return Err(RscmdError::coded("INVALID_ARGS", "ROOT 必须是目录")
            .with_field("root")
            .with_expected("已存在的目录"));
    }

    let mut files: Vec<PathBuf> = Vec::new();
    walk_files(&root, |path| {
        files.push(path.to_path_buf());
        true
    })
    .map_err(|err| RscmdError::coded("IO", format!("遍历目录失败: {err}")).with_field("root"))?;
    // 排序后遍历，保证 `--limit` 截断时返回的集合稳定。
    files.sort();
    let scanned = files.len();

    let mut results: Vec<Value> = Vec::new();
    let mut truncated = false;
    for path in &files {
        let relative = relative_slash(&root, path);
        if let Some(pattern) = name {
            let file_name = path
                .file_name()
                .map(|value| value.to_string_lossy())
                .unwrap_or_default();
            let target = if pattern.contains('/') || pattern.contains('\\') {
                relative.as_str()
            } else {
                file_name.as_ref()
            };
            if !glob_match(pattern, target) {
                continue;
            }
        }

        match content {
            None => {
                let Ok(metadata) = std::fs::metadata(path) else {
                    continue;
                };
                results.push(json!({"path": relative, "bytes": metadata.len()}));
            }
            Some(needle) => {
                let Ok(metadata) = std::fs::metadata(path) else {
                    continue;
                };
                if metadata.len() > args.max_bytes || !is_text_file(path).unwrap_or(false) {
                    continue;
                }
                let Ok(text) = read_text(path) else {
                    continue;
                };
                for (index, line) in text.lines().enumerate() {
                    if line.contains(needle) {
                        results.push(json!({
                            "path": relative,
                            "line": index + 1,
                            "text": line,
                        }));
                        if limit_reached(limit, results.len()) {
                            truncated = true;
                            break;
                        }
                    }
                }
                if truncated {
                    break;
                }
                continue;
            }
        }

        if limit_reached(limit, results.len()) {
            truncated = true;
            break;
        }
    }

    Ok(json!({
        "root": args.root.to_string_lossy(),
        "scanned": scanned,
        "matched": results.len(),
        "truncated": truncated,
        "results": results,
    }))
}

/// 上限为 `None` 或 0 时表示不限制。
fn limit_reached(limit: Option<usize>, count: usize) -> bool {
    matches!(limit, Some(value) if value > 0 && count >= value)
}

/// 输出相对 `ROOT` 的路径，统一使用 `/` 分隔；无法剥离前缀时退化为完整路径。
fn relative_slash(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub fn def() -> ToolDef {
    ToolDef {
        meta: &META,
        build,
        run: run_cli,
        run_json: Some(run_json),
        schema: Some(filesearch_args_schema),
    }
}

fn filesearch_args_schema() -> String {
    let schema = schemars::schema_for!(FilesearchArgs);
    serde_json::to_string_pretty(&schema).expect("FilesearchArgs schema 可序列化")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rscmd_runtime::fs::temp_dir;
    use std::time::{Duration, Instant};

    fn matches(args: &[&str]) -> ArgMatches {
        build().try_get_matches_from(args).expect("参数应可解析")
    }

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content.as_bytes()).unwrap();
    }

    fn run_args(ctx: &mut ToolCtx, args: FilesearchArgs) -> Result<Value, RscmdError> {
        run(ctx, args)
    }

    fn args(root: &Path) -> FilesearchArgs {
        FilesearchArgs {
            root: root.to_path_buf(),
            name: None,
            content: None,
            limit: None,
            max_bytes: 1024 * 1024,
        }
    }

    #[test]
    fn name_glob_matches_basename_across_directories() {
        let dir = temp_dir().unwrap();
        write(&dir.path().join("a.txt"), "x");
        write(&dir.path().join("sub/b.txt"), "x");
        write(&dir.path().join("sub/c.md"), "x");
        let mut input = args(dir.path());
        input.name = Some("*.txt".to_string());
        let out = run_args(&mut ToolCtx::default(), input).unwrap();
        assert_eq!(out["matched"], 2, "{out}");
        assert_eq!(out["scanned"], 3);
        assert_eq!(out["truncated"], false);
        let paths: Vec<&str> = out["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["path"].as_str().unwrap())
            .collect();
        assert_eq!(paths, ["a.txt", "sub/b.txt"]);
    }

    #[test]
    fn name_glob_with_separator_matches_relative_path() {
        let dir = temp_dir().unwrap();
        write(&dir.path().join("sub/a.txt"), "x");
        write(&dir.path().join("sub/deep/b.txt"), "x");
        let mut input = args(dir.path());
        input.name = Some("sub/*.txt".to_string());
        let out = run_args(&mut ToolCtx::default(), input).unwrap();
        assert_eq!(out["matched"], 1);

        let mut input = args(dir.path());
        input.name = Some("sub/**/*.txt".to_string());
        let out = run_args(&mut ToolCtx::default(), input).unwrap();
        assert_eq!(out["matched"], 2);
    }

    #[test]
    fn result_paths_are_relative_and_use_forward_slashes() {
        let dir = temp_dir().unwrap();
        write(&dir.path().join("deep/nested/a.txt"), "x");
        let mut input = args(dir.path());
        input.name = Some("**/*.txt".to_string());
        let out = run_args(&mut ToolCtx::default(), input).unwrap();
        let path = out["results"][0]["path"].as_str().unwrap();
        assert_eq!(path, "deep/nested/a.txt");
        assert!(!path.contains('\\'));
    }

    #[test]
    fn content_search_reports_line_numbers_and_strips_cr() {
        let dir = temp_dir().unwrap();
        write(&dir.path().join("crlf.txt"), "first\r\nneedle here\r\nlast");
        let mut input = args(dir.path());
        input.content = Some("needle".to_string());
        let out = run_args(&mut ToolCtx::default(), input).unwrap();
        assert_eq!(out["matched"], 1);
        assert_eq!(out["results"][0]["path"], "crlf.txt");
        assert_eq!(out["results"][0]["line"], 2);
        assert_eq!(out["results"][0]["text"], "needle here");
    }

    #[test]
    fn content_search_reports_every_matching_line() {
        let dir = temp_dir().unwrap();
        write(&dir.path().join("many.txt"), "hit\nmiss\nhit");
        let mut input = args(dir.path());
        input.content = Some("hit".to_string());
        let out = run_args(&mut ToolCtx::default(), input).unwrap();
        assert_eq!(out["matched"], 2);
        assert_eq!(out["results"][0]["line"], 1);
        assert_eq!(out["results"][1]["line"], 3);
    }

    #[test]
    fn name_and_content_filters_combine() {
        let dir = temp_dir().unwrap();
        write(&dir.path().join("a.txt"), "needle");
        write(&dir.path().join("b.md"), "needle");
        let mut input = args(dir.path());
        input.name = Some("*.txt".to_string());
        input.content = Some("needle".to_string());
        let out = run_args(&mut ToolCtx::default(), input).unwrap();
        assert_eq!(out["matched"], 1);
        assert_eq!(out["results"][0]["path"], "a.txt");
    }

    #[test]
    fn content_search_skips_binary_and_oversized_files() {
        let dir = temp_dir().unwrap();
        std::fs::write(dir.path().join("blob.dat"), [0u8; 64]).unwrap();
        write(&dir.path().join("big.txt"), &"needle".repeat(32));
        write(&dir.path().join("ok.txt"), "needle");
        let mut input = args(dir.path());
        input.content = Some("needle".to_string());
        input.max_bytes = 16;
        let out = run_args(&mut ToolCtx::default(), input).unwrap();
        assert_eq!(out["matched"], 1, "{out}");
        assert_eq!(out["results"][0]["path"], "ok.txt");
        assert_eq!(out["scanned"], 3);
    }

    #[test]
    fn content_search_skips_non_utf8_files() {
        let dir = temp_dir().unwrap();
        std::fs::write(dir.path().join("bad.txt"), b"needle \xff\xfe").unwrap();
        write(&dir.path().join("good.txt"), "needle");
        let mut input = args(dir.path());
        input.content = Some("needle".to_string());
        let out = run_args(&mut ToolCtx::default(), input).unwrap();
        assert_eq!(out["matched"], 1);
        assert_eq!(out["results"][0]["path"], "good.txt");
    }

    #[test]
    fn zero_or_absent_limit_means_unlimited() {
        let dir = temp_dir().unwrap();
        for index in 0..5 {
            write(&dir.path().join(format!("f{index}.txt")), "x");
        }
        let mut input = args(dir.path());
        input.name = Some("*.txt".to_string());
        let out = run_args(&mut ToolCtx::default(), input).unwrap();
        assert_eq!(out["matched"], 5);
        assert_eq!(out["truncated"], false);

        // JSON 入口用 0 显式表示不限制。
        let mut input = args(dir.path());
        input.name = Some("*.txt".to_string());
        input.limit = Some(0);
        let out = run_args(&mut ToolCtx::default(), input).unwrap();
        assert_eq!(out["matched"], 5);
        assert_eq!(out["truncated"], false);
    }

    #[test]
    fn global_context_limit_is_consumed() {
        let dir = temp_dir().unwrap();
        for index in 0..5 {
            write(&dir.path().join(format!("f{index}.txt")), "x");
        }
        let mut input = args(dir.path());
        input.name = Some("*.txt".to_string());
        let mut ctx = ToolCtx::new(false, false, false, Some(2), 0, false);
        let out = run_args(&mut ctx, input).unwrap();
        assert_eq!(out["matched"], 2);
        assert_eq!(out["truncated"], true);
    }

    #[test]
    fn case_sensitive_on_every_platform() {
        let dir = temp_dir().unwrap();
        write(&dir.path().join("a.txt"), "x");
        let mut input = args(dir.path());
        input.name = Some("*.TXT".to_string());
        let out = run_args(&mut ToolCtx::default(), input).unwrap();
        assert_eq!(out["matched"], 0);
    }

    #[test]
    fn missing_root_is_io_error_with_field() {
        let input = FilesearchArgs {
            root: PathBuf::from("definitely-not-a-real-root-xyz"),
            name: Some("*.txt".to_string()),
            ..args(Path::new("."))
        };
        let err = run_args(&mut ToolCtx::default(), input).unwrap_err();
        assert_eq!(err.code, "IO");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(err.field.as_deref(), Some("root"));
    }

    #[test]
    fn file_root_is_invalid_args() {
        let dir = temp_dir().unwrap();
        let file = dir.path().join("plain.txt");
        write(&file, "x");
        let input = FilesearchArgs {
            root: file,
            name: Some("*.txt".to_string()),
            ..args(dir.path())
        };
        let err = run_args(&mut ToolCtx::default(), input).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("root"));
    }

    #[test]
    fn requires_name_or_content() {
        let dir = temp_dir().unwrap();
        let err = run_args(&mut ToolCtx::default(), args(dir.path())).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("name"));
    }

    #[test]
    fn rejects_empty_filters_and_root() {
        let dir = temp_dir().unwrap();
        let mut input = args(dir.path());
        input.name = Some("  ".to_string());
        let err = run_args(&mut ToolCtx::default(), input).unwrap_err();
        assert_eq!(err.field.as_deref(), Some("name"));

        let mut input = args(dir.path());
        input.content = Some(String::new());
        let err = run_args(&mut ToolCtx::default(), input).unwrap_err();
        assert_eq!(err.field.as_deref(), Some("content"));

        let mut input = args(Path::new(""));
        input.name = Some("*.txt".to_string());
        let err = run_args(&mut ToolCtx::default(), input).unwrap_err();
        assert_eq!(err.field.as_deref(), Some("root"));
    }

    #[test]
    fn cli_entry_matches_json_shape() {
        let dir = temp_dir().unwrap();
        write(&dir.path().join("a.txt"), "needle");
        let root = dir.path().to_string_lossy().into_owned();
        let out = run_cli(
            &mut ToolCtx::default(),
            &matches(&["filesearch", &root, "--content", "needle"]),
        )
        .unwrap();
        assert_eq!(out["matched"], 1);
        assert_eq!(out["results"][0]["line"], 1);

        let out = run_json(
            &mut ToolCtx::default(),
            &json!({"root": root, "content": "needle"}),
        )
        .unwrap();
        assert_eq!(out["matched"], 1);
    }

    #[test]
    fn json_entry_applies_defaults_and_rejects_bad_values() {
        let dir = temp_dir().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        let out = run_json(
            &mut ToolCtx::default(),
            &json!({"root": root, "name": "*.txt"}),
        )
        .unwrap();
        assert_eq!(out["matched"], 0);

        let err = run_json(
            &mut ToolCtx::default(),
            &json!({"root": root, "name": "*.txt", "limit": -1}),
        )
        .unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("args"));

        let err = run_json(&mut ToolCtx::default(), &json!({"name": "*.txt"})).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
    }

    #[test]
    fn limit_caps_scan_on_large_directory() {
        let dir = temp_dir().unwrap();
        for index in 0..5000 {
            std::fs::write(dir.path().join(format!("f{index:04}.txt")), "needle\n").unwrap();
        }

        let mut input = args(dir.path());
        input.name = Some("*.txt".to_string());
        // 文件名模式走全局 `--limit` 注入的 ToolCtx.limit。
        let mut ctx = ToolCtx::new(false, false, false, Some(10), 0, false);
        let started = Instant::now();
        let out = run_args(&mut ctx, input).unwrap();
        let elapsed = started.elapsed();
        assert_eq!(out["scanned"], 5000);
        assert_eq!(out["matched"], 10);
        assert_eq!(out["truncated"], true);
        assert!(
            elapsed < Duration::from_secs(5),
            "文件名模式下 --limit 应提前停止，实际耗时 {elapsed:?}"
        );

        let mut input = args(dir.path());
        input.content = Some("needle".to_string());
        input.limit = Some(10);
        let started = Instant::now();
        let out = run_args(&mut ToolCtx::default(), input).unwrap();
        let elapsed = started.elapsed();
        assert_eq!(out["matched"], 10);
        assert_eq!(out["truncated"], true);
        assert!(
            elapsed < Duration::from_secs(5),
            "内容模式下 --limit 应提前停止，实际耗时 {elapsed:?}"
        );
    }

    #[test]
    fn def_exposes_structured_json_entry() {
        let tool = def();
        assert_eq!(tool.meta.name, "filesearch");
        assert_eq!(tool.meta.aliases, &["filesearchtool"]);
        assert!(tool.run_json.is_some());
        let schema = (tool.schema.expect("应提供 schema"))();
        assert!(schema.contains("\"name\""), "{schema}");
        assert!(schema.contains("\"content\""), "{schema}");
        assert!(schema.contains("\"limit\""), "{schema}");
    }
}
