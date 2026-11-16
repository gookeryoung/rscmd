//! `archive`：把目录打包成 zip（不加密、不删除源文件）。
//!
//! 只写不读：不会解包任何输入，因此不涉及不可信压缩数据的解析。
//! `-o/--output` 必填，不猜默认输出路径；写文件需 `--confirm`，`--dry-run` 免确认，
//! 只返回将要打包的清单（受全局 `--limit` 限制条数）。
//!
//! 条目按相对路径排序，路径统一用 `/` 分隔；条目的修改时间固定为 zip 的默认值
//! （1980-01-01），同一输入重复打包得到相同字节。符号链接一律跳过，避免打包到目录外
//! 的文件或形成环路。若输出文件位于源目录内，会被排除，避免把自己打包进去。
//!
//! 本工具只有 argv 入口（`python=argv_only`）：写操作需要 `--confirm`，而 JSON 调用
//! 通道没有 confirm/dry-run 参数，暴露 Structured 入口只会得到一个必然失败的 API。

use std::fs::{self, File};
use std::io;
use std::path::{Component, Path, PathBuf};

use clap::{ArgMatches, Command};
use rscmd_core::{
    command_for, PythonExposure, RscmdError, SideEffects, ToolCtx, ToolDef, ToolMeta,
};
use rscmd_runtime::fs::glob_match;
use serde_json::{json, Value};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

pub static META: ToolMeta = ToolMeta {
    name: "archive",
    aliases: &["archivetool"],
    summary: "把目录打包成 zip（不加密，写需 --confirm，--dry-run 列清单）",
    side_effects: SideEffects::Write,
    gui: false,
    python: PythonExposure::ArgvOnly,
    examples: &[
        "rscmd archive ./dist -o dist.zip --dry-run",
        "rscmd archive ./dist -o dist.zip --confirm",
        "rscmd archive . -o src.zip --include '**/*.rs' --exclude 'target/**' --confirm",
    ],
};

#[derive(Debug, clap::Args, schemars::JsonSchema)]
pub struct ArchiveArgs {
    /// 要打包的目录
    #[arg(value_name = "DIR")]
    pub directory: String,
    /// 输出 zip 路径（必填，不猜默认值）
    #[arg(short = 'o', long, value_name = "ZIP")]
    pub output: String,
    /// 只打包匹配的 glob（相对 DIR，可重复；`*`/`?` 不跨目录，`**` 跨目录）
    #[arg(long, value_name = "GLOB")]
    pub include: Vec<String>,
    /// 排除匹配的 glob（相对 DIR，可重复）
    #[arg(long, value_name = "GLOB")]
    pub exclude: Vec<String>,
}

struct PlannedFile {
    relative: String,
    absolute: PathBuf,
    bytes: u64,
}

pub fn build() -> Command {
    command_for::<ArchiveArgs>(&META)
}

pub fn run_cli(ctx: &mut ToolCtx, matches: &ArgMatches) -> Result<Value, RscmdError> {
    let args = <ArchiveArgs as clap::FromArgMatches>::from_arg_matches(matches)
        .map_err(RscmdError::from_clap)?;
    run(ctx, args)
}

fn run(ctx: &mut ToolCtx, args: ArchiveArgs) -> Result<Value, RscmdError> {
    let root = Path::new(&args.directory);
    let metadata = fs::metadata(root).map_err(|err| {
        RscmdError::coded("IO", format!("无法访问目录: {err}")).with_field("directory")
    })?;
    if !metadata.is_dir() {
        return Err(RscmdError::coded("INVALID_ARGS", "DIR 必须是目录")
            .with_field("directory")
            .with_expected("已存在的目录"));
    }
    let root = fs::canonicalize(root).map_err(|err| {
        RscmdError::coded("IO", format!("无法解析目录路径: {err}")).with_field("directory")
    })?;

    let output = Path::new(&args.output);
    validate_output(output)?;
    let skip = fs::canonicalize(output).ok();

    let files = collect(&root, &args.include, &args.exclude, skip.as_deref())?;
    let total_bytes: u64 = files.iter().map(|file| file.bytes).sum();

    ctx.require_confirm("打包目录并写入 zip")?;
    let dry_run = ctx.dry_run;
    if !dry_run {
        write_zip(output, &files)?;
    }

    let mut out = json!({
        "directory": args.directory,
        "output": args.output,
        "file_count": files.len(),
        "total_bytes": total_bytes,
        "dry_run": dry_run,
    });
    if dry_run {
        let limit = ctx.limit.filter(|limit| *limit > 0);
        let mut listed: Vec<&str> = files.iter().map(|file| file.relative.as_str()).collect();
        if let Some(limit) = limit {
            listed.truncate(limit);
        }
        out["files"] = json!(listed);
        out["truncated"] = json!(limit.is_some_and(|limit| limit < files.len()));
    }
    Ok(out)
}

