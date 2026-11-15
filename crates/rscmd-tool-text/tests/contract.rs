use rscmd_core::ToolCtx;
use serde_json::json;

#[test]
fn case_json_entry_returns_converted_text() {
    let def = rscmd_tool_text::case::def();
    let run = def.run_json.expect("case 应暴露 JSON 入口");
    let mut ctx = ToolCtx::default();
    let out = run(&mut ctx, &json!({"text": "HelloWorld", "mode": "snake"})).unwrap();
    assert_eq!(out["text"], "hello_world");
}

#[test]
fn case_rejects_bad_mode() {
    let def = rscmd_tool_text::case::def();
    let run = def.run_json.expect("case 应暴露 JSON 入口");
    let mut ctx = ToolCtx::default();
    let err = run(&mut ctx, &json!({"text": "HelloWorld", "mode": "dot"})).unwrap_err();
    assert_eq!(err.code, "INVALID_MODE");
    assert_eq!(err.exit_code(), 3);
    assert_eq!(err.field.as_deref(), Some("mode"));
}

#[test]
fn wc_reports_structured_stats() {
    let def = rscmd_tool_text::wc::def();
    let run = def.run_json.expect("wc 应暴露 JSON 入口");
    let mut ctx = ToolCtx::default();
    let out = run(&mut ctx, &json!({"text": "a b\nc"})).unwrap();
    assert_eq!(out["lines"], 2);
    assert_eq!(out["words"], 3);
    assert_eq!(out["chars"], 5);
}
