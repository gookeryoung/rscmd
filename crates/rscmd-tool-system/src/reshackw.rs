//! `reshackw`：拉起 `reshack` 的 GUI 前端（`rscmd-reshackw`）。
//!
//! GUI 是 `rscmd-gui` 提供的独立二进制，依赖 `eframe` / `image` / `arboard`，
//! 只在 `--features gui` 时编译。本工具只负责「定位 + 启动」，因此默认构建
//! （CLI / PyO3 / wheel / MCP）不会带上 GUI 依赖，也不会改变既有信封与退出码。
//!
//! 定位顺序（命中即用）：
//! 1. 环境变量 `RSCMD_RESHACKW`（显式指定优先，不存在则直接报错，不静默回退）；
//! 2. `rscmd` 可执行文件同目录；
//! 3. `PATH`（`cargo install` 装到同一 bin 目录时命中）；
//! 4. 源码树的 `target/{debug,release}`（从 cwd 逐级向上，含 `CARGO_TARGET_DIR`）——
//!    Python 包 / `uv tool` 的入口不是 Rust 二进制，靠这条才能找到本地构建的 GUI。
//!
//! 启动后立即返回，不等待窗口关闭；成功时返回 `gui` / `pid` / `file`。

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::{ArgMatches, Command as ClapCommand};
use rscmd_core::{
    command_for, PythonExposure, RscmdError, SideEffects, ToolCtx, ToolDef, ToolMeta,
};
use rscmd_runtime::which::{find as find_in_path, WhichEnv};
use serde_json::{json, Value};

pub static META: ToolMeta = ToolMeta {
    name: "reshackw",
    aliases: &["reshackgui"],
    summary: "启动 reshack 资源编辑器窗口（GUI 二进制 rscmd-reshackw，可带一个待打开文件）",
    side_effects: SideEffects::None,
    gui: true,
    python: PythonExposure::Hidden,
    examples: &["rscmd reshackw", "rscmd reshackw app.exe"],
};

/// 覆盖 GUI 二进制路径的环境变量。
pub const GUI_ENV: &str = "RSCMD_RESHACKW";
/// GUI 二进制名（不含平台后缀）。
pub const GUI_BINARY: &str = "rscmd-reshackw";

/// 查找时可接受的候选文件名：Windows 优先 `.exe`，Unix 只有无后缀形式。
const GUI_BINARY_NAMES: &[&str] = if cfg!(windows) {
    &["rscmd-reshackw.exe", "rscmd-reshackw"]
} else {
    &["rscmd-reshackw"]
};

#[derive(Debug, clap::Args, serde::Deserialize, schemars::JsonSchema)]
pub struct ReshackwArgs {
    /// 启动时打开的文件（PE 或已导出的 ICO/BMP/PNG）；缺省只开窗口
    #[arg(value_name = "FILE")]
    pub file: Option<String>,
}

pub fn build() -> ClapCommand {
    command_for::<ReshackwArgs>(&META)
}

pub fn run_cli(ctx: &mut ToolCtx, matches: &ArgMatches) -> Result<Value, RscmdError> {
    let args = <ReshackwArgs as clap::FromArgMatches>::from_arg_matches(matches)
        .map_err(RscmdError::from_clap)?;
    run(ctx, args)
}

fn run(_ctx: &mut ToolCtx, args: ReshackwArgs) -> Result<Value, RscmdError> {
    let env_override = std::env::var_os(GUI_ENV).map(PathBuf::from);
    let binary = resolve_gui(env_override, &candidate_dirs())?;

    let mut command = Command::new(&binary);
    if let Some(file) = args.file.as_deref() {
        command.arg("--open").arg(file);
    }
    let child = command.spawn().map_err(|err| {
        RscmdError::coded("IO", format!("启动 GUI `{}` 失败: {err}", binary.display()))
            .with_hint("确认该文件是可执行的 rscmd-reshackw，且当前用户有执行权限")
    })?;

    Ok(json!({
        "gui": binary.to_string_lossy().into_owned(),
        "pid": child.id(),
        "file": args.file,
    }))
}

