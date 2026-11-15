//! 统一呈现：人读文本与 JSON 信封共用同一份结果对象。

use serde_json::Value;

use crate::builtins;
use crate::dispatch::Outcome;
use crate::envelope;
use crate::errors::RscmdError;

/// 返回 `(stdout, stderr)`。
pub fn render(outcome: &Outcome) -> (String, String) {
    match &outcome.result {
        Ok(data) => {
            if outcome.json {
                let ok = !builtins::check_failed(&outcome.tool, data);
                let env = envelope::result(&outcome.tool, data.clone(), ok);
                (format!("{}\n", compact(&env)), String::new())
            } else if outcome.quiet {
                (String::new(), String::new())
            } else {
                (human_success(&outcome.tool, data), String::new())
            }
        }
        Err(err) => {
            if outcome.json {
                let env = envelope::error(&outcome.tool, err);
                (String::new(), format!("{}\n", compact(&env)))
            } else {
                (String::new(), human_error(err))
            }
        }
    }
}

fn compact(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{\"ok\":false}".to_string())
}

fn human_success(tool: &str, data: &Value) -> String {
    // 内建命令有专门的人读形态，其余沿用 text / json / pretty 回退。
    match tool {
        "version" => return format!("{}\n", builtins::version_text(data)),
        "doctor" => return builtins::doctor_text(data),
        _ => {}
    }
    if let Some(object) = data.as_object() {
        if let Some(text) = object.get("text").and_then(Value::as_str) {
            return format!("{text}\n");
        }
        if let Some(text) = object.get("json").and_then(Value::as_str) {
            return format!("{text}\n");
        }
    }
    match serde_json::to_string_pretty(data) {
        Ok(text) => format!("{text}\n"),
        Err(_) => String::new(),
    }
}

fn human_error(err: &RscmdError) -> String {
    let mut out = format!("错误[{}]: {}\n", err.code, err.message);
    if let Some(field) = &err.field {
        out.push_str(&format!("  字段: {field}\n"));
    }
    if let Some(expected) = &err.expected {
        out.push_str(&format!("  期望: {expected}\n"));
    }
    if let Some(hint) = &err.hint {
        out.push_str(&format!("  提示: {hint}\n"));
    }
    out
}
