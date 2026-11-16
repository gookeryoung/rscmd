use std::process::Command;

use serde_json::Value;

fn run(args: &[&str]) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_rscmd"))
        .args(args)
        .output()
        .expect("应能启动 rscmd");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn parse_json(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|err| panic!("输出不是合法 JSON: {err}\n{text}"))
}

#[test]
fn case_success_envelope() {
    let (code, stdout, stderr) = run(&["--json", "case", "--mode", "snake", "HelloWorld"]);
    assert_eq!(code, 0, "stderr: {stderr}");
    let envelope = parse_json(&stdout);
    assert_eq!(envelope["schema"], "rscmd-result/1");
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["tool"], "case");
    assert_eq!(envelope["data"]["text"], "hello_world");
}

#[test]
fn case_invalid_mode_returns_error_envelope() {
    let (code, stdout, stderr) = run(&["--json", "case", "--mode", "dot", "HelloWorld"]);
    assert_eq!(code, 3);
    assert!(stdout.is_empty(), "错误信封应写到 stderr: {stdout}");
    let envelope = parse_json(&stderr);
    assert_eq!(envelope["schema"], "rscmd-error/1");
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["code"], "INVALID_MODE");
    assert_eq!(envelope["field"], "mode");
    assert_eq!(envelope["exit_code"], 3);
}

#[test]
fn json_fmt_invalid_json_returns_error_envelope() {
    let (code, _stdout, stderr) = run(&["--json", "json_fmt", "{"]);
    assert_eq!(code, 3);
    let envelope = parse_json(&stderr);
    assert_eq!(envelope["code"], "INVALID_JSON");
    assert_eq!(envelope["field"], "json");
}

#[test]
fn verbose_trace_only_goes_to_stderr() {
    let (code, stdout, stderr) = run(&["--verbose", "case", "--mode", "snake", "HelloWorld"]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout, "hello_world\n");
    assert!(stderr.starts_with("[rscmd] tool=case"), "{stderr}");
    assert!(stderr.contains("elapsed="), "{stderr}");
}

#[test]
fn global_flags_work_after_subcommand() {
    let (code, stdout, _stderr) = run(&["case", "HelloWorld", "--json"]);
    assert_eq!(code, 0);
    let envelope = parse_json(&stdout);
    assert_eq!(envelope["data"]["text"], "hello_world");
}

#[test]
fn tools_lists_builtin_tools() {
    let (code, stdout, _stderr) = run(&["--json", "tools"]);
    assert_eq!(code, 0);
    let items = parse_json(&stdout);
    let names: Vec<&str> = items["data"]
        .as_array()
        .expect("data 应返回数组")
        .iter()
        .filter_map(|item| item["name"].as_str())
        .collect();
    assert!(names.contains(&"case"));
    assert!(names.contains(&"json_fmt"));
}

#[test]
fn unknown_tool_is_usage_error() {
    let (code, _stdout, stderr) = run(&["--json", "nope"]);
    assert_eq!(code, 2);
    let envelope = parse_json(&stderr);
    assert_eq!(envelope["schema"], "rscmd-error/1");
    assert_eq!(envelope["code"], "USAGE");
}

#[test]
fn quiet_suppresses_human_output() {
    let (code, stdout, _stderr) = run(&["--quiet", "case", "HelloWorld"]);
    assert_eq!(code, 0);
    assert!(stdout.is_empty(), "quiet 下不应输出成功结果: {stdout}");
}

#[test]
fn version_reports_package_version() {
    let (code, stdout, _stderr) = run(&["--json", "version"]);
    assert_eq!(code, 0);
    let envelope = parse_json(&stdout);
    assert_eq!(envelope["data"]["version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn dash_version_exits_zero() {
    let (code, stdout, _stderr) = run(&["--version"]);
    assert_eq!(code, 0);
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")), "{stdout}");
}

#[test]
fn dash_help_exits_zero() {
    let (code, stdout, _stderr) = run(&["--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("Usage"), "{stdout}");
}

#[test]
fn no_args_prints_help_and_lists_commands() {
    let (code, stdout, stderr) = run(&[]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("Commands:"), "{stdout}");
    assert!(stdout.contains("doctor"), "{stdout}");
    assert!(stdout.contains("version"), "{stdout}");
}

#[test]
fn no_args_json_returns_envelope_with_help() {
    let (code, stdout, _stderr) = run(&["--json"]);
    assert_eq!(code, 0);
    let envelope = parse_json(&stdout);
    assert_eq!(envelope["ok"], true);
    let text = envelope["data"]["text"].as_str().unwrap_or_default();
    assert!(text.contains("Commands:"), "{text}");
}

#[test]
fn version_human_is_single_line() {
    let (code, stdout, _stderr) = run(&["version"]);
    assert_eq!(code, 0);
    assert_eq!(
        stdout.trim(),
        format!("rscmd {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn doctor_human_report_passes() {
    let (code, stdout, _stderr) = run(&["doctor"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("环境自检"), "{stdout}");
    assert!(stdout.contains("诊断结果:"), "{stdout}");
    assert!(stdout.contains("[OK  ]"), "{stdout}");
}

#[test]
fn doctor_json_has_checks() {
    let (code, stdout, _stderr) = run(&["--json", "doctor"]);
    assert_eq!(code, 0);
    let envelope = parse_json(&stdout);
    assert_eq!(envelope["ok"], true);
    let data = &envelope["data"];
    assert_eq!(data["ok"], true);
    assert!(data["checks"].as_array().unwrap().len() >= 3);
}
