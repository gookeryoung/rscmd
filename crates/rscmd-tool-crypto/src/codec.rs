//! `codec`：base64 / hex / URL 文本编解码。

use clap::{ArgMatches, Command};
use rscmd_core::{
    command_for, PythonExposure, RscmdError, SideEffects, ToolCtx, ToolDef, ToolMeta,
};
use rscmd_runtime::codec::{base64_decode, base64_encode, hex_decode, hex_encode};
use rscmd_runtime::net;
use serde_json::{json, Value};

pub static META: ToolMeta = ToolMeta {
    name: "codec",
    aliases: &["codetool"],
    summary: "文本编解码：base64 / hex / URL",
    side_effects: SideEffects::None,
    gui: false,
    python: PythonExposure::Structured,
    examples: &[
        "rscmd codec hello --mode hex",
        "rscmd codec aGVsbG8= --mode base64 --decode",
        "rscmd codec 'a b' --mode url",
    ],
};

#[derive(Debug, clap::Args, serde::Deserialize, schemars::JsonSchema)]
pub struct CodecArgs {
    /// 待编码或解码的文本
    #[arg(value_name = "TEXT")]
    pub text: String,
    /// 编解码方式：base64/hex/url
    #[arg(long, value_name = "MODE", default_value = "base64")]
    #[serde(default = "default_mode")]
    pub mode: String,
    /// 解码（默认编码）
    #[arg(long)]
    #[serde(default)]
    pub decode: bool,
}

fn default_mode() -> String {
    "base64".to_string()
}

pub fn build() -> Command {
    command_for::<CodecArgs>(&META)
}

pub fn run_cli(ctx: &mut ToolCtx, matches: &ArgMatches) -> Result<Value, RscmdError> {
    let args = <CodecArgs as clap::FromArgMatches>::from_arg_matches(matches)
        .map_err(RscmdError::from_clap)?;
    run(ctx, args)
}

pub fn run_json(ctx: &mut ToolCtx, input: &Value) -> Result<Value, RscmdError> {
    let args: CodecArgs = serde_json::from_value(input.clone())
        .map_err(|err| RscmdError::coded("INVALID_ARGS", err.to_string()).with_field("args"))?;
    run(ctx, args)
}

fn run(_ctx: &mut ToolCtx, args: CodecArgs) -> Result<Value, RscmdError> {
    let mode = args.mode.to_ascii_lowercase();
    let direction = if args.decode { "decode" } else { "encode" };
    let text = if args.decode {
        decode(&mode, &args.text)?
    } else {
        encode(&mode, &args.text)?
    };
    Ok(json!({
        "mode": mode,
        "direction": direction,
        "text": text,
        "bytes": text.len(),
    }))
}

fn encode(mode: &str, text: &str) -> Result<String, RscmdError> {
    match mode {
        "base64" => Ok(base64_encode(text.as_bytes())),
        "hex" => Ok(hex_encode(text.as_bytes())),
        "url" => Ok(net::url_encode(text)),
        other => Err(unknown_mode(other)),
    }
}

fn decode(mode: &str, text: &str) -> Result<String, RscmdError> {
    let bytes = match mode {
        "base64" => base64_decode(text).map_err(encoding_error)?,
        "hex" => hex_decode(text).map_err(encoding_error)?,
        "url" => {
            let decoded = net::decode(text).map_err(|_| {
                RscmdError::coded("INVALID_ENCODING", "URL 解码结果不是有效的 UTF-8 文本")
                    .with_field("text")
            })?;
            return Ok(decoded);
        }
        other => return Err(unknown_mode(other)),
    };
    String::from_utf8(bytes).map_err(|_| {
        RscmdError::coded("INVALID_ENCODING", "解码结果不是有效的 UTF-8 文本")
            .with_field("text")
            .with_expected("UTF-8 文本")
    })
}

fn encoding_error(err: rscmd_runtime::codec::CodecError) -> RscmdError {
    RscmdError::coded("INVALID_ENCODING", err.to_string()).with_field("text")
}

fn unknown_mode(mode: &str) -> RscmdError {
    RscmdError::coded("INVALID_MODE", format!("未知编解码方式: {mode}"))
        .with_field("mode")
        .with_expected("base64/hex/url")
}

pub fn def() -> ToolDef {
    ToolDef {
        meta: &META,
        build,
        run: run_cli,
        run_json: Some(run_json),
        schema: Some(codec_args_schema),
    }
}

