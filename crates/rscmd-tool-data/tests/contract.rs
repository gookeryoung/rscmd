use rscmd_core::ToolCtx;
use serde_json::json;

#[test]
fn formats_valid_json() {
    let def = rscmd_tool_data::json_fmt::def();
    let run = def.run_json.expect("json_fmt 应暴露 JSON 入口");
    let mut ctx = ToolCtx::default();
    let out = run(&mut ctx, &json!({"json": "{\"a\":1}"})).unwrap();
    assert!(out["json"].as_str().unwrap().contains("\"a\""));
}

#[test]
fn invalid_json_is_invalid_input() {
    let def = rscmd_tool_data::json_fmt::def();
    let run = def.run_json.expect("json_fmt 应暴露 JSON 入口");
    let mut ctx = ToolCtx::default();
    let err = run(&mut ctx, &json!({"json": "{"})).unwrap_err();
    assert_eq!(err.code, "INVALID_JSON");
    assert_eq!(err.exit_code(), 3);
    assert_eq!(err.field.as_deref(), Some("json"));
}
