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
fn csv_converts_csv_to_json() {
    let def = rscmd_tool_data::csv::def();
    let run = def.run_json.expect("csv 应暴露 JSON 入口");
    let mut ctx = ToolCtx::default();
    let out = run(&mut ctx, &json!({"text": "a,b\n1,2\n"})).unwrap();
    assert_eq!(out["has_header"], true);
    assert_eq!(out["rows"][0]["a"], "1");
}

#[test]
fn csv_converts_json_to_csv() {
    let def = rscmd_tool_data::csv::def();
    let run = def.run_json.expect("csv 应暴露 JSON 入口");
    let mut ctx = ToolCtx::default();
    let out = run(&mut ctx, &json!({"text": "[{\"a\":1}]", "to": "csv"})).unwrap();
    assert_eq!(
        out["csv"].as_str().unwrap().lines().collect::<Vec<_>>(),
        ["a", "1"]
    );
}

#[test]
fn csv_rejects_unknown_direction() {
    let def = rscmd_tool_data::csv::def();
    let run = def.run_json.expect("csv 应暴露 JSON 入口");
    let mut ctx = ToolCtx::default();
    let err = run(&mut ctx, &json!({"text": "a", "to": "xml"})).unwrap_err();
    assert_eq!(err.code, "INVALID_MODE");
    assert_eq!(err.exit_code(), 3);
    assert_eq!(err.field.as_deref(), Some("to"));
}

#[test]
fn timestamp_converts_unix_to_rfc3339() {
    let def = rscmd_tool_data::timestamp::def();
    let run = def.run_json.expect("timestamp 应暴露 JSON 入口");
    let mut ctx = ToolCtx::default();
    let out = run(&mut ctx, &json!({"input": "0", "offset": "+08:00"})).unwrap();
    assert_eq!(out["unix"], 0);
    assert_eq!(out["rfc3339"], "1970-01-01T08:00:00+08:00");
    assert_eq!(out["offset"], "+08:00");
}

#[test]
fn timestamp_rejects_unknown_offset() {
    let def = rscmd_tool_data::timestamp::def();
    let run = def.run_json.expect("timestamp 应暴露 JSON 入口");
    let mut ctx = ToolCtx::default();
    let err = run(&mut ctx, &json!({"input": "0", "offset": "+25:00"})).unwrap_err();
    assert_eq!(err.code, "INVALID_ARGS");
    assert_eq!(err.exit_code(), 3);
    assert_eq!(err.field.as_deref(), Some("offset"));
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

#[test]
fn toml_converts_file_to_json_and_yaml() {
    let dir = rscmd_runtime::fs::temp_dir().unwrap();
    let path = dir.path().join("sample.toml");
    std::fs::write(&path, "title = \"demo\"\n[db]\nport = 5432\n").unwrap();
    let display = path.to_string_lossy().into_owned();
    let def = rscmd_tool_data::toml::def();
    let run = def.run_json.expect("toml 应暴露 JSON 入口");

    let json_out = run(&mut ToolCtx::default(), &json!({"file": display})).unwrap();
    assert_eq!(json_out["to"], "json");
    let parsed: serde_json::Value =
        serde_json::from_str(json_out["text"].as_str().unwrap()).unwrap();
    assert_eq!(parsed["db"]["port"], 5432);

    let yaml_out = run(
        &mut ToolCtx::default(),
        &json!({"file": display, "to": "yaml"}),
    )
    .unwrap();
    assert!(yaml_out["text"].as_str().unwrap().contains("port: 5432"));
}

#[test]
fn toml_parse_failure_is_reported_with_path() {
    let dir = rscmd_runtime::fs::temp_dir().unwrap();
    let path = dir.path().join("bad.toml");
    std::fs::write(&path, "key = [1,\n").unwrap();
    let def = rscmd_tool_data::toml::def();
    let run = def.run_json.expect("toml 应暴露 JSON 入口");
    let err = run(
        &mut ToolCtx::default(),
        &json!({"file": path.to_string_lossy()}),
    )
    .unwrap_err();
    assert_eq!(err.code, "PARSE_FAILED");
    assert_eq!(err.exit_code(), 3);
    assert_eq!(err.field.as_deref(), Some("path"));
}

#[test]
fn yaml_converts_file_to_json_and_toml() {
    let dir = rscmd_runtime::fs::temp_dir().unwrap();
    let path = dir.path().join("sample.yaml");
    std::fs::write(&path, "title: demo\ndb:\n  port: 5432\n").unwrap();
    let display = path.to_string_lossy().into_owned();
    let def = rscmd_tool_data::yaml::def();
    let run = def.run_json.expect("yaml 应暴露 JSON 入口");

    let json_out = run(&mut ToolCtx::default(), &json!({"file": display})).unwrap();
    assert_eq!(json_out["to"], "json");
    let parsed: serde_json::Value =
        serde_json::from_str(json_out["text"].as_str().unwrap()).unwrap();
    assert_eq!(parsed["db"]["port"], 5432);

    let toml_out = run(
        &mut ToolCtx::default(),
        &json!({"file": display, "to": "toml"}),
    )
    .unwrap();
    let text = toml_out["text"].as_str().unwrap();
    assert!(text.contains("title = \"demo\""), "{text}");
    assert!(text.contains("port = 5432"), "{text}");
}

#[test]
fn yaml_duplicate_keys_are_parse_failed() {
    let dir = rscmd_runtime::fs::temp_dir().unwrap();
    let path = dir.path().join("dup.yaml");
    std::fs::write(&path, "a: 1\na: 2\n").unwrap();
    let def = rscmd_tool_data::yaml::def();
    let run = def.run_json.expect("yaml 应暴露 JSON 入口");
    let err = run(
        &mut ToolCtx::default(),
        &json!({"file": path.to_string_lossy()}),
    )
    .unwrap_err();
    assert_eq!(err.code, "PARSE_FAILED");
    assert_eq!(err.exit_code(), 3);
    assert_eq!(err.field.as_deref(), Some("path"));
}
