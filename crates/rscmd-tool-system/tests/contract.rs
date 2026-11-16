use rscmd_core::ToolCtx;
use serde_json::json;

#[test]
fn path_normalize_unifies_separators() {
    let def = rscmd_tool_system::path::def();
    let run = def.run_json.expect("path 应暴露 JSON 入口");
    let mut ctx = ToolCtx::default();
    let out = run(&mut ctx, &json!({"op": "normalize", "path": "a\\b\\..\\c"})).unwrap();
    assert_eq!(out["path"], "a/c");
}

#[test]
fn path_relative_uses_forward_slashes() {
    let def = rscmd_tool_system::path::def();
    let run = def.run_json.expect("path 应暴露 JSON 入口");
    let mut ctx = ToolCtx::default();
    let out = run(
        &mut ctx,
        &json!({"op": "relative", "path": "C:/a/b", "base": "C:/a"}),
    )
    .unwrap();
    assert_eq!(out["relative"], "b");
}

#[test]
fn path_is_abs_is_platform_independent() {
    let def = rscmd_tool_system::path::def();
    let run = def.run_json.expect("path 应暴露 JSON 入口");
    let mut ctx = ToolCtx::default();
    assert_eq!(
        run(&mut ctx, &json!({"op": "is-abs", "path": "/x"})).unwrap()["absolute"],
        true
    );
    assert_eq!(
        run(&mut ctx, &json!({"op": "is-abs", "path": "x/y"})).unwrap()["absolute"],
        false
    );
}

#[test]
fn path_rejects_unknown_op() {
    let def = rscmd_tool_system::path::def();
    let run = def.run_json.expect("path 应暴露 JSON 入口");
    let mut ctx = ToolCtx::default();
    let err = run(&mut ctx, &json!({"op": "resolve", "path": "a"})).unwrap_err();
    assert_eq!(err.code, "INVALID_MODE");
    assert_eq!(err.exit_code(), 3);
    assert_eq!(err.field.as_deref(), Some("op"));
}

#[test]
fn which_returns_null_for_missing_program() {
    let def = rscmd_tool_system::which::def();
    let run = def.run_json.expect("which 应暴露 JSON 入口");
    let mut ctx = ToolCtx::default();
    let out = run(
        &mut ctx,
        &json!({"program": "rscmd-definitely-missing-xyz"}),
    )
    .unwrap();
    assert!(out["path"].is_null());
    assert_eq!(out["program"], "rscmd-definitely-missing-xyz");
}

#[test]
fn which_rejects_empty_program() {
    let def = rscmd_tool_system::which::def();
    let run = def.run_json.expect("which 应暴露 JSON 入口");
    let mut ctx = ToolCtx::default();
    let err = run(&mut ctx, &json!({"program": ""})).unwrap_err();
    assert_eq!(err.code, "INVALID_ARGS");
    assert_eq!(err.exit_code(), 3);
}
