//! 内建命令（`version` / `doctor`）的数据构造与人读渲染。
//!
//! 内建命令不进入工具注册表，由 [`crate::dispatch`] 直接分发。
//! 数据与渲染分离：同一份 `data` 既进 JSON 信封，也用于人读输出，
//! 避免两条路径各写一套逻辑而漂移。

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

/// 收集 `doctor` 的全部检查项。
///
/// `validation` 由调用方传入注册表校验结果，便于测试注入失败分支。
pub fn doctor_checks(tools: &[ToolDef], validation: Result<(), Vec<String>>) -> Vec<Check> {
    let mut checks = Vec::with_capacity(3);

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

    let cwd = std::env::current_dir();
    let cwd_ok = cwd.is_ok();
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

    checks
}

/// `doctor` 命令的数据：检查项 + 汇总 + 运行环境。
pub fn doctor_data(tools: &[ToolDef], validation: Result<(), Vec<String>>) -> Value {
    let checks = doctor_checks(tools, validation);
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
