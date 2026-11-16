//! 内建命令（`version` / `doctor`）的数据构造与人读渲染。
//!
//! 内建命令不进入工具注册表，由 [`crate::dispatch`] 直接分发。
//! 数据与渲染分离：同一份 `data` 既进 JSON 信封，也用于人读输出，
//! 避免两条路径各写一套逻辑而漂移。

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::{json, Value};

use crate::registry::BUILTIN_COMMANDS;
use crate::tool::ToolDef;

/// 单项自检结果。
pub struct Check {
    pub item: &'static str,
    pub ok: bool,
    pub detail: String,
    pub fix: String,
}

/// `_native` 扩展与 Rust 源码的新鲜度结论。
///
/// 抽成可注入的值：真实探测依赖当前工作目录与文件 mtime，单测直接构造结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeFreshness {
    /// 当前不在源码树中（安装态），跳过该检查。
    NotInSourceTree,
    /// 源码树中未找到已构建的 `_native` 扩展。
    Missing,
    /// 扩展旧于最新 Rust 源码。
    Stale,
    /// 扩展不旧于源码。
    Fresh,
}

impl NativeFreshness {
    /// 依据「扩展 mtime」与「最新源码 mtime」给出结论。
    pub fn from_mtimes(native: Option<SystemTime>, newest_source: Option<SystemTime>) -> Self {
        match (native, newest_source) {
            (_, None) => Self::NotInSourceTree,
            (None, Some(_)) => Self::Missing,
            (Some(native), Some(source)) if native < source => Self::Stale,
            (Some(_), Some(_)) => Self::Fresh,
        }
    }

    /// 从当前可执行文件与工作目录逐级向上找到源码树，再比较 mtime。
    pub fn detect() -> Self {
        Self::detect_from(&probe_starts())
    }

    /// 按给定起点探测，便于单测固定「不在源码树」分支。
    fn detect_from(starts: &[PathBuf]) -> Self {
        let Some(root) = find_source_root(starts) else {
            return Self::NotInSourceTree;
        };
        let native = newest_mtime(&root.join("python").join("rscmd"), &is_native_artifact);
        let source = newest_mtime(&root.join("crates"), &has_rust_extension);
        Self::from_mtimes(native, source)
    }
}

/// `doctor` 依赖的外部环境结论；字段可注入，单测无需依赖真实机器状态。
#[derive(Debug, Clone)]
pub struct DoctorProbe {
    pub native: NativeFreshness,
    pub code_page: Option<u32>,
    /// 当前工作目录；`None` 表示取不到。
    pub cwd: Option<PathBuf>,
}

impl DoctorProbe {
    /// 真实探测：源码树新鲜度、控制台代码页与工作目录。
    pub fn detect() -> Self {
        Self {
            native: NativeFreshness::detect(),
            code_page: detect_console_code_page(),
            cwd: std::env::current_dir().ok(),
        }
    }
}

/// `version` 命令的数据。
pub fn version_data() -> Value {
    json!({
        "name": "rscmd",
        "version": env!("CARGO_PKG_VERSION"),
    })
}

/// `version` 命令的人读输出（单行）。
pub fn version_text(data: &Value) -> String {
    let version = data
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    format!("rscmd {version}")
}

/// 收集 `doctor` 的全部检查项；环境结论由 `probe` 给出，便于单测覆盖各分支。
pub fn doctor_checks_with(
    tools: &[ToolDef],
    validation: Result<(), Vec<String>>,
    probe: &DoctorProbe,
) -> Vec<Check> {
    let mut checks = Vec::with_capacity(5);

    let (registry_ok, registry_detail, registry_fix) = match validation {
        Ok(()) => (true, format!("{} 个工具已注册", tools.len()), String::new()),
        Err(problems) => (
            false,
            format!("{} 个问题", problems.len()),
            problems.join("；"),
        ),
    };
    checks.push(Check {
        item: "工具注册表",
        ok: registry_ok,
        detail: registry_detail,
        fix: registry_fix,
    });

    let has_tools = !tools.is_empty();
    checks.push(Check {
        item: "工具数量",
        ok: has_tools,
        detail: format!("{} 个", tools.len()),
        fix: if has_tools {
            String::new()
        } else {
            "至少启用一个工具域".to_string()
        },
    });

    let cwd = probe.cwd.clone();
    let cwd_ok = cwd.is_some();
    let cwd_detail = cwd
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    let cwd_fix = if cwd_ok {
        ""
    } else {
        "切换到仍然存在的目录后重试"
    };
    checks.push(Check {
        item: "工作目录",
        ok: cwd_ok,
        detail: cwd_detail,
        fix: cwd_fix.to_string(),
    });

    let (native_ok, native_detail, native_fix) = match probe.native {
        NativeFreshness::Fresh => (true, "扩展不旧于最新 Rust 源码".to_string(), String::new()),
        NativeFreshness::Stale => (
            false,
            "扩展旧于最新 Rust 源码".to_string(),
            "运行 `maturin develop` 重建 `_native`".to_string(),
        ),
        NativeFreshness::Missing => (
            true,
            "源码树中未构建 `_native`（CLI 可独立使用）".to_string(),
            String::new(),
        ),
        NativeFreshness::NotInSourceTree => (
            true,
            "未在源码树中运行，跳过该检查".to_string(),
            String::new(),
        ),
    };
    checks.push(Check {
        item: "Python 扩展",
        ok: native_ok,
        detail: native_detail,
        fix: native_fix,
    });

    let (terminal_ok, terminal_detail, terminal_fix) = terminal_encoding(probe.code_page);
    checks.push(Check {
        item: "终端编码",
        ok: terminal_ok,
        detail: terminal_detail,
        fix: terminal_fix,
    });

    checks
}

