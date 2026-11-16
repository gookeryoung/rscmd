use rscmd_core::ToolCtx;
use rscmd_runtime::fs::temp_dir;
use serde_json::json;

#[test]
fn filesearch_lists_name_matches_relative_to_root() {
    let dir = temp_dir().unwrap();
    std::fs::create_dir_all(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("a.txt"), "needle").unwrap();
    std::fs::write(dir.path().join("sub/b.txt"), "other").unwrap();
    let def = rscmd_tool_system::filesearch::def();
    let run = def.run_json.expect("filesearch 应暴露 JSON 入口");
    let out = run(
        &mut ToolCtx::default(),
        &json!({"root": dir.path().to_string_lossy(), "name": "**/*.txt"}),
    )
    .unwrap();
    assert_eq!(out["matched"], 2);
    assert_eq!(out["results"][1]["path"], "sub/b.txt");
}

#[test]
fn filesearch_content_matches_lines() {
    let dir = temp_dir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "x\r\nneedle\r\n").unwrap();
    let def = rscmd_tool_system::filesearch::def();
    let run = def.run_json.expect("filesearch 应暴露 JSON 入口");
    let out = run(
        &mut ToolCtx::default(),
        &json!({"root": dir.path().to_string_lossy(), "content": "needle"}),
    )
    .unwrap();
    assert_eq!(out["results"][0]["line"], 2);
    assert_eq!(out["results"][0]["text"], "needle");
}

#[test]
fn filesearch_requires_a_filter() {
    let dir = temp_dir().unwrap();
    let def = rscmd_tool_system::filesearch::def();
    let run = def.run_json.expect("filesearch 应暴露 JSON 入口");
    let err = run(
        &mut ToolCtx::default(),
        &json!({"root": dir.path().to_string_lossy()}),
    )
    .unwrap_err();
    assert_eq!(err.code, "INVALID_ARGS");
    assert_eq!(err.exit_code(), 3);
}

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

#[test]
fn filedate_reads_and_writes_whole_second_mtime() {
    let dir = temp_dir().unwrap();
    let path = dir.path().join("stamp.txt");
    std::fs::write(&path, "x").unwrap();
    let display = path.to_string_lossy().into_owned();
    let def = rscmd_tool_system::filedate::def();
    let run = def.run_json.expect("filedate 应暴露 JSON 入口");

    let mut ctx = ToolCtx {
        confirm: true,
        ..ToolCtx::default()
    };
    let written = run(&mut ctx, &json!({"path": display, "set": 1700000000})).unwrap();
    assert_eq!(written["mtime_unix"], 1700000000);
    assert_eq!(written["mtime_ns"], 0);
    assert_eq!(written["rfc3339"], "2023-11-14T22:13:20Z");
    assert_eq!(written["dry_run"], json!(false));

    let read = run(&mut ToolCtx::default(), &json!({"path": display})).unwrap();
    assert_eq!(read["mtime_unix"], 1700000000);
    assert!(!read.as_object().unwrap().contains_key("dry_run"));
}

#[test]
fn filedate_write_without_confirm_is_rejected() {
    let dir = temp_dir().unwrap();
    let path = dir.path().join("stamp.txt");
    std::fs::write(&path, "x").unwrap();
    let def = rscmd_tool_system::filedate::def();
    let run = def.run_json.expect("filedate 应暴露 JSON 入口");
    let err = run(
        &mut ToolCtx::default(),
        &json!({"path": path.to_string_lossy(), "set": 1700000000}),
    )
    .unwrap_err();
    assert_eq!(err.code, "CONFIRM_REQUIRED");
    assert_eq!(err.exit_code(), 2);
}

#[test]
fn archive_writes_zip_with_relative_entries() {
    let dir = temp_dir().unwrap();
    std::fs::create_dir_all(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("a.txt"), "aa").unwrap();
    std::fs::write(dir.path().join("sub/b.txt"), "b").unwrap();
    let output = dir.path().join("out.zip");
    let def = rscmd_tool_system::archive::def();
    let matches = (def.build)()
        .try_get_matches_from([
            "archive",
            &dir.path().to_string_lossy(),
            "-o",
            &output.to_string_lossy(),
        ])
        .expect("参数应可解析");
    let mut ctx = ToolCtx {
        confirm: true,
        ..ToolCtx::default()
    };
    let out = (def.run)(&mut ctx, &matches).unwrap();
    assert_eq!(out["file_count"], 2);
    assert_eq!(out["total_bytes"], 3);
    assert_eq!(out["dry_run"], json!(false));

    let file = std::fs::File::open(&output).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    assert_eq!(archive.len(), 2);
    assert_eq!(archive.by_index(0).unwrap().name(), "a.txt");
    assert_eq!(archive.by_index(1).unwrap().name(), "sub/b.txt");
}

#[test]
fn env_lists_filters_and_never_returns_zero_for_path() {
    let def = rscmd_tool_system::env::def();
    let run = def.run_json.expect("env 应暴露 JSON 入口");

    let all = run(&mut ToolCtx::default(), &json!({})).unwrap();
    assert!(all["count"].as_u64().unwrap() >= 1);
    assert_eq!(
        all["count"].as_u64().unwrap(),
        all["vars"].as_array().unwrap().len() as u64
    );
    assert_eq!(all["case_insensitive"], json!(cfg!(windows)));

    let path = run(&mut ToolCtx::default(), &json!({"name": "PATH"})).unwrap();
    assert!(path["count"].as_u64().unwrap() >= 1);

    let none = run(
        &mut ToolCtx::default(),
        &json!({"prefix": "RSCMD_NO_SUCH_PREFIX_"}),
    )
    .unwrap();
    assert_eq!(none["count"], 0);
}
