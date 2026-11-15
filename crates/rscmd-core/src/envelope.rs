//! 成功 / 错误 / 自描述三种 JSON 信封。

use serde_json::{json, Map, Value};

use crate::errors::RscmdError;

pub const RESULT_SCHEMA: &str = "rscmd-result/1";
pub const ERROR_SCHEMA: &str = "rscmd-error/1";

pub fn success(tool: &str, data: Value) -> Value {
    json!({
        "schema": RESULT_SCHEMA,
        "ok": true,
        "tool": tool,
        "data": data,
    })
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

    #[test]
    fn success_envelope_has_schema_and_data() {
        let env = success("case", json!({"text": "hello_world"}));
        assert_eq!(env["schema"], RESULT_SCHEMA);
        assert_eq!(env["ok"], true);
        assert_eq!(env["tool"], "case");
        assert_eq!(env["data"]["text"], "hello_world");
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
