//! `reshackw`：拉起 `reshack` 的 GUI 前端（`rscmd-reshackw`）。
//!
//! GUI 是 `rscmd-gui` 提供的独立二进制，依赖 `eframe` / `image` / `arboard`，
//! 只在 `--features gui` 时编译。本工具只负责「定位 + 启动」，因此默认构建
//! （CLI / PyO3 / wheel / MCP）不会带上 GUI 依赖，也不会改变既有信封与退出码。
//!
//! 定位顺序：
//! 1. 环境变量 `RSCMD_RESHACKW`（显式指定优先，不存在则直接报错，不静默回退）；
//! 2. `rscmd` 可执行文件同目录下的 `rscmd-reshackw[.exe]`（`cargo build` 后
//!    CLI 与 GUI 共用同一个 `target/<profile>/` 目录）。
//!
//! 启动后立即返回，不等待窗口关闭；成功时返回 `gui` / `pid` / `file`。

use std::path::{Path, PathBuf};
use std::process::Command;

use clap::{ArgMatches, Command as ClapCommand};
use rscmd_core::{
    command_for, PythonExposure, RscmdError, SideEffects, ToolCtx, ToolDef, ToolMeta,
};
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
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    let binary = resolve_gui(env_override, exe_dir.as_deref())?;

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

/// 解析 GUI 二进制路径。
///
/// `env_override` 非空时**只**认该路径：显式指定的文件不存在就报错，
/// 避免用户以为用了自定义二进制、实际却跑同目录的旧产物。
fn resolve_gui(
    env_override: Option<PathBuf>,
    exe_dir: Option<&Path>,
) -> Result<PathBuf, RscmdError> {
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

    let Some(dir) = exe_dir else {
        return Err(unavailable("无法确定 rscmd 可执行文件所在目录".to_string()));
    };
    for name in GUI_BINARY_NAMES {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    let tried = GUI_BINARY_NAMES
        .iter()
        .map(|name| dir.join(name).display().to_string())
        .collect::<Vec<_>>()
        .join("、");
    Err(unavailable(format!("未找到 GUI 二进制（已尝试 {tried}）")))
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
        let resolved = resolve_gui(Some(fake.clone()), None).expect("应优先采用环境变量");
        assert_eq!(resolved, fake);
    }

    #[test]
    fn env_override_missing_is_an_error_not_a_fallback() {
        let temp = rscmd_runtime::fs::temp_dir().expect("临时目录可用");
        let missing = temp.path().join("nope-reshackw");
        let err = resolve_gui(Some(missing), Some(temp.path())).unwrap_err();
        assert_eq!(err.code, "GUI_UNAVAILABLE");
        assert!(err.hint.unwrap().contains(GUI_ENV));
    }

    #[test]
    fn falls_back_to_sibling_binary() {
        let temp = rscmd_runtime::fs::temp_dir().expect("临时目录可用");
        let sibling = temp.path().join(GUI_BINARY_NAMES[0]);
        std::fs::write(&sibling, b"stub").unwrap();
        let resolved = resolve_gui(None, Some(temp.path())).expect("应找到同目录二进制");
        assert_eq!(resolved, sibling);
    }

    #[test]
    fn missing_binary_reports_hint() {
        let temp = rscmd_runtime::fs::temp_dir().expect("临时目录可用");
        let err = resolve_gui(None, Some(temp.path())).unwrap_err();
        assert_eq!(err.code, "GUI_UNAVAILABLE");
        assert!(err.message.contains("未找到 GUI 二进制"), "{}", err.message);
        assert!(err.hint.unwrap().contains("--features gui"));
    }

    #[test]
    fn meta_marks_tool_as_gui_only() {
        assert!(META.gui);
        assert_eq!(META.python, PythonExposure::Hidden);
        assert!(def().run_json.is_none());
        assert!(def().schema.is_some());
    }
}
