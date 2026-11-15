//! core 契约的黄金测试：注册表、错误、信封、渲染与调度。

use clap::{ArgMatches, Command};
use serde_json::{json, Value};

use rscmd_core::{
    command_for, dispatch, render, ErrorKind, Outcome, PythonExposure, Registry, RscmdError,
    SideEffects, ToolCtx, ToolDef, ToolMeta,
};

static META_ALPHA: ToolMeta = ToolMeta {
    name: "alpha",
    aliases: &["a"],
    summary: "示例工具 alpha",
    side_effects: SideEffects::None,
    gui: false,
    python: PythonExposure::Structured,
    examples: &["rscmd alpha hi"],
};

static META_GUI: ToolMeta = ToolMeta {
    name: "beta",
    aliases: &[],
    summary: "GUI 工具 beta",
    side_effects: SideEffects::Read,
    gui: true,
    python: PythonExposure::Hidden,
    examples: &["rscmd beta"],
};

#[derive(clap::Args)]
struct AlphaArgs {
    #[arg(value_name = "TEXT")]
    text: String,
}

fn build_alpha() -> Command {
    command_for::<AlphaArgs>(&META_ALPHA)
}

fn run_alpha(_ctx: &mut ToolCtx, matches: &ArgMatches) -> Result<Value, RscmdError> {
    let text = matches
        .get_one::<String>("text")
        .cloned()
        .unwrap_or_default();
    Ok(json!({ "text": text }))
}

fn run_alpha_json(_ctx: &mut ToolCtx, input: &Value) -> Result<Value, RscmdError> {
    Ok(input.clone())
}

fn build_beta() -> Command {
    Command::new("beta")
}

fn run_beta(_ctx: &mut ToolCtx, _matches: &ArgMatches) -> Result<Value, RscmdError> {
    Ok(json!({ "ok": true }))
}

static TOOLS: [ToolDef; 2] = [
    ToolDef {
        meta: &META_ALPHA,
        build: build_alpha,
        run: run_alpha,
        run_json: Some(run_alpha_json),
        schema: None,
    },
    ToolDef {
        meta: &META_GUI,
        build: build_beta,
        run: run_beta,
        run_json: None,
        schema: None,
    },
];

fn argv(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_string()).collect()
}

fn outcome(tool: &str, result: Result<Value, RscmdError>, json_mode: bool) -> Outcome {
    Outcome {
        tool: tool.to_string(),
        result,
        json: json_mode,
        quiet: false,
    }
}

#[test]
fn registry_finds_name_and_alias() {
    let registry = Registry::new(&TOOLS);
    assert_eq!(registry.tools().len(), 2);
    assert_eq!(registry.find("alpha").unwrap().meta.name, "alpha");
    assert_eq!(registry.find("a").unwrap().meta.name, "alpha");
    assert!(registry.find("missing").is_none());
}

#[test]
fn registry_accepts_valid_tools() {
    assert!(Registry::new(&TOOLS).validate().is_ok());
}

#[test]
fn registry_reports_all_problems() {
    static EMPTY: ToolMeta = ToolMeta {
        name: "",
        aliases: &[],
        summary: "",
        side_effects: SideEffects::None,
        gui: false,
        python: PythonExposure::Hidden,
        examples: &[],
    };
    static BAD: ToolMeta = ToolMeta {
        name: "Bad Name",
        aliases: &["", "tools"],
        summary: "x",
        side_effects: SideEffects::None,
        gui: false,
        python: PythonExposure::Structured,
        examples: &[],
    };
    static GUIDE: ToolMeta = ToolMeta {
        name: "guide",
        aliases: &["Bad Name"],
        summary: "y",
        side_effects: SideEffects::None,
        gui: false,
        python: PythonExposure::Hidden,
        examples: &["rscmd guide"],
    };
    static BAD_TOOLS: [ToolDef; 3] = [
        ToolDef {
            meta: &EMPTY,
            build: build_beta,
            run: run_beta,
            run_json: None,
            schema: None,
        },
        ToolDef {
            meta: &BAD,
            build: build_beta,
            run: run_beta,
            run_json: None,
            schema: None,
        },
        ToolDef {
            meta: &GUIDE,
            build: build_beta,
            run: run_beta,
            run_json: None,
            schema: None,
        },
    ];

    let problems = Registry::new(&BAD_TOOLS).validate().unwrap_err();
    let joined = problems.join("\n");
    assert!(joined.contains("空工具名"), "{joined}");
    assert!(joined.contains("只允许小写字母"), "{joined}");
    assert!(
        joined.contains("摘要超过 200") || joined.contains("缺少示例"),
        "{joined}"
    );
    assert!(joined.contains("别名冲突"), "{joined}");
    assert!(joined.contains("Structured"), "{joined}");
}