/// 终端编码检查结论：(是否通过, 说明, 修复建议)。
///
/// 只在能拿到控制台代码页时判定：输出被重定向（测试、CI、管道）时一律跳过，
/// 避免把「没有控制台」误报成编码问题。
pub fn terminal_encoding(code_page: Option<u32>) -> (bool, String, String) {
    match code_page {
        None => (
            true,
            "未检测到控制台代码页（非 Windows 或输出已重定向），按 Unicode 输出".to_string(),
            String::new(),
        ),
        Some(65001) => (true, "代码页 65001（UTF-8）".to_string(), String::new()),
        Some(page) => (
            false,
            format!("代码页 {page}（非 UTF-8），中文可能显示为乱码"),
            "chcp 65001 或改用 Windows Terminal".to_string(),
        ),
    }
}

/// 读取控制台输出代码页；非 Windows、非终端或取不到时返回 `None`。
fn detect_console_code_page() -> Option<u32> {
    #[cfg(windows)]
    {
        use std::io::IsTerminal;

        if !std::io::stdout().is_terminal() {
            return None;
        }
        let output = std::process::Command::new("chcp").output().ok()?;
        parse_code_page(&String::from_utf8_lossy(&output.stdout))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// 从 `chcp` 输出里取代码页数字（中英文界面的前缀不同，只认最后一个数字段）。
///
/// 仅 Windows 的探测路径与单测会用到；其他平台显式收窄 cfg，避免 `-D dead-code` 报警。
#[cfg(any(windows, test))]
fn parse_code_page(text: &str) -> Option<u32> {
    text.split(|c: char| !c.is_ascii_digit())
        .rfind(|piece| !piece.is_empty())?
        .parse()
        .ok()
}

/// 探测起点：当前可执行文件所在目录链与工作目录。
fn probe_starts() -> Vec<PathBuf> {
    let mut starts = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        starts.push(exe);
    }
    if let Ok(cwd) = std::env::current_dir() {
        starts.push(cwd);
    }
    starts
}

/// 从起点逐级向上寻找同时含 `crates/rscmd-core` 与 `python/rscmd` 的源码树根。
fn find_source_root(starts: &[PathBuf]) -> Option<PathBuf> {
    for start in starts {
        for dir in start.ancestors().take(6) {
            if dir.join("crates").join("rscmd-core").is_dir()
                && dir.join("python").join("rscmd").is_dir()
            {
                return Some(dir.to_path_buf());
            }
        }
    }
    None
}

/// 递归找出目录下满足条件文件的最新修改时间；目录不存在或没有命中时返回 `None`。
fn newest_mtime(dir: &Path, accept: &dyn Fn(&Path) -> bool) -> Option<SystemTime> {
    if !dir.is_dir() {
        return None;
    }
    let mut newest: Option<SystemTime> = None;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if !accept(&path) {
                continue;
            }
            if let Ok(modified) = entry.metadata().and_then(|meta| meta.modified()) {
                newest = Some(newest.map_or(modified, |current| current.max(modified)));
            }
        }
    }
    newest
}

fn has_rust_extension(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("rs")
}

/// 已构建的 Python 扩展产物：`_native.pyd` / `_native.so` / `_native.dylib`。
fn is_native_artifact(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    name.starts_with("_native")
        && matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("pyd") | Some("so") | Some("dylib")
        )
}

/// `doctor` 命令的数据：检查项 + 汇总 + 运行环境（环境相关结论走真实探测）。
pub fn doctor_data(tools: &[ToolDef], validation: Result<(), Vec<String>>) -> Value {
    doctor_data_with(tools, validation, &DoctorProbe::detect())
}

