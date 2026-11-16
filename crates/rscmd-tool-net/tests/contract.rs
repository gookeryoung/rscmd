use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread::JoinHandle;

use rscmd_core::ToolCtx;
use serde_json::json;

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
fn urlcheck_reports_status_through_json_entry() {
    let (url, handle) =
        serve_once(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    let def = rscmd_tool_net::urlcheck::def();
    let run = def.run_json.expect("urlcheck 应暴露 JSON 入口");
    let mut ctx = ToolCtx::default();
    let out = run(&mut ctx, &json!({"url": url, "method": "get"})).unwrap();
    assert_eq!(out["status"], 204);
    assert_eq!(out["ok"], true);
    assert_eq!(out["method"], "get");
    assert_eq!(out["redirects"], json!([]));
    handle.join().unwrap();
}

#[test]
fn urlcheck_rejects_unsupported_scheme() {
    let def = rscmd_tool_net::urlcheck::def();
    let run = def.run_json.expect("urlcheck 应暴露 JSON 入口");
    let mut ctx = ToolCtx::default();
    let err = run(&mut ctx, &json!({"url": "file:///etc/passwd"})).unwrap_err();
    assert_eq!(err.code, "UNSUPPORTED_SCHEME");
    assert_eq!(err.exit_code(), 3);
    assert_eq!(err.field.as_deref(), Some("url"));
}

#[test]
fn urlcheck_rejects_unknown_method() {
    let def = rscmd_tool_net::urlcheck::def();
    let run = def.run_json.expect("urlcheck 应暴露 JSON 入口");
    let mut ctx = ToolCtx::default();
    let err = run(
        &mut ctx,
        &json!({"url": "http://127.0.0.1:1", "method": "post"}),
    )
    .unwrap_err();
    assert_eq!(err.code, "INVALID_MODE");
    assert_eq!(err.field.as_deref(), Some("method"));
}
