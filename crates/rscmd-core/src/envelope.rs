//! 成功 / 错误 / 自描述三种 JSON 信封。

use serde_json::{Map, Value};

use crate::errors::RscmdError;

pub const RESULT_SCHEMA: &str = "rscmd-result/1";
pub const ERROR_SCHEMA: &str = "rscmd-error/1";
/// `rscmd schema` 自描述输出的契约版本。
pub const SCHEMA_CONTRACT: &str = "rscmd-schema/1";
/// `rscmd tools` 的版本信封，位于结果信封的 `data` 内层。
pub const TOOLS_CONTRACT: &str = "rscmd-tools/1";

pub fn success(tool: &str, data: Value) -> Value {
    result(tool, None, data, true)
}

/// 结果信封；`ok=false` 表示「检查类命令的结论为不通过」（退出码 5）。
///
/// `subcommand` 只在工具确实带下级子命令时出现。没有该键时字段集合与
/// 1.x 消费方看到的完全一致，属兼容扩展；后续工具出现真实子命令才有值。
pub fn result(tool: &str, subcommand: Option<&str>, data: Value, ok: bool) -> Value {
    let mut obj = Map::new();
    obj.insert("schema".into(), Value::from(RESULT_SCHEMA));
    obj.insert("ok".into(), Value::from(ok));
    obj.insert("tool".into(), Value::from(tool));
    if let Some(subcommand) = subcommand.filter(|value| !value.is_empty()) {
        obj.insert("subcommand".into(), Value::from(subcommand));
    }
    obj.insert("data".into(), data);
    Value::Object(obj)
}

pub fn error(tool: &str, err: &RscmdError) -> Value {
    let mut obj = Map::new();
    obj.insert("schema".into(), Value::from(ERROR_SCHEMA));
    obj.insert("ok".into(), Value::from(false));
    obj.insert("tool".into(), Value::from(tool));
    obj.insert("kind".into(), Value::from(err.kind.as_str()));
    obj.insert("code".into(), Value::from(err.code.clone()));
    obj.insert("message".into(), Value::from(err.message.clone()));
    if let Some(field) = &err.field {
        obj.insert("field".into(), Value::from(field.clone()));
    }
    if let Some(expected) = &err.expected {
        obj.insert("expected".into(), Value::from(expected.clone()));
    }
    if let Some(hint) = &err.hint {
        obj.insert("hint".into(), Value::from(hint.clone()));
    }
    obj.insert("exit_code".into(), Value::from(err.exit_code()));
    Value::Object(obj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::ErrorKind;
    use serde_json::json;

    #[test]
    fn success_envelope_has_schema_and_data() {
        let env = success("case", json!({"text": "hello_world"}));
        assert_eq!(env["schema"], RESULT_SCHEMA);
        assert_eq!(env["ok"], true);
        assert_eq!(env["tool"], "case");
        assert_eq!(env["data"]["text"], "hello_world");
    }

    #[test]
    fn subcommand_is_additive_and_optional() {
        let plain = success("case", json!({"text": "x"}));
        assert!(
            plain.get("subcommand").is_none(),
            "无子命令时不得出现该键: {plain}"
        );

        let nested = result("git", Some("status"), json!({}), true);
        assert_eq!(nested["subcommand"], "status");
        assert_eq!(nested["schema"], RESULT_SCHEMA);

        // 空串等价于「没有子命令」，避免消费方拿到无意义字段。
        let empty = result("git", Some(""), json!({}), true);
        assert!(empty.get("subcommand").is_none());
    }

    #[test]
    fn error_envelope_maps_kind_and_exit_code() {
        let err = RscmdError::invalid_input("INVALID_JSON", "不是合法 JSON")
            .with_field("json")
            .with_hint("检查括号与引号");
        let env = error("json_fmt", &err);
        assert_eq!(env["schema"], ERROR_SCHEMA);
        assert_eq!(env["kind"], ErrorKind::InvalidInput.as_str());
        assert_eq!(env["exit_code"], 3);
        assert_eq!(env["field"], "json");
        assert_eq!(env["hint"], "检查括号与引号");
    }
}