fn codec_args_schema() -> String {
    let schema = schemars::schema_for!(CodecArgs);
    serde_json::to_string_pretty(&schema).expect("CodecArgs schema 可序列化")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(args: &[&str]) -> ArgMatches {
        build().try_get_matches_from(args).expect("参数应可解析")
    }

    #[test]
    fn cli_default_mode_is_base64() {
        let out = run_cli(&mut ToolCtx::default(), &matches(&["codec", "hello"])).unwrap();
        assert_eq!(out["mode"], "base64");
        assert_eq!(out["direction"], "encode");
        assert_eq!(out["text"], "aGVsbG8=");
        assert_eq!(out["bytes"], 8);
    }

    #[test]
    fn cli_encodes_hex_and_url() {
        let out = run_cli(
            &mut ToolCtx::default(),
            &matches(&["codec", "abc", "--mode", "hex"]),
        )
        .unwrap();
        assert_eq!(out["text"], "616263");

        let out = run_cli(
            &mut ToolCtx::default(),
            &matches(&["codec", "a b", "--mode", "URL"]),
        )
        .unwrap();
        assert_eq!(out["mode"], "url");
        assert_eq!(out["text"], "a%20b");
    }

    #[test]
    fn cli_decodes_each_mode() {
        for (mode, encoded, expected) in [
            ("base64", "aGVsbG8=", "hello"),
            ("hex", "616263", "abc"),
            ("url", "a%20b", "a b"),
        ] {
            let out = run_cli(
                &mut ToolCtx::default(),
                &matches(&["codec", encoded, "--mode", mode, "--decode"]),
            )
            .unwrap();
            assert_eq!(out["direction"], "decode");
            assert_eq!(out["text"], expected, "mode={mode}");
        }
    }

    #[test]
    fn json_entry_roundtrips_utf8() {
        let out = run_json(
            &mut ToolCtx::default(),
            &json!({"text": "你好", "mode": "base64"}),
        )
        .unwrap();
        let encoded = out["text"].as_str().unwrap().to_string();
        let out = run_json(
            &mut ToolCtx::default(),
            &json!({"text": encoded, "mode": "base64", "decode": true}),
        )
        .unwrap();
        assert_eq!(out["text"], "你好");
    }

    #[test]
    fn json_entry_rejects_missing_text() {
        let err = run_json(&mut ToolCtx::default(), &json!({})).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("args"));
    }

    #[test]
    fn unknown_mode_is_invalid_mode() {
        let err = run_cli(
            &mut ToolCtx::default(),
            &matches(&["codec", "abc", "--mode", "rot13"]),
        )
        .unwrap_err();
        assert_eq!(err.code, "INVALID_MODE");
        assert_eq!(err.exit_code(), 3);
        assert_eq!(err.field.as_deref(), Some("mode"));
    }

    #[test]
    fn bad_base64_is_invalid_encoding() {
        let err = run_cli(
            &mut ToolCtx::default(),
            &matches(&["codec", "abc", "--decode"]),
        )
        .unwrap_err();
        assert_eq!(err.code, "INVALID_ENCODING");
        assert_eq!(err.exit_code(), 3);
        assert_eq!(err.field.as_deref(), Some("text"));
    }

    #[test]
    fn non_utf8_payload_is_invalid_encoding() {
        // 0xFF 不是合法 UTF-8。
        let err = run_cli(
            &mut ToolCtx::default(),
            &matches(&["codec", "/w==", "--decode"]),
        )
        .unwrap_err();
        assert_eq!(err.code, "INVALID_ENCODING");
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn bad_hex_is_invalid_encoding() {
        let err = run_cli(
            &mut ToolCtx::default(),
            &matches(&["codec", "zz", "--mode", "hex", "--decode"]),
        )
        .unwrap_err();
        assert_eq!(err.code, "INVALID_ENCODING");
    }

    #[test]
    fn def_exposes_structured_json_entry() {
        let tool = def();
        assert_eq!(tool.meta.name, "codec");
        assert_eq!(tool.meta.aliases, &["codetool"]);
        assert!(tool.run_json.is_some());
        let schema = (tool.schema.expect("应提供 schema"))();
        assert!(schema.contains("\"mode\""), "{schema}");
        assert!(schema.contains("\"decode\""), "{schema}");
    }
}