/// `doctor` 命令的数据；环境结论由 `probe` 给出，便于单测固定分支。
pub fn doctor_data_with(
    tools: &[ToolDef],
    validation: Result<(), Vec<String>>,
    probe: &DoctorProbe,
) -> Value {
    let checks = doctor_checks_with(tools, validation, probe);
    let passed = checks.iter().filter(|check| check.ok).count();
    let total = checks.len();

    json!({
        "ok": passed == total,
        "passed": passed,
        "failed": total - passed,
        "total": total,
        "version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "tool_count": tools.len(),
        "builtins": BUILTIN_COMMANDS,
        "checks": checks.iter().map(check_json).collect::<Vec<_>>(),
    })
}

/// 该结果是否为「检查不通过」（结论性失败，不是运行错误）。
pub fn check_failed(tool: &str, data: &Value) -> bool {
    tool == "doctor" && data.get("ok").and_then(Value::as_bool) == Some(false)
}

/// `doctor` 命令的人读输出。
pub fn doctor_text(data: &Value) -> String {
    let version = text_or(data, "version", "unknown");
    let os = text_or(data, "os", "unknown");
    let arch = text_or(data, "arch", "unknown");
    let tool_count = number_or(data, "tool_count");
    let builtins = data
        .get("builtins")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();

    let mut out = format!(
        "rscmd 环境自检 ({version})\n平台: {os}/{arch}\n工具: {tool_count} 个；内建命令: {builtins}\n"
    );

    if let Some(checks) = data.get("checks").and_then(Value::as_array) {
        out.push('\n');
        for check in checks {
            let item = text_or(check, "item", "?");
            let ok = check.get("ok").and_then(Value::as_bool).unwrap_or(false);
            let detail = text_or(check, "detail", "");
            let fix = text_or(check, "fix", "");
            let status = if ok { "OK  " } else { "FAIL" };
            out.push_str(&format!("[{status}] {item}: {detail}\n"));
            if !ok && !fix.is_empty() {
                out.push_str(&format!("       修复: {fix}\n"));
            }
        }
    }

    let passed = number_or(data, "passed");
    let total = number_or(data, "total");
    let failed = number_or(data, "failed");
    if failed == 0 {
        out.push_str(&format!("\n诊断结果: {passed}/{total} 全部通过\n"));
    } else {
        out.push_str(&format!(
            "\n诊断结果: {passed}/{total} 通过，{failed} 项失败\n"
        ));
        out.push_str("提示: 按上表每项的「修复」建议处理后，重新运行 rscmd doctor\n");
    }
    out
}

fn text_or<'a>(data: &'a Value, key: &str, default: &'a str) -> &'a str {
    data.get(key).and_then(Value::as_str).unwrap_or(default)
}