fn validate_output(output: &Path) -> Result<(), RscmdError> {
    if let Ok(metadata) = fs::metadata(output) {
        if metadata.is_dir() {
            return Err(RscmdError::coded("INVALID_ARGS", "ZIP 输出路径是目录")
                .with_field("output")
                .with_expected("不存在的路径或普通文件"));
        }
    }
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(
            RscmdError::coded("INVALID_ARGS", "ZIP 输出路径的父目录不存在")
                .with_field("output")
                .with_expected("父目录已存在"),
        );
    }
    Ok(())
}

fn collect(
    root: &Path,
    include: &[String],
    exclude: &[String],
    skip: Option<&Path>,
) -> Result<Vec<PlannedFile>, RscmdError> {
    let mut files = Vec::new();
    walk(root, root, include, exclude, skip, &mut files)?;
    files.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(files)
}

fn walk(
    root: &Path,
    dir: &Path,
    include: &[String],
    exclude: &[String],
    skip: Option<&Path>,
    files: &mut Vec<PlannedFile>,
) -> Result<(), RscmdError> {
    let mut entries = fs::read_dir(dir)
        .map_err(io_error)?
        .collect::<io::Result<Vec<_>>>()
        .map_err(io_error)?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let file_type = entry.file_type().map_err(io_error)?;
        // 符号链接一律跳过：既避免打包目录外的文件，也避免目录环导致无限递归。
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            walk(root, &path, include, exclude, skip, files)?;
        } else if file_type.is_file() {
            if skip.is_some_and(|skip| skip == path) {
                continue;
            }
            let relative = relative_path(root, &path);
            if !selected(&relative, include, exclude) {
                continue;
            }
            let bytes = entry.metadata().map_err(io_error)?.len();
            files.push(PlannedFile {
                relative,
                absolute: path,
                bytes,
            });
        }
    }
    Ok(())
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn selected(relative: &str, include: &[String], exclude: &[String]) -> bool {
    let included =
        include.is_empty() || include.iter().any(|pattern| glob_match(pattern, relative));
    included && !exclude.iter().any(|pattern| glob_match(pattern, relative))
}

fn write_zip(output: &Path, files: &[PlannedFile]) -> Result<(), RscmdError> {
    let file = File::create(output).map_err(io_error)?;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for entry in files {
        writer
            .start_file(entry.relative.as_str(), options)
            .map_err(zip_error)?;
        let mut source = File::open(&entry.absolute).map_err(io_error)?;
        io::copy(&mut source, &mut writer).map_err(io_error)?;
    }
    writer.finish().map_err(zip_error)?;
    Ok(())
}

fn io_error(err: io::Error) -> RscmdError {
    RscmdError::coded("IO", format!("文件操作失败: {err}"))
        .with_field("directory")
        .with_expected("可读的目录与可写的输出路径")
}

fn zip_error(err: zip::result::ZipError) -> RscmdError {
    RscmdError::coded("IO", format!("写入 zip 失败: {err}")).with_field("output")
}

pub fn def() -> ToolDef {
    ToolDef {
        meta: &META,
        build,
        run: run_cli,
        run_json: None,
        schema: Some(archive_args_schema),
    }
}

