//! MCP（Model Context Protocol）薄封装：把注册表映射成 MCP 的 `tools/list` 与 `tools/call`。
//!
//! 协议对象是纯构造；stdio 循环由 [`serve`] / [`serve_stdio`] 提供，CLI 与 Python 绑定共用，
//! 避免两侧各写一份逐行读写逻辑。
//! 暴露范围：有 JSON 入口（`run_json`）且非 GUI 的工具，参数表直接复用工具的 `schema`。

use std::io::{BufRead, BufWriter, Write};

use serde_json::{json, Map, Value};

use crate::ctx::ToolCtx;
use crate::errors::RscmdError;
use crate::registry::Registry;
use crate::render;
use crate::tool::ToolDef;

/// 本服务实现的 MCP 协议版本（stdio 传输）。
pub const PROTOCOL_VERSION: &str = "2025-06-18";
pub const SERVER_NAME: &str = "rscmd";

/// `tools/list` 的 `result`。
pub fn tools_list(tools: &'static [ToolDef]) -> Value {
    let items: Vec<Value> = tools
        .iter()
        .filter(|tool| !tool.meta.gui && tool.run_json.is_some())
        .map(|tool| {
            json!({
                "name": tool.meta.name,
                "description": tool.meta.summary,
                "inputSchema": input_schema(tool),
            })
        })
        .collect();
    json!({ "tools": items })
}

fn input_schema(tool: &'static ToolDef) -> Value {
    tool.schema
        .and_then(|schema| serde_json::from_str::<Value>(&schema()).ok())
        .unwrap_or_else(|| json!({ "type": "object", "properties": {} }))
}

/// 处理一条 JSON-RPC 请求；通知（无 `id`）返回 `None`。
pub fn handle_request(tools: &'static [ToolDef], request: &Value) -> Option<Value> {
    let Some(object) = request.as_object() else {
        return Some(error_response(
            Value::Null,
            -32600,
            "请求必须是 JSON 对象".to_string(),
        ));
    };
    // 通知没有 id，按 JSON-RPC 2.0 不应有响应。
    let id = object.get("id")?.clone();
    let method = object
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();

    match method {
        "initialize" => Some(success_response(
            id,
            initialize_result(object.get("params")),
        )),
        "ping" => Some(success_response(id, json!({}))),
        "tools/list" => Some(success_response(id, tools_list(tools))),
        "tools/call" => {
            let result = tools_call(tools, object.get("params"));
            Some(match result {
                Ok(result) => success_response(id, result),
                Err((code, message)) => error_response(id, code, message),
            })
        }
        _ => Some(error_response(id, -32601, format!("未知方法: {method}"))),
    }
}

/// 无法解析成 JSON 时的响应。
pub fn parse_error_response() -> Value {
    error_response(Value::Null, -32700, "无法解析 JSON".to_string())
}

/// 逐行读取 JSON-RPC 请求并向 `writer` 写回响应；读到 EOF 返回 0，写出失败返回 1。
pub fn serve<R: BufRead, W: Write>(tools: &'static [ToolDef], reader: R, writer: W) -> i32 {
    let mut writer = BufWriter::new(writer);
    for line in reader.lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(request) => handle_request(tools, &request),
            Err(_) => Some(parse_error_response()),
        };
        let Some(response) = response else {
            continue; // 通知不需要响应
        };
        let text = serde_json::to_string(&response).unwrap_or_default();
        if writeln!(writer, "{text}")
            .and_then(|_| writer.flush())
            .is_err()
        {
            return 1;
        }
    }
    0
}

/// 以当前进程的 stdin/stdout 提供服务（CLI `rscmd mcp` 与 Python 绑定共用）。
pub fn serve_stdio(tools: &'static [ToolDef]) -> i32 {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    serve(tools, stdin.lock(), stdout.lock())
}

fn initialize_result(params: Option<&Value>) -> Value {
    // 回显客户端请求的版本；缺省时用本服务实现的版本。
    let protocol = params
        .and_then(|params| params.get("protocolVersion"))
        .and_then(Value::as_str)
        .unwrap_or(PROTOCOL_VERSION);
    json!({
        "protocolVersion": protocol,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") },
    })
}

