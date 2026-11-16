use rscmd_core::ToolCtx;
use serde_json::json;

#[test]
fn hash_json_entry_returns_sha256_and_bytes() {
    let def = rscmd_tool_crypto::hash::def();
    let run = def.run_json.expect("hash 应暴露 JSON 入口");
    let mut ctx = ToolCtx::default();
    let out = run(&mut ctx, &json!({"text": "abc"})).unwrap();
    assert_eq!(
        out["hex"],
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(out["bytes"], 3);
}

#[test]
fn hash_rejects_conflicting_inputs() {
    let def = rscmd_tool_crypto::hash::def();
    let run = def.run_json.expect("hash 应暴露 JSON 入口");
    let mut ctx = ToolCtx::default();
    let err = run(&mut ctx, &json!({"text": "abc", "file": "README.md"})).unwrap_err();
    assert_eq!(err.code, "INVALID_ARGS");
    assert_eq!(err.exit_code(), 3);
}

#[test]
fn codec_json_entry_roundtrips_base64() {
    let def = rscmd_tool_crypto::codec::def();
    let run = def.run_json.expect("codec 应暴露 JSON 入口");
    let mut ctx = ToolCtx::default();
    let encoded = run(&mut ctx, &json!({"text": "hello", "mode": "base64"})).unwrap();
    assert_eq!(encoded["text"], "aGVsbG8=");
    assert_eq!(encoded["direction"], "encode");
    let decoded = run(
        &mut ctx,
        &json!({"text": "aGVsbG8=", "mode": "base64", "decode": true}),
    )
    .unwrap();
    assert_eq!(decoded["text"], "hello");
}

#[test]
fn codec_reports_invalid_encoding() {
    let def = rscmd_tool_crypto::codec::def();
    let run = def.run_json.expect("codec 应暴露 JSON 入口");
    let mut ctx = ToolCtx::default();
    let err = run(
        &mut ctx,
        &json!({"text": "!!", "mode": "base64", "decode": true}),
    )
    .unwrap_err();
    assert_eq!(err.code, "INVALID_ENCODING");
    assert_eq!(err.kind, rscmd_core::ErrorKind::InvalidInput);
    assert_eq!(err.exit_code(), 3);
    assert_eq!(err.field.as_deref(), Some("text"));
}
