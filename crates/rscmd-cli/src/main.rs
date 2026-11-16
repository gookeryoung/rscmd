use std::io::Write;
use std::process::ExitCode;

use rscmd_core::{scan_flags, Outcome, RawFlags};

/// Ctrl+C 时按当前输出模式应写出的内容与退出码（抽成纯函数便于单测）。
fn interrupt_payload(flags: RawFlags) -> (String, String, i32) {
    let outcome = Outcome::interrupted(&flags);
    let (stdout, stderr) = rscmd_core::render(&outcome);
    (stdout, stderr, outcome.exit_code())
}

/// Ctrl+C 统一出口：按当前输出模式渲染错误信封，再以 130 退出。
fn exit_on_interrupt(flags: RawFlags) -> ! {
    let (stdout, stderr, code) = interrupt_payload(flags);
    let _ = std::io::stdout().write_all(stdout.as_bytes());
    let _ = std::io::stderr().write_all(stderr.as_bytes());
    std::process::exit(code)
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    // MCP 是阻塞式 stdio 服务，不走 clap/信封路径，直接接管入口。
    if argv.first().map(String::as_str) == Some("mcp") {
        return if argv.len() == 1 {
            serve_mcp()
        } else {
            mcp_usage()
        };
    }
    // 注册失败（受限环境）时不阻断命令，退回系统默认中断行为。
    let _ = ctrlc::set_handler({
        let flags = scan_flags(&argv);
        move || exit_on_interrupt(flags)
    });

    let outcome = rscmd_core::dispatch(rscmd_tools::tools(), &argv);
    let (stdout, stderr) = rscmd_core::render(&outcome);

    if !stdout.is_empty() {
        print!("{stdout}");
    }
    if !stderr.is_empty() {
        eprint!("{stderr}");
    }

    ExitCode::from(outcome.exit_code().clamp(0, 255) as u8)
}

fn mcp_usage() -> ExitCode {
    eprintln!(
        "用法: rscmd mcp\n\n\
         以 MCP stdio server 模式运行：从 stdin 逐行读取 JSON-RPC 请求，向 stdout 写响应。"
    );
    ExitCode::from(2)
}

/// 逐行读取 JSON-RPC 请求并写回响应；stdin 关闭后正常退出。
fn serve_mcp() -> ExitCode {
    use std::io::{BufRead, BufWriter, Write};

    let tools = rscmd_tools::tools();
    let stdin = std::io::stdin();
    let mut stdout = BufWriter::new(std::io::stdout().lock());
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<serde_json::Value>(&line) {
            Ok(request) => rscmd_core::mcp::handle_request(tools, &request),
            Err(_) => Some(rscmd_core::mcp::parse_error_response()),
        };
        let Some(response) = response else {
            continue; // 通知不需要响应
        };
        let text = serde_json::to_string(&response).unwrap_or_default();
        if writeln!(stdout, "{text}")
            .and_then(|_| stdout.flush())
            .is_err()
        {
            return ExitCode::from(1);
        }
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupt_payload_json_writes_error_envelope_to_stderr() {
        let flags = RawFlags {
            json: true,
            quiet: true,
            verbose: true,
        };
        let (stdout, stderr, code) = interrupt_payload(flags);
        assert!(stdout.is_empty());
        assert_eq!(code, 130);
        // `--quiet` 不吞中断信息，`--verbose` 也不追加跟踪。
        let envelope: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
        assert_eq!(envelope["schema"], "rscmd-error/1");
        assert_eq!(envelope["kind"], "interrupted");
        assert_eq!(envelope["code"], "INTERRUPTED");
        assert_eq!(envelope["exit_code"], 130);
    }

    #[test]
    fn interrupt_payload_human_mode_writes_stderr() {
        let (stdout, stderr, code) = interrupt_payload(RawFlags::default());
        assert!(stdout.is_empty());
        assert_eq!(code, 130);
        assert!(stderr.contains("INTERRUPTED"), "{stderr}");
    }
}