fn number_or(data: &Value, key: &str) -> u64 {
    data.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn check_json(check: &Check) -> Value {
    json!({
        "item": check.item,
        "ok": check.ok,
        "detail": check.detail,
        "fix": check.fix,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn temp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("rscmd-doctor-{tag}-{}", std::process::id()))
    }

    /// 固定的理想环境：扩展新鲜、UTF-8 控制台、工作目录可取到。
    fn nominal(native: NativeFreshness) -> DoctorProbe {
        DoctorProbe {
            native,
            code_page: Some(65001),
            cwd: Some(PathBuf::from(".")),
        }
    }

    #[test]
    fn native_freshness_classifies_mtimes() {
        assert_eq!(
            NativeFreshness::from_mtimes(None, None),
            NativeFreshness::NotInSourceTree
        );
        assert_eq!(
            NativeFreshness::from_mtimes(Some(at(5)), None),
            NativeFreshness::NotInSourceTree
        );
        assert_eq!(
            NativeFreshness::from_mtimes(None, Some(at(5))),
            NativeFreshness::Missing
        );
        assert_eq!(
            NativeFreshness::from_mtimes(Some(at(5)), Some(at(6))),
            NativeFreshness::Stale
        );
        assert_eq!(
            NativeFreshness::from_mtimes(Some(at(6)), Some(at(6))),
            NativeFreshness::Fresh
        );
    }

    #[test]
    fn terminal_encoding_flags_non_utf8_code_page() {
        assert!(terminal_encoding(None).0);
        assert!(terminal_encoding(Some(65001)).0);

        let (ok, detail, fix) = terminal_encoding(Some(936));
        assert!(!ok);
        assert!(detail.contains("936"), "{detail}");
        assert!(fix.contains("chcp 65001"), "{fix}");
    }

    #[test]
    fn parses_localized_code_page_output() {
        assert_eq!(parse_code_page("Active code page: 65001\r\n"), Some(65001));
        assert_eq!(parse_code_page("活动代码页: 936\n"), Some(936));
        assert_eq!(parse_code_page("no digits here"), None);
    }

    #[test]
    fn doctor_reports_stale_native_extension_as_failure() {
        let checks = doctor_checks_with(&[], Ok(()), &nominal(NativeFreshness::Stale));
        assert_eq!(checks.len(), 5);
        let native = checks
            .iter()
            .find(|check| check.item == "Python 扩展")
            .expect("应包含 Python 扩展检查");
        assert!(!native.ok);
        assert!(native.fix.contains("maturin develop"), "{}", native.fix);
        assert!(
            checks
                .iter()
                .find(|check| check.item == "终端编码")
                .expect("应包含终端编码检查")
                .ok
        );
    }

    #[test]
    fn doctor_skips_native_check_outside_source_tree() {
        let mut probe = nominal(NativeFreshness::NotInSourceTree);
        probe.code_page = None;
        let checks = doctor_checks_with(&[], Ok(()), &probe);
        let native = checks
            .iter()
            .find(|check| check.item == "Python 扩展")
            .expect("应包含 Python 扩展检查");
        assert!(native.ok);
        assert!(native.detail.contains("跳过"), "{}", native.detail);
        assert!(
            checks
                .iter()
                .find(|check| check.item == "终端编码")
                .expect("应包含终端编码检查")
                .ok
        );
    }

    #[test]
    fn doctor_notes_missing_native_extension_as_ok() {
        let checks = doctor_checks_with(&[], Ok(()), &nominal(NativeFreshness::Missing));
        let native = checks
            .iter()
            .find(|check| check.item == "Python 扩展")
            .expect("应包含 Python 扩展检查");
        assert!(native.ok);
        assert!(native.detail.contains("未构建"), "{}", native.detail);
    }

    #[test]
    fn doctor_flags_unavailable_working_directory() {
        let mut probe = nominal(NativeFreshness::Fresh);
        probe.cwd = None;
        let checks = doctor_checks_with(&[], Ok(()), &probe);
        let cwd = checks
            .iter()
            .find(|check| check.item == "工作目录")
            .expect("应包含工作目录检查");
        assert!(!cwd.ok);
        assert!(cwd.fix.contains("切换"), "{}", cwd.fix);
    }

    #[test]
    fn detects_outside_source_tree_without_candidates() {
        assert_eq!(
            NativeFreshness::detect_from(&[]),
            NativeFreshness::NotInSourceTree
        );
    }

    #[test]
    fn doctor_summary_counts_failed_checks() {
        let data = doctor_data(&[], Ok(()));
        // 空工具表会让「工具数量」失败；只要没有出现意外失败项即可。
        let checks = data["checks"].as_array().expect("checks 应为数组");
        assert!(checks.iter().any(|check| check["item"] == "Python 扩展"));
        assert!(checks.iter().any(|check| check["item"] == "终端编码"));
        assert_eq!(data["total"], checks.len());
    }

    #[test]
    fn finds_source_root_only_with_expected_layout() {
        let base = temp_dir("root");
        std::fs::create_dir_all(base.join("crates").join("rscmd-core")).unwrap();
        std::fs::create_dir_all(base.join("python").join("rscmd")).unwrap();
        let nested = base.join("target").join("debug");
        std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(
            find_source_root(&[nested.join("rscmd.exe")]),
            Some(base.clone())
        );

        let empty = temp_dir("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert_eq!(find_source_root(&[empty.join("x")]), None);

        std::fs::remove_dir_all(base).ok();
        std::fs::remove_dir_all(empty).ok();
    }

    #[test]
    fn newest_mtime_only_considers_accepted_files() {
        let dir = temp_dir("mtime");
        std::fs::create_dir_all(dir.join("nested")).unwrap();
        let rust = dir.join("a.rs");
        let later_rust = dir.join("nested").join("c.rs");
        let other = dir.join("b.txt");
        std::fs::write(&rust, "x").unwrap();
        std::fs::write(&later_rust, "x").unwrap();
        std::fs::write(&other, "x").unwrap();

        // 把不计入的 .txt 设为更晚，验证过滤条件生效。
        let file = std::fs::OpenOptions::new()
            .append(true)
            .open(&other)
            .unwrap();
        file.set_modified(at(2_000_000_000)).unwrap();

        // 两个 .rs 的 mtime 显式拉开，验证递归与「取最新」分支。
        std::fs::OpenOptions::new()
            .append(true)
            .open(&rust)
            .unwrap()
            .set_modified(at(1_000_000_000))
            .unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(&later_rust)
            .unwrap()
            .set_modified(at(1_500_000_000))
            .unwrap();

        let found = newest_mtime(&dir, &has_rust_extension).expect("应找到 .rs");
        assert_eq!(found, at(1_500_000_000));
        assert!(found < at(2_000_000_000));
        assert!(
            newest_mtime(&temp_dir("mtime-missing"), &has_rust_extension).is_none(),
            "目录不存在时应返回 None"
        );
        assert!(is_native_artifact(Path::new("_native.cp311-win_amd64.pyd")));
        assert!(!is_native_artifact(Path::new("native.pyd")));

        std::fs::remove_dir_all(dir).ok();
    }
}