fn tools_call(tools: &'static [ToolDef], params: Option<&Value>) -> Result<Value, (i64, String)> {
    let params = params
        .and_then(Value::as_object)
        .ok_or((-32602, "tools/call 缺少 params".to_string()))?;
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or((-32602, "tools/call 缺少工具名".to_string()))?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let Some(tool) = Registry::new(tools).find(name) else {
        return Ok(tool_error(format!("未知工具: {name}")));
    };
    let Some(run_json) = tool.run_json else {
        return Ok(tool_error(format!(
            "工具 `{}` 只支持命令行参数，未提供 JSON 入口",
            tool.meta.name
        )));
    };

    Ok(match run_json(&mut ToolCtx::default(), &arguments) {
        Ok(data) => json!({
            "content": [{ "type": "text", "text": data_text(&data) }],
            "isError": false,
        }),
        Err(err) => tool_error(error_text(&err)),
    })
}

fn tool_error(message: String) -> Value {
    json!({
        "content": [{ "type": "text", "text": message }],
        "isError": true,
    })
}

/// 成功结果转文本：优先 `text` / `json` 字段，其余用紧凑 JSON。
fn data_text(data: &Value) -> String {
    if let Some(object) = data.as_object() {
        if let Some(text) = object.get("text").and_then(Value::as_str) {
            return text.to_string();
        }
        if let Some(text) = object.get("json").and_then(Value::as_str) {
            return text.to_string();
        }
    }
    serde_json::to_string(data).unwrap_or_default()
}

fn error_text(err: &RscmdError) -> String {
    render::human_error(err).trim_end().to_string()
}