#[test]
fn registry_rejects_long_summary() {
    static LONG: ToolMeta = ToolMeta {
        name: "long",
        aliases: &[],
        summary: "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
        side_effects: SideEffects::None,
        gui: false,
        python: PythonExposure::Hidden,
        examples: &["rscmd long"],
    };
    static LONG_TOOLS: [ToolDef; 1] = [ToolDef {
        meta: &LONG,
        build: build_beta,
        run: run_beta,
        run_json: None,
        schema: None,
    }];
    let problems = Registry::new(&LONG_TOOLS).validate().unwrap_err();
    assert!(problems.iter().any(|item| item.contains("超过 200 字符")));
}

#[test]
fn error_kinds_map_to_exit_codes() {
    let cases = [
        (ErrorKind::Runtime, "runtime", 1),
        (ErrorKind::Usage, "usage", 2),
        (ErrorKind::InvalidInput, "invalid_input", 3),
        (ErrorKind::NotFound, "not_found", 4),
        (ErrorKind::CheckFailed, "check_failed", 5),
        (ErrorKind::Interrupted, "interrupted", 130),
    ];
    for (kind, text, code) in cases {
        assert_eq!(kind.as_str(), text);
        assert_eq!(kind.exit_code(), code);
    }
}

#[test]
fn error_builders_attach_context() {
    let err = RscmdError::invalid_input("INVALID_X", "bad x")
        .with_field("x")
        .with_expected("1..10")
        .with_hint("试试 5");
    assert_eq!(err.exit_code(), 3);
    assert_eq!(err.field.as_deref(), Some("x"));
    assert_eq!(err.expected.as_deref(), Some("1..10"));
    assert_eq!(err.hint.as_deref(), Some("试试 5"));
    assert_eq!(err.to_string(), "bad x");
    assert_eq!(RscmdError::runtime("E", "m").exit_code(), 1);
    assert_eq!(RscmdError::usage("E", "m").exit_code(), 2);
    assert_eq!(RscmdError::not_found("E", "m").exit_code(), 4);
    assert_eq!(RscmdError::check_failed("E", "m").exit_code(), 5);
}

#[test]
fn clap_error_becomes_usage_error() {
    let err = Command::new("x")
        .arg(clap::Arg::new("need").required(true))
        .try_get_matches_from(["x"])
        .unwrap_err();
    let mapped = RscmdError::from_clap(err);
    assert_eq!(mapped.kind, ErrorKind::Usage);
    assert_eq!(mapped.code, "USAGE");
}

#[test]
fn tool_meta_json_is_stable() {
    let tool = &TOOLS[0];
    let meta = rscmd_core::tool::meta_json(tool);
    assert_eq!(meta["name"], "alpha");
    assert_eq!(meta["aliases"], json!(["a"]));
    assert_eq!(meta["side_effects"], "none");
    assert_eq!(meta["python"], "structured");
    assert_eq!(meta["gui"], false);
    assert_eq!(tool.meta.side_effects.as_str(), "none");
    assert_eq!(tool.meta.python.as_str(), "structured");
    assert_eq!(SideEffects::Read.as_str(), "read");
    assert_eq!(SideEffects::Write.as_str(), "write");
    assert_eq!(SideEffects::Network.as_str(), "network");
    assert_eq!(SideEffects::Destructive.as_str(), "destructive");
    assert_eq!(PythonExposure::Hidden.as_str(), "hidden");
    assert_eq!(PythonExposure::ArgvOnly.as_str(), "argv_only");
}

#[test]
fn render_json_success_goes_to_stdout() {
    let (stdout, stderr) = render(&outcome("alpha", Ok(json!({"text": "hi"})), true));
    assert!(stderr.is_empty());
    let envelope: Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(envelope["schema"], "rscmd-result/1");
    assert_eq!(envelope["data"]["text"], "hi");
}

#[test]
fn render_human_prefers_text_then_json_then_pretty() {
    let (stdout, _) = render(&outcome("t", Ok(json!({"text": "hello"})), false));
    assert_eq!(stdout, "hello\n");

    let (stdout, _) = render(&outcome("t", Ok(json!({"json": "{\"a\":1}"})), false));
    assert_eq!(stdout, "{\"a\":1}\n");

    let (stdout, _) = render(&outcome("t", Ok(json!({"value": 1})), false));
    assert!(stdout.contains("\"value\": 1"), "{stdout}");
}

#[test]
fn render_quiet_suppresses_success() {
    let mut result = outcome("t", Ok(json!({"text": "hello"})), false);
    result.quiet = true;
    let (stdout, stderr) = render(&result);
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
}

