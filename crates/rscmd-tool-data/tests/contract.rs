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

#[test]
fn stat_summarizes_values() {
    let def = rscmd_tool_data::stat::def();
    let run = def.run_json.expect("stat 应暴露 JSON 入口");
    let mut ctx = ToolCtx::default();
    let out = run(
        &mut ctx,
        &json!({"values": [1, 2, 3, 4], "percentiles": [25, 90]}),
    )
    .unwrap();
    assert_eq!(out["count"], 4);
    assert_eq!(out["sum"], 10.0);
    assert_eq!(out["median"], 2.5);
    assert_eq!(out["percentiles"][0]["value"], 1.75);
}

#[test]
fn stat_rejects_out_of_range_percentile() {
    let def = rscmd_tool_data::stat::def();
    let run = def.run_json.expect("stat 应暴露 JSON 入口");
    let mut ctx = ToolCtx::default();
    let err = run(&mut ctx, &json!({"values": [1, 2], "percentiles": [-1]})).unwrap_err();
    assert_eq!(err.code, "INVALID_ARGS");
    assert_eq!(err.exit_code(), 3);
    assert_eq!(err.field.as_deref(), Some("percentiles"));
}