fn success_response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: Value, code: i64, message: String) -> Value {
    let mut error = Map::new();
    error.insert("code".into(), Value::from(code));
    error.insert("message".into(), Value::from(message));
    json!({ "jsonrpc": "2.0", "id": id, "error": Value::Object(error) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::{PythonExposure, SideEffects, ToolMeta};
    use clap::{ArgMatches, Command};

    static META_ECHO: ToolMeta = ToolMeta {
        name: "echo",
        aliases: &[],
        summary: "回显文本",
        side_effects: SideEffects::None,
        gui: false,
        python: PythonExposure::Structured,
        examples: &["rscmd echo --text hi"],
    };

    static META_FAIL: ToolMeta = ToolMeta {
        name: "fail",
        aliases: &[],
        summary: "总是失败",
        side_effects: SideEffects::None,
        gui: false,
        python: PythonExposure::Structured,
        examples: &["rscmd fail"],
    };

    static META_ARGV: ToolMeta = ToolMeta {
        name: "argv_only",
        aliases: &[],
        summary: "只有命令行入口",
        side_effects: SideEffects::None,
        gui: false,
        python: PythonExposure::ArgvOnly,
        examples: &["rscmd argv_only"],
    };

    fn build_echo() -> Command {
        Command::new("echo")
    }

    fn build_fail() -> Command {
        Command::new("fail")
    }

    fn build_argv() -> Command {
        Command::new("argv_only")
    }

    fn run_cli(_ctx: &mut ToolCtx, _matches: &ArgMatches) -> Result<Value, RscmdError> {
        Ok(Value::Null)
    }

    fn run_echo(_ctx: &mut ToolCtx, input: &Value) -> Result<Value, RscmdError> {
        Ok(json!({ "text": input.get("text").and_then(Value::as_str).unwrap_or_default() }))
    }

    fn run_fail(_ctx: &mut ToolCtx, _input: &Value) -> Result<Value, RscmdError> {
        Err(RscmdError::coded("INVALID_ARGS", "空的输入").with_hint("传入 text"))
    }

    fn echo_schema() -> String {
        r#"{"type":"object","properties":{"text":{"type":"string"}}}"#.to_string()
    }

    static TOOLS: [ToolDef; 3] = [
        ToolDef {
            meta: &META_ECHO,
            build: build_echo,
            run: run_cli,
            run_json: Some(run_echo),
            schema: Some(echo_schema),
        },
        ToolDef {
            meta: &META_FAIL,
            build: build_fail,
            run: run_cli,
            run_json: Some(run_fail),
            schema: None,
        },
        ToolDef {
            meta: &META_ARGV,
            build: build_argv,
            run: run_cli,
            run_json: None,
            schema: None,
        },
    ];

    fn request(method: &str, params: Value) -> Value {
        json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })
    }

    #[test]
    fn lists_structured_non_gui_tools_with_schemas() {
        let result = tools_list(&TOOLS);
        let tools = result["tools"].as_array().unwrap();
        let names: Vec<&str> = tools
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        assert_eq!(names, ["echo", "fail"]);
        assert_eq!(tools[0]["description"], "回显文本");
        assert_eq!(
            tools[0]["inputSchema"]["properties"]["text"]["type"],
            "string"
        );
        // 未提供 schema 的工具回退为宽松对象，保证 MCP 客户端可调用。
        assert_eq!(tools[1]["inputSchema"]["type"], "object");
    }

    #[test]
    fn initialize_echoes_requested_protocol_version() {
        let response = handle_request(
            &TOOLS,
            &request("initialize", json!({ "protocolVersion": "2025-06-18" })),
        )
        .unwrap();
        assert_eq!(response["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(response["result"]["serverInfo"]["name"], SERVER_NAME);
        assert_eq!(response["result"]["capabilities"]["tools"], json!({}));

        let response = handle_request(&TOOLS, &request("initialize", json!({}))).unwrap();
        assert_eq!(response["result"]["protocolVersion"], PROTOCOL_VERSION);
    }

    #[test]
    fn notifications_get_no_response() {
        let notification = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        assert!(handle_request(&TOOLS, &notification).is_none());
    }

    #[test]
    fn calls_tool_and_returns_text_content() {
        let response = handle_request(
            &TOOLS,
            &request(
                "tools/call",
                json!({ "name": "echo", "arguments": { "text": "hi" } }),
            ),
        )
        .unwrap();
        assert_eq!(response["result"]["isError"], false);
        assert_eq!(response["result"]["content"][0]["type"], "text");
        assert_eq!(response["result"]["content"][0]["text"], "hi");
    }

    #[test]
    fn tool_failures_are_reported_as_is_error() {
        let response = handle_request(
            &TOOLS,
            &request("tools/call", json!({ "name": "fail", "arguments": {} })),
        )
        .unwrap();
        let result = &response["result"];
        assert_eq!(result["isError"], true);
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("INVALID_ARGS"), "{text}");
        assert!(text.contains("传入 text"), "{text}");
    }

    #[test]
    fn unknown_and_argv_only_tools_report_actionable_errors() {
        let response = handle_request(
            &TOOLS,
            &request("tools/call", json!({ "name": "missing", "arguments": {} })),
        )
        .unwrap();
        assert_eq!(response["result"]["isError"], true);
        assert!(response["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("未知工具: missing"));

        let response = handle_request(
            &TOOLS,
            &request(
                "tools/call",
                json!({ "name": "argv_only", "arguments": {} }),
            ),
        )
        .unwrap();
        assert_eq!(response["result"]["isError"], true);
        assert!(response["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("未提供 JSON 入口"));
    }

    #[test]
    fn invalid_calls_map_to_jsonrpc_errors() {
        let response = handle_request(&TOOLS, &request("tools/call", json!({}))).unwrap();
        assert_eq!(response["error"]["code"], -32602);

        let response =
            handle_request(&TOOLS, &request("tools/call", json!({ "arguments": {} }))).unwrap();
        assert_eq!(response["error"]["code"], -32602);
    }

    #[test]
    fn unknown_method_and_invalid_payloads_are_errors() {
        let response = handle_request(&TOOLS, &request("nope", json!({}))).unwrap();
        assert_eq!(response["error"]["code"], -32601);

        let response = handle_request(&TOOLS, &json!([])).unwrap();
        assert_eq!(response["error"]["code"], -32600);
        assert_eq!(response["id"], Value::Null);

        let response = parse_error_response();
        assert_eq!(response["error"]["code"], -32700);
        assert_eq!(response["jsonrpc"], "2.0");
    }

    #[test]
    fn ping_returns_empty_result() {
        let response = handle_request(&TOOLS, &request("ping", json!({}))).unwrap();
        assert_eq!(response["result"], json!({}));
    }

    #[test]
    fn serve_writes_one_line_per_request_and_skips_notifications() {
        let input = [
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            "",
            r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
            "{not json}",
        ]
        .join("\n")
            + "\n";
        let mut output: Vec<u8> = Vec::new();
        let code = serve(&TOOLS, std::io::Cursor::new(input), &mut output);
        assert_eq!(code, 0);

        let lines: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        // 通知与空行不产生响应，非法 JSON 产生 -32700。
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0]["result"]["serverInfo"]["name"], SERVER_NAME);
        assert_eq!(lines[1]["result"], json!({}));
        assert_eq!(lines[2]["error"]["code"], -32700);
    }
}