#[test]
fn render_error_human_includes_context() {
    let err = RscmdError::invalid_input("INVALID_X", "bad x")
        .with_field("x")
        .with_expected("1..10")
        .with_hint("试试 5");
    let (stdout, stderr) = render(&outcome("t", Err(err), false));
    assert!(stdout.is_empty());
    assert!(stderr.contains("错误[INVALID_X]: bad x"));
    assert!(stderr.contains("字段: x"));
    assert!(stderr.contains("期望: 1..10"));
    assert!(stderr.contains("提示: 试试 5"));
}

#[test]
fn render_error_json_goes_to_stderr() {
    let (stdout, stderr) = render(&outcome(
        "t",
        Err(RscmdError::not_found("TOOL_NOT_FOUND", "nope")),
        true,
    ));
    assert!(stdout.is_empty());
    let envelope: Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(envelope["schema"], "rscmd-error/1");
    assert_eq!(envelope["exit_code"], 4);
}

#[test]
fn dispatch_runs_tool_and_alias() {
    let out = dispatch(&TOOLS, &argv(&["--json", "alpha", "hi"]));
    assert_eq!(out.exit_code(), 0);
    assert_eq!(out.result.unwrap()["text"], "hi");
    assert!(out.json);

    let out = dispatch(&TOOLS, &argv(&["a", "hi"]));
    assert_eq!(out.result.unwrap()["text"], "hi");
    assert!(!out.json);
}

#[test]
fn dispatch_global_flags_after_subcommand() {
    let out = dispatch(
        &TOOLS,
        &argv(&[
            "alpha",
            "hi",
            "--json",
            "--quiet",
            "--dry-run",
            "--confirm",
            "--workers",
            "2",
            "--limit",
            "5",
        ]),
    );
    assert_eq!(out.exit_code(), 0);
    assert!(out.json);
    assert!(out.quiet);
}

#[test]
fn dispatch_builtins() {
    let out = dispatch(&TOOLS, &argv(&["tools"]));
    assert_eq!(out.result.unwrap().as_array().unwrap().len(), 2);

    let out = dispatch(&TOOLS, &argv(&["tools", "--for-llm"]));
    let items = out.result.unwrap();
    assert_eq!(items.as_array().unwrap().len(), 1);
    assert_eq!(items[0]["name"], "alpha");

    let out = dispatch(&TOOLS, &argv(&["info", "a"]));
    assert_eq!(out.result.unwrap()["name"], "alpha");

    let out = dispatch(&TOOLS, &argv(&["info", "missing"]));
    assert_eq!(out.result.unwrap_err().code, "TOOL_NOT_FOUND");

    let out = dispatch(&TOOLS, &argv(&["schema"]));
    assert_eq!(out.result.unwrap().as_array().unwrap().len(), 2);

    let out = dispatch(&TOOLS, &argv(&["version"]));
    assert_eq!(out.result.unwrap()["name"], "rscmd");

    let out = dispatch(&TOOLS, &argv(&["doctor"]));
    assert_eq!(out.result.unwrap()["tool_count"], 2);
}

#[test]
fn dispatch_reports_missing_and_unknown_commands() {
    let out = dispatch(&TOOLS, &argv(&[]));
    assert_eq!(out.exit_code(), 2);
    assert_eq!(out.result.unwrap_err().code, "MISSING_COMMAND");

    let out = dispatch(&TOOLS, &argv(&["nope"]));
    assert_eq!(out.exit_code(), 2);
    assert_eq!(out.result.unwrap_err().code, "USAGE");
}

#[test]
fn dispatch_bad_tool_arguments_are_usage_errors() {
    let out = dispatch(&TOOLS, &argv(&["alpha"]));
    assert_eq!(out.exit_code(), 2);
    assert_eq!(out.result.unwrap_err().kind, ErrorKind::Usage);
}

#[test]
fn dispatch_help_and_version_are_success() {
    let out = dispatch(&TOOLS, &argv(&["--version"]));
    assert_eq!(out.exit_code(), 0);
    let text = out.result.unwrap()["text"].as_str().unwrap().to_string();
    assert!(text.contains(env!("CARGO_PKG_VERSION")), "{text}");

    let out = dispatch(&TOOLS, &argv(&["--help"]));
    assert_eq!(out.exit_code(), 0);
    assert!(out.result.unwrap()["text"]
        .as_str()
        .unwrap()
        .contains("Usage"));
}

#[test]
fn dispatch_respects_raw_json_on_parse_error() {
    let out = dispatch(&TOOLS, &argv(&["--json", "--nope"]));
    assert_eq!(out.exit_code(), 2);
    assert!(out.json);
}