fn archive_args_schema() -> String {
    let schema = schemars::schema_for!(ArchiveArgs);
    serde_json::to_string_pretty(&schema).expect("ArchiveArgs schema 可序列化")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    use rscmd_runtime::fs::temp_dir;
    use zip::ZipArchive;

    fn matches(args: &[&str]) -> ArgMatches {
        build().try_get_matches_from(args).expect("参数应可解析")
    }

    fn sample() -> (rscmd_runtime::fs::TempDir, String, String) {
        let dir = temp_dir().expect("应创建临时目录");
        fs::create_dir_all(dir.path().join("sub")).expect("应创建子目录");
        fs::write(dir.path().join("b.txt"), "bbb").unwrap();
        fs::write(dir.path().join("a.txt"), "aa").unwrap();
        fs::write(dir.path().join("sub/c.txt"), "c").unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        let output = dir.path().join("out.zip").to_string_lossy().into_owned();
        (dir, root, output)
    }

    fn entry_names(path: &str) -> Vec<String> {
        let file = File::open(path).expect("应能打开 zip");
        let mut archive = ZipArchive::new(file).expect("应是合法 zip");
        (0..archive.len())
            .map(|index| archive.by_index(index).unwrap().name().to_string())
            .collect()
    }

    fn entry_text(path: &str, name: &str) -> String {
        let file = File::open(path).expect("应能打开 zip");
        let mut archive = ZipArchive::new(file).expect("应是合法 zip");
        let mut entry = archive.by_name(name).expect("条目应存在");
        let mut text = String::new();
        entry.read_to_string(&mut text).unwrap();
        text
    }

    #[test]
    fn dry_run_lists_sorted_plan_without_writing() {
        let (_dir, root, output) = sample();
        let mut ctx = ToolCtx {
            dry_run: true,
            ..ToolCtx::default()
        };
        let out = run_cli(&mut ctx, &matches(&["archive", &root, "-o", &output])).unwrap();
        assert_eq!(out["file_count"], 3);
        assert_eq!(out["total_bytes"], 6);
        assert_eq!(out["dry_run"], json!(true));
        assert_eq!(out["files"], json!(["a.txt", "b.txt", "sub/c.txt"]));
        assert_eq!(out["truncated"], json!(false));
        assert!(!Path::new(&output).exists());
    }

    #[test]
    fn dry_run_respects_global_limit() {
        let (_dir, root, output) = sample();
        let mut ctx = ToolCtx {
            dry_run: true,
            limit: Some(2),
            ..ToolCtx::default()
        };
        let out = run_cli(&mut ctx, &matches(&["archive", &root, "-o", &output])).unwrap();
        assert_eq!(out["file_count"], 3);
        assert_eq!(out["files"], json!(["a.txt", "b.txt"]));
        assert_eq!(out["truncated"], json!(true));
    }

    #[test]
    fn write_requires_confirm() {
        let (_dir, root, output) = sample();
        let err = run_cli(
            &mut ToolCtx::default(),
            &matches(&["archive", &root, "-o", &output]),
        )
        .unwrap_err();
        assert_eq!(err.code, "CONFIRM_REQUIRED");
        assert_eq!(err.exit_code(), 2);
        assert!(!Path::new(&output).exists());
    }

    #[test]
    fn writes_zip_after_confirm() {
        let (_dir, root, output) = sample();
        let mut ctx = ToolCtx {
            confirm: true,
            ..ToolCtx::default()
        };
        let out = run_cli(&mut ctx, &matches(&["archive", &root, "-o", &output])).unwrap();
        assert_eq!(out["file_count"], 3);
        assert_eq!(out["dry_run"], json!(false));
        assert!(out.get("files").is_none());
        assert_eq!(entry_names(&output), vec!["a.txt", "b.txt", "sub/c.txt"]);
        assert_eq!(entry_text(&output, "b.txt"), "bbb");
        assert_eq!(entry_text(&output, "sub/c.txt"), "c");
    }

    #[test]
    fn include_and_exclude_filters_apply() {
        let (_dir, root, output) = sample();
        let mut ctx = ToolCtx {
            confirm: true,
            ..ToolCtx::default()
        };
        let out = run_cli(
            &mut ctx,
            &matches(&[
                "archive",
                &root,
                "-o",
                &output,
                "--include",
                "**/*.txt",
                "--exclude",
                "sub/**",
            ]),
        )
        .unwrap();
        assert_eq!(out["file_count"], 2);
        assert_eq!(entry_names(&output), vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn output_inside_source_is_excluded() {
        let (_dir, root, output) = sample();
        fs::write(&output, "stale").unwrap();
        let mut ctx = ToolCtx {
            confirm: true,
            ..ToolCtx::default()
        };
        let out = run_cli(&mut ctx, &matches(&["archive", &root, "-o", &output])).unwrap();
        assert_eq!(out["file_count"], 3);
        assert_eq!(entry_names(&output), vec!["a.txt", "b.txt", "sub/c.txt"]);
    }

    #[test]
    fn output_bytes_are_reproducible() {
        let (_dir, root, _output) = sample();
        let out_dir = temp_dir().unwrap();
        let first = out_dir
            .path()
            .join("one.zip")
            .to_string_lossy()
            .into_owned();
        let second = out_dir
            .path()
            .join("two.zip")
            .to_string_lossy()
            .into_owned();
        for output in [&first, &second] {
            let mut ctx = ToolCtx {
                confirm: true,
                ..ToolCtx::default()
            };
            run_cli(&mut ctx, &matches(&["archive", &root, "-o", output])).unwrap();
        }
        assert_eq!(fs::read(&first).unwrap(), fs::read(&second).unwrap());
    }

    #[test]
    fn empty_directory_produces_empty_zip() {
        let dir = temp_dir().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        let output = dir.path().join("empty.zip").to_string_lossy().into_owned();
        let mut ctx = ToolCtx {
            confirm: true,
            ..ToolCtx::default()
        };
        let out = run_cli(&mut ctx, &matches(&["archive", &root, "-o", &output])).unwrap();
        assert_eq!(out["file_count"], 0);
        assert_eq!(out["total_bytes"], 0);
        assert!(entry_names(&output).is_empty());
    }

    #[test]
    fn directory_must_be_a_directory() {
        let (dir, _root, output) = sample();
        let file = dir.path().join("a.txt").to_string_lossy().into_owned();
        let err = run_cli(
            &mut ToolCtx::default(),
            &matches(&["archive", &file, "-o", &output]),
        )
        .unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.exit_code(), 3);
        assert_eq!(err.field.as_deref(), Some("directory"));
    }

    #[test]
    fn missing_directory_is_io_error() {
        let (_dir, root, output) = sample();
        let missing = format!("{root}/nope");
        let err = run_cli(
            &mut ToolCtx::default(),
            &matches(&["archive", &missing, "-o", &output]),
        )
        .unwrap_err();
        assert_eq!(err.code, "IO");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(err.field.as_deref(), Some("directory"));
        assert!(!Path::new(&output).exists());
    }

    #[test]
    fn missing_output_parent_is_invalid_args() {
        let (_dir, root, _output) = sample();
        let output = format!("{root}/missing/out.zip");
        let err = run_cli(
            &mut ToolCtx::default(),
            &matches(&["archive", &root, "-o", &output]),
        )
        .unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("output"));
    }

    #[test]
    fn output_that_is_a_directory_is_invalid_args() {
        let (_dir, root, _output) = sample();
        let err = run_cli(
            &mut ToolCtx::default(),
            &matches(&["archive", &root, "-o", &root]),
        )
        .unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("output"));
    }

    #[test]
    fn output_flag_is_required() {
        let (_dir, root, _output) = sample();
        let err = build()
            .try_get_matches_from(["archive", &root])
            .unwrap_err();
        let rendered = err.to_string();
        assert!(rendered.contains("--output"), "{rendered}");
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_skipped() {
        use std::os::unix::fs::symlink;

        let (dir, root, output) = sample();
        symlink(dir.path().join("a.txt"), dir.path().join("link.txt")).unwrap();
        symlink(dir.path().join("sub"), dir.path().join("link-dir")).unwrap();
        let mut ctx = ToolCtx {
            confirm: true,
            ..ToolCtx::default()
        };
        let out = run_cli(&mut ctx, &matches(&["archive", &root, "-o", &output])).unwrap();
        assert_eq!(out["file_count"], 3);
        assert_eq!(entry_names(&output), vec!["a.txt", "b.txt", "sub/c.txt"]);
    }

    #[test]
    fn def_exposes_argv_only_exposure() {
        let tool = def();
        assert_eq!(tool.meta.name, "archive");
        assert_eq!(tool.meta.aliases, &["archivetool"]);
        assert_eq!(tool.meta.side_effects, SideEffects::Write);
        assert_eq!(tool.meta.python, PythonExposure::ArgvOnly);
        assert!(tool.run_json.is_none());
        let schema = (tool.schema.expect("应提供 schema 供文档使用"))();
        for field in ["\"directory\"", "\"output\"", "\"include\"", "\"exclude\""] {
            assert!(schema.contains(field), "{field} 不在 schema: {schema}");
        }
    }

    #[test]
    fn bare_output_filename_resolves_against_cwd() {
        // 只校验路径形状，不创建任何文件。
        validate_output(Path::new("rscmd-archive-bare-output.zip")).unwrap();
    }

    #[test]
    fn relative_path_skips_non_normal_components() {
        assert_eq!(
            relative_path(Path::new("/root"), Path::new("/root/../x")),
            "x"
        );
    }

    #[test]
    fn io_error_maps_to_directory_field() {
        let err = io_error(io::Error::new(io::ErrorKind::PermissionDenied, "denied"));
        assert_eq!(err.code, "IO");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(err.field.as_deref(), Some("directory"));
    }

    #[test]
    fn zip_error_maps_to_output_field() {
        let err = zip_error(zip::result::ZipError::InvalidArchive(
            std::borrow::Cow::Borrowed("bad"),
        ));
        assert_eq!(err.code, "IO");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(err.field.as_deref(), Some("output"));
    }
}