/// 按优先级收集候选目录：可执行文件所在目录 → PATH 命中项 → 源码树 target 目录。
///
/// 目录本身不存在的也保留，错误信息里能看到完整的搜寻范围。
fn candidate_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            push_unique(&mut dirs, parent.to_path_buf());
        }
    }
    if let Some(found) = find_in_path(GUI_BINARY, &WhichEnv::from_process()) {
        if let Some(parent) = found.parent() {
            push_unique(&mut dirs, parent.to_path_buf());
        }
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    for dir in source_tree_target_dirs(&cwd, std::env::var_os("CARGO_TARGET_DIR")) {
        push_unique(&mut dirs, dir);
    }
    dirs
}

/// 源码树的候选输出目录：`CARGO_TARGET_DIR` 优先，其后从 cwd 逐级向上找 `target/{debug,release}`。
fn source_tree_target_dirs(cwd: &Path, cargo_target_dir: Option<OsString>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(target_dir) = cargo_target_dir {
        let target_dir = PathBuf::from(target_dir);
        for profile in ["debug", "release"] {
            dirs.push(target_dir.join(profile));
        }
    }
    for ancestor in cwd.ancestors() {
        for profile in ["debug", "release"] {
            dirs.push(ancestor.join("target").join(profile));
        }
    }
    dirs
}

fn push_unique(dirs: &mut Vec<PathBuf>, dir: PathBuf) {
    if !dirs.contains(&dir) {
        dirs.push(dir);
    }
}

