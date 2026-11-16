//! `urlcheck`：探测 URL 的 HTTP 状态码、重定向链与耗时。

use std::time::{Duration, Instant};

use clap::{ArgMatches, Command};
use rscmd_core::{
    command_for, PythonExposure, RscmdError, SideEffects, ToolCtx, ToolDef, ToolMeta,
};
use rscmd_runtime::net::{probe, HttpMethod};
use serde_json::{json, Value};

pub static META: ToolMeta = ToolMeta {
    name: "urlcheck",
    aliases: &["urlchecktool"],
    summary: "探测 URL 的 HTTP 状态码、重定向链与耗时（HEAD/GET）",
    side_effects: SideEffects::Network,
    gui: false,
    python: PythonExposure::Structured,
    examples: &[
        "rscmd urlcheck https://example.com",
        "rscmd urlcheck https://example.com --method get --max-redirects 3",
    ],
};

#[derive(Debug, clap::Args, serde::Deserialize, schemars::JsonSchema)]
pub struct UrlcheckArgs {
    /// 待探测的 URL（仅支持 http:// 与 https://）
    #[arg(value_name = "URL")]
    pub url: String,
    /// 请求方法：head 或 get
    #[arg(long, value_name = "METHOD", default_value = "head")]
    #[serde(default = "default_method")]
    pub method: String,
    /// 最多跟随的重定向次数；0 表示不跟随
    #[arg(long = "max-redirects", value_name = "N", default_value_t = 5)]
    #[serde(default = "default_max_redirects")]
    pub max_redirects: u32,
}

fn default_method() -> String {
    "head".to_string()
}

fn default_max_redirects() -> u32 {
    5
}

pub fn build() -> Command {
    command_for::<UrlcheckArgs>(&META)
}

pub fn run_cli(ctx: &mut ToolCtx, matches: &ArgMatches) -> Result<Value, RscmdError> {
    let args = <UrlcheckArgs as clap::FromArgMatches>::from_arg_matches(matches)
        .map_err(RscmdError::from_clap)?;
    run(ctx, args)
}

pub fn run_json(ctx: &mut ToolCtx, input: &Value) -> Result<Value, RscmdError> {
    let args: UrlcheckArgs = serde_json::from_value(input.clone())
        .map_err(|err| RscmdError::coded("INVALID_ARGS", err.to_string()).with_field("args"))?;
    run(ctx, args)
}

fn run(ctx: &mut ToolCtx, args: UrlcheckArgs) -> Result<Value, RscmdError> {
    let method = match args.method.to_ascii_lowercase().as_str() {
        "head" => HttpMethod::Head,
        "get" => HttpMethod::Get,
        _ => {
            return Err(RscmdError::coded(
                "INVALID_MODE",
                format!("不支持的请求方法: {}", args.method),
            )
            .with_field("method")
            .with_expected("head 或 get"));
        }
    };
    let timeout = ctx.timeout_or(Duration::from_secs(30));
    let started = Instant::now();
    let result = probe(&args.url, method, timeout, args.max_redirects).map_err(|err| {
        RscmdError::coded(err.code(), err.to_string())
            .with_field("url")
            .with_expected("可访问的 http:// 或 https:// 地址")
    })?;
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);

    Ok(json!({
        "url": args.url,
        "method": method.as_str(),
        "final_url": result.final_url,
        "status": result.status,
        "ok": result.status < 400,
        "redirects": result
            .redirects
            .iter()
            .map(|hop| json!({
                "url": hop.url,
                "status": hop.status,
                "location": hop.location,
            }))
            .collect::<Vec<_>>(),
        "elapsed_ms": elapsed_ms,
        "content_length": result.content_length,
    }))
}

pub fn def() -> ToolDef {
    ToolDef {
        meta: &META,
        build,
        run: run_cli,
        run_json: Some(run_json),
        schema: Some(urlcheck_args_schema),
    }
}

fn urlcheck_args_schema() -> String {
    let schema = schemars::schema_for!(UrlcheckArgs);
    serde_json::to_string_pretty(&schema).expect("UrlcheckArgs schema 可序列化")
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread::JoinHandle;

    use super::*;

    fn matches(args: &[&str]) -> ArgMatches {
        build().try_get_matches_from(args).expect("参数应可解析")
    }

    fn serve_once(response: &'static [u8]) -> (String, JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("应能绑定端口");
        let addr = listener.local_addr().expect("应能取到地址");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("应能接受连接");
            let mut request = Vec::new();
            let mut buffer = [0u8; 1024];
            loop {
                let read = stream.read(&mut buffer).expect("应能读取请求");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            stream.write_all(response).expect("应能写响应");
            String::from_utf8_lossy(&request).into_owned()
        });
        (format!("http://{addr}/data"), handle)
    }

    #[test]
    fn probes_local_server_with_shared_envelope() {
        let (url, handle) =
            serve_once(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nignored");
        let out = run_cli(&mut ToolCtx::default(), &matches(&["urlcheck", &url])).unwrap();
        assert_eq!(out["status"], 200);
        assert_eq!(out["ok"], true);
        assert_eq!(out["method"], "head");
        assert_eq!(out["content_length"], 7);
        assert_eq!(out["final_url"], url.as_str());
        assert!(out["redirects"].as_array().unwrap().is_empty());
        assert!(out["elapsed_ms"].is_u64());
        let request = handle.join().unwrap();
        assert!(request.starts_with("HEAD /data HTTP/1.1"), "{request}");
    }

    #[test]
    fn get_reports_server_error_status_but_not_tool_error() {
        let (url, handle) = serve_once(
            b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        let out = run_json(
            &mut ToolCtx::default(),
            &json!({"url": url, "method": "get"}),
        )
        .unwrap();
        assert_eq!(out["status"], 500);
        assert_eq!(out["ok"], false);
        assert_eq!(out["method"], "get");
        handle.join().unwrap();
    }

    #[test]
    fn rejects_unknown_method() {
        let err = run_cli(
            &mut ToolCtx::default(),
            &matches(&["urlcheck", "http://127.0.0.1:1", "--method", "post"]),
        )
        .unwrap_err();
        assert_eq!(err.code, "INVALID_MODE");
        assert_eq!(err.exit_code(), 3);
        assert_eq!(err.field.as_deref(), Some("method"));
    }

    #[test]
    fn rejects_unsupported_scheme() {
        let err = run_cli(
            &mut ToolCtx::default(),
            &matches(&["urlcheck", "ftp://example.com"]),
        )
        .unwrap_err();
        assert_eq!(err.code, "UNSUPPORTED_SCHEME");
        assert_eq!(err.exit_code(), 3);
        assert_eq!(err.field.as_deref(), Some("url"));
    }

    #[test]
    fn json_entry_requires_url() {
        let err = run_json(&mut ToolCtx::default(), &json!({})).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("args"));
    }

    #[test]
    fn def_exposes_schema_with_all_fields() {
        let tool = def();
        assert_eq!(tool.meta.name, "urlcheck");
        assert_eq!(tool.meta.side_effects, SideEffects::Network);
        assert!(tool.run_json.is_some());
        let schema = (tool.schema.expect("应提供 schema"))();
        assert!(schema.contains("\"url\""), "{schema}");
        assert!(schema.contains("\"method\""), "{schema}");
        assert!(schema.contains("\"max_redirects\""), "{schema}");
    }
}
