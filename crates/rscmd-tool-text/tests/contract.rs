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

#[test]
fn regex_json_entry_reports_matches_and_offsets() {
    let def = rscmd_tool_text::regex::def();
    let run = def.run_json.expect("regex 应暴露 JSON 入口");
    let mut ctx = ToolCtx::default();
    let out = run(&mut ctx, &json!({"pattern": "(\\w)(\\d)", "text": "a1 b2"})).unwrap();
    assert_eq!(out["count"], 2);
    assert_eq!(out["matches"][0]["text"], "a1");
    assert_eq!(out["matches"][0]["start"], 0);
    assert_eq!(out["matches"][0]["groups"], json!(["a", "1"]));
}

#[test]
fn regex_json_entry_replaces_with_groups() {
    let def = rscmd_tool_text::regex::def();
    let run = def.run_json.expect("regex 应暴露 JSON 入口");
    let mut ctx = ToolCtx::default();
    let out = run(
        &mut ctx,
        &json!({"pattern": "(\\w+)@(\\w+)", "text": "a@b", "replace": "$2:$1"}),
    )
    .unwrap();
    assert_eq!(out["text"], "b:a");
}

#[test]
fn regex_rejects_invalid_pattern() {
    let def = rscmd_tool_text::regex::def();
    let run = def.run_json.expect("regex 应暴露 JSON 入口");
    let mut ctx = ToolCtx::default();
    let err = run(&mut ctx, &json!({"pattern": "(", "text": "abc"})).unwrap_err();
    assert_eq!(err.code, "INVALID_REGEX");
    assert_eq!(err.exit_code(), 3);
    assert_eq!(err.field.as_deref(), Some("pattern"));
}