/// 解析 GUI 二进制路径。
///
/// `env_override` 非空时**只**认该路径：显式指定的文件不存在就报错，
/// 避免用户以为用了自定义二进制、实际却跑同目录的旧产物。
fn resolve_gui(env_override: Option<PathBuf>, dirs: &[PathBuf]) -> Result<PathBuf, RscmdError> {
    if let Some(path) = env_override {
        return if path.is_file() {
            Ok(path)
        } else {
            Err(unavailable(format!(
                "{GUI_ENV} 指向的 GUI 二进制不存在: {}",
                path.display()
            )))
        };
    }

    for dir in dirs {
        for name in GUI_BINARY_NAMES {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    let shown = dirs
        .iter()
        .take(6)
        .map(|dir| dir.display().to_string())
        .collect::<Vec<_>>()
        .join("、");
    let more = dirs.len().saturating_sub(6);
    let suffix = if more > 0 {
        format!(" 等 {} 个目录", dirs.len())
    } else if shown.is_empty() {
        "（没有可搜寻的目录）".to_string()
    } else {
        String::new()
    };
    Err(unavailable(format!(
        "未找到 GUI 二进制（已尝试 {shown}{suffix}）"
    )))
}

fn unavailable(message: String) -> RscmdError {
    RscmdError::coded("GUI_UNAVAILABLE", message).with_hint(format!(
        "运行 `cargo build -p rscmd-gui --features gui` 生成 {GUI_BINARY}，或用环境变量 {GUI_ENV} 指定其路径"
    ))
}

pub fn def() -> ToolDef {
    ToolDef {
        meta: &META,
        build,
        run: run_cli,
        run_json: None,
        schema: Some(reshackw_args_schema),
    }
}

fn reshackw_args_schema() -> String {
    let schema = schemars::schema_for!(ReshackwArgs);
    serde_json::to_string_pretty(&schema).expect("ReshackwArgs schema 可序列化")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(args: &[&str]) -> ArgMatches {
        build().try_get_matches_from(args).expect("参数应可解析")
    }

    #[test]
    fn file_is_optional() {
        let args =
            <ReshackwArgs as clap::FromArgMatches>::from_arg_matches(&matches(&["reshackw"]))
                .unwrap();
        assert!(args.file.is_none());

        let args = <ReshackwArgs as clap::FromArgMatches>::from_arg_matches(&matches(&[
            "reshackw", "app.exe",
        ]))
        .unwrap();
        assert_eq!(args.file.as_deref(), Some("app.exe"));
    }

    #[test]
    fn env_override_wins_when_it_exists() {
        let temp = rscmd_runtime::fs::temp_dir().expect("临时目录可用");
        let fake = temp.path().join("custom-reshackw");
        std::fs::write(&fake, b"stub").unwrap();
        let resolved = resolve_gui(Some(fake.clone()), &[]).expect("应优先采用环境变量");
        assert_eq!(resolved, fake);
    }

    #[test]
    fn env_override_missing_is_an_error_not_a_fallback() {
        let temp = rscmd_runtime::fs::temp_dir().expect("临时目录可用");
        let missing = temp.path().join("nope-reshackw");
        let err = resolve_gui(Some(missing), &[temp.path().to_path_buf()]).unwrap_err();
        assert_eq!(err.code, "GUI_UNAVAILABLE");
        assert!(err.hint.unwrap().contains(GUI_ENV));
    }

    #[test]
    fn falls_back_to_sibling_binary() {
        let temp = rscmd_runtime::fs::temp_dir().expect("临时目录可用");
        let sibling = temp.path().join(GUI_BINARY_NAMES[0]);
        std::fs::write(&sibling, b"stub").unwrap();
        let resolved = resolve_gui(None, &[temp.path().to_path_buf()]).expect("应找到同目录二进制");
        assert_eq!(resolved, sibling);
    }

    #[test]
    fn searches_later_directories_in_order() {
        let temp = rscmd_runtime::fs::temp_dir().expect("临时目录可用");
        let empty = temp.path().join("empty");
        let target = temp.path().join("target").join("debug");
        std::fs::create_dir_all(&empty).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        let binary = target.join(GUI_BINARY_NAMES[0]);
        std::fs::write(&binary, b"stub").unwrap();
        let resolved = resolve_gui(None, &[empty, target]).expect("应命中后面的源码树目录");
        assert_eq!(resolved, binary);
    }

    #[test]
    fn source_tree_dirs_include_cargo_target_dir_then_ancestors() {
        let dirs = source_tree_target_dirs(Path::new("C:/repo/sub"), Some(OsString::from("D:/td")));
        assert_eq!(dirs[0], Path::new("D:/td/debug"));
        assert_eq!(dirs[1], Path::new("D:/td/release"));
        assert!(dirs.contains(&Path::new("C:/repo/sub/target/debug").to_path_buf()));
        assert!(dirs.contains(&Path::new("C:/repo/target/release").to_path_buf()));
    }

    #[test]
    fn missing_binary_lists_searched_directories() {
        let temp = rscmd_runtime::fs::temp_dir().expect("临时目录可用");
        let err = resolve_gui(None, &[temp.path().to_path_buf()]).unwrap_err();
        assert_eq!(err.code, "GUI_UNAVAILABLE");
        assert!(err.message.contains("未找到 GUI 二进制"), "{}", err.message);
        assert!(err.message.contains(&temp.path().display().to_string()));
        assert!(err.hint.unwrap().contains("--features gui"));
    }

    #[test]
    fn error_message_caps_directory_list() {
        let temp = rscmd_runtime::fs::temp_dir().expect("临时目录可用");
        let dirs: Vec<PathBuf> = (0..8)
            .map(|index| temp.path().join(format!("d{index}")))
            .collect();
        let err = resolve_gui(None, &dirs).unwrap_err();
        assert!(err.message.contains("等 8 个目录"), "{}", err.message);

        let err = resolve_gui(None, &[]).unwrap_err();
        assert!(err.message.contains("没有可搜寻的目录"), "{}", err.message);
    }

    #[test]
    fn push_unique_keeps_first_occurrence() {
        let mut dirs = vec![PathBuf::from("a")];
        push_unique(&mut dirs, PathBuf::from("a"));
        push_unique(&mut dirs, PathBuf::from("b"));
        assert_eq!(dirs, vec![PathBuf::from("a"), PathBuf::from("b")]);
    }

    // 环境变量是进程级状态：本用例是唯一读写 `GUI_ENV` 的测试，避免并行串扰。
    #[test]
    fn run_reports_spawn_failure() {
        let temp = rscmd_runtime::fs::temp_dir().expect("临时目录可用");
        let stub = temp.path().join("stub-reshackw");
        std::fs::write(&stub, b"not an executable").unwrap();
        std::env::set_var(GUI_ENV, &stub);
        let result = run_cli(&mut ToolCtx::default(), &matches(&["reshackw"]));
        std::env::remove_var(GUI_ENV);
        let err = result.expect_err("非可执行文件应启动失败");
        assert_eq!(err.code, "IO");
        assert!(err.message.contains("stub-reshackw"), "{}", err.message);
    }

    #[test]
    fn meta_marks_tool_as_gui_only() {
        assert!(META.gui);
        assert_eq!(META.python, PythonExposure::Hidden);
        assert!(def().run_json.is_none());
        assert!(def().schema.is_some());
    }
}
