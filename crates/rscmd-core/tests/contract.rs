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

static META_PROBE: ToolMeta = ToolMeta {
    name: "probe",
    aliases: &[],
    summary: "回显运行上下文",
    side_effects: SideEffects::None,
    gui: false,
    python: PythonExposure::Hidden,
    examples: &["rscmd probe"],
};

fn build_probe() -> Command {
    Command::new("probe")
}

fn run_probe(ctx: &mut ToolCtx, _matches: &ArgMatches) -> Result<Value, RscmdError> {
    Ok(json!({
        "timeout_ms": ctx.timeout.map(|value| value.as_millis()).unwrap_or(0),
        "workers": ctx.workers,
        "verbose": ctx.verbose,
    }))
}

static PROBE_TOOLS: [ToolDef; 1] = [ToolDef {
    meta: &META_PROBE,
    build: build_probe,
    run: run_probe,
    run_json: None,
    schema: None,
}];

fn argv(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_string()).collect()
}

fn outcome(tool: &str, result: Result<Value, RscmdError>, json_mode: bool) -> Outcome {
    Outcome {
        tool: tool.to_string(),
        result,
        json: json_mode,
        quiet: false,
        verbose: false,
        elapsed_ms: 0,
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
fn registry_rejects_builtin_command_name() {
    static RUN: ToolMeta = ToolMeta {
        name: "run",
        aliases: &[],
        summary: "占用内建命令名",
        side_effects: SideEffects::None,
        gui: false,
        python: PythonExposure::Hidden,
        examples: &["rscmd run"],
    };
    static RUN_TOOLS: [ToolDef; 1] = [ToolDef {
        meta: &RUN,
        build: build_beta,
        run: run_beta,
        run_json: None,
        schema: None,
    }];
    let problems = Registry::new(&RUN_TOOLS).validate().unwrap_err();
    assert!(
        problems.iter().any(|item| item.contains("冲突")),
        "{problems:?}"
    );
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
fn render_verbose_traces_to_stderr_only() {
    let mut result = outcome("t", Ok(json!({"text": "hello"})), false);
    result.verbose = true;
    result.elapsed_ms = 7;
    let (stdout, stderr) = render(&result);
    assert_eq!(stdout, "hello\n");
    assert_eq!(stderr, "[rscmd] tool=t elapsed=7ms\n");

    // JSON 模式下 stdout 仍是纯信封，跟踪只进 stderr。
    let mut result = outcome("version", Ok(json!({"name": "rscmd"})), true);
    result.verbose = true;
    let (stdout, stderr) = render(&result);
    let envelope: Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(envelope["schema"], "rscmd-result/1");
    assert!(stderr.starts_with("[rscmd] tool=version"));
    assert!(stderr.contains("elapsed="));
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
fn dispatch_run_subcommand_runs_tool() {
    // 新风格：rscmd run <工具>
    let out = dispatch(&TOOLS, &argv(&["run", "alpha", "hello"]));
    assert_eq!(out.exit_code(), 0);
    assert_eq!(out.tool, "alpha");
    assert_eq!(out.result.unwrap()["text"], "hello");

    // run 短别名 r
    let out = dispatch(&TOOLS, &argv(&["r", "alpha", "hello"]));
    assert_eq!(out.exit_code(), 0);
    assert_eq!(out.tool, "alpha");
}

#[test]
fn dispatch_short_aliases_for_top_level() {
    // version 短别名 v
    let out = dispatch(&TOOLS, &argv(&["v"]));
    assert_eq!(out.exit_code(), 0);
    assert_eq!(out.tool, "version");
    assert!(out.result.unwrap()["version"]
        .as_str()
        .unwrap()
        .contains("0.1"));

    // doctor 短别名 d
    let out = dispatch(&TOOLS, &argv(&["d"]));
    assert_eq!(out.exit_code(), 0);
    assert_eq!(out.tool, "doctor");

    // info 短别名 i
    let out = dispatch(&TOOLS, &argv(&["i", "alpha"]));
    assert_eq!(out.exit_code(), 0);
    assert_eq!(out.tool, "info");
    assert_eq!(out.result.unwrap()["name"], "alpha");
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
fn dispatch_enables_verbose_anywhere() {
    assert!(dispatch(&TOOLS, &argv(&["--verbose", "version"])).verbose);
    assert!(dispatch(&TOOLS, &argv(&["version", "-v"])).verbose);
    assert!(!dispatch(&TOOLS, &argv(&["version"])).verbose);
}

#[test]
fn dispatch_propagates_timeout_to_ctx() {
    let out = dispatch(&PROBE_TOOLS, &argv(&["probe"]));
    assert_eq!(out.exit_code(), 0);
    assert_eq!(out.result.unwrap()["timeout_ms"], 0);

    let out = dispatch(&PROBE_TOOLS, &argv(&["--timeout", "5", "probe"]));
    assert_eq!(out.result.unwrap()["timeout_ms"], 5000);

    // 全局参数放在子命令之后同样生效
    let out = dispatch(&PROBE_TOOLS, &argv(&["probe", "--timeout", "3"]));
    assert_eq!(out.result.unwrap()["timeout_ms"], 3000);

    // run 嵌套入口也写入同一个 ctx
    let out = dispatch(&PROBE_TOOLS, &argv(&["run", "probe", "--timeout", "2"]));
    assert_eq!(out.result.unwrap()["timeout_ms"], 2000);
}

#[test]
fn scan_flags_reads_raw_output_modes() {
    assert_eq!(
        rscmd_core::scan_flags(&argv(&["--json", "--quiet", "-v", "case"])),
        rscmd_core::RawFlags {
            json: true,
            quiet: true,
            verbose: true
        }
    );
    assert_eq!(
        rscmd_core::scan_flags(&argv(&["case"])),
        rscmd_core::RawFlags::default()
    );
}

#[test]
fn interrupted_outcome_exits_130_and_keeps_envelope() {
    let json_flags = rscmd_core::RawFlags {
        json: true,
        quiet: true,
        verbose: true,
    };
    let outcome = Outcome::interrupted(&json_flags);
    assert_eq!(outcome.exit_code(), 130);
    // 中断信息必须可见，不能被 --quiet 吞掉，也不追加 verbose 跟踪。
    assert!(!outcome.quiet);
    assert!(!outcome.verbose);

    let (stdout, stderr) = render(&outcome);
    assert!(stdout.is_empty());
    let envelope: Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(envelope["schema"], "rscmd-error/1");
    assert_eq!(envelope["kind"], "interrupted");
    assert_eq!(envelope["code"], "INTERRUPTED");
    assert_eq!(envelope["exit_code"], 130);

    let human = Outcome::interrupted(&rscmd_core::RawFlags::default());
    let (stdout, stderr) = render(&human);
    assert!(stdout.is_empty());
    assert!(stderr.contains("错误[INTERRUPTED]"), "{stderr}");
    assert!(stderr.contains("中断"), "{stderr}");
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
    let data = out.result.unwrap();
    assert_eq!(data["schema"], "rscmd-schema/1");
    assert_eq!(data["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(data["tools"].as_array().unwrap().len(), 2);
    let builtins = data["builtins"].as_array().unwrap();
    assert!(builtins.contains(&json!("doctor")));
    assert_eq!(data["global"]["title"], "GlobalArgs");
    assert!(data["global"]["properties"].is_object());

    let out = dispatch(&TOOLS, &argv(&["version"]));
    assert_eq!(out.result.unwrap()["name"], "rscmd");

    let out = dispatch(&TOOLS, &argv(&["doctor"]));
    assert_eq!(out.result.unwrap()["tool_count"], 2);
}

#[test]
fn dispatch_reports_missing_and_unknown_commands() {
    // 无参数 → 正常帮助（exit=0），不是 USAGE 错误
    let out = dispatch(&TOOLS, &argv(&[]));
    assert_eq!(out.exit_code(), 0);
    assert!(out.result.is_ok());

    // 未知命令 → USAGE 错误（exit=2）
    let out = dispatch(&TOOLS, &argv(&["nope"]));
    assert_eq!(out.exit_code(), 2);
    assert_eq!(out.result.unwrap_err().code, "USAGE");

    // `rscmd run` 不带工具名 → USAGE 错误
    let out = dispatch(&TOOLS, &argv(&["run"]));
    assert_eq!(out.exit_code(), 2);
    assert_eq!(out.result.unwrap_err().code, "MISSING_TOOL");
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

#[test]
fn doctor_data_reports_checks_and_environment() {
    let data = rscmd_core::builtins::doctor_data(&TOOLS, Registry::new(&TOOLS).validate());
    assert_eq!(data["ok"], true);
    assert_eq!(data["total"], 3);
    assert_eq!(data["passed"], 3);
    assert_eq!(data["failed"], 0);
    assert_eq!(data["tool_count"], 2);
    assert_eq!(data["checks"].as_array().unwrap().len(), 3);
    assert!(data["checks"][0]["detail"]
        .as_str()
        .unwrap()
        .contains("2 个工具"));
}

#[test]
fn doctor_flags_broken_registry_and_empty_tools() {
    let data = rscmd_core::builtins::doctor_data(&TOOLS, Err(vec!["别名冲突".to_string()]));
    assert_eq!(data["ok"], false);
    assert_eq!(data["failed"], 1);
    assert_eq!(data["checks"][0]["fix"], "别名冲突");
    assert!(rscmd_core::builtins::check_failed("doctor", &data));
    assert!(!rscmd_core::builtins::check_failed("version", &data));

    let out = Outcome {
        tool: "doctor".to_string(),
        result: Ok(data),
        json: false,
        quiet: false,
        verbose: false,
        elapsed_ms: 0,
    };
    assert_eq!(out.exit_code(), 5);

    let empty = rscmd_core::builtins::doctor_data(&[], Ok(()));
    assert_eq!(empty["ok"], false);
    assert_eq!(empty["checks"][1]["item"], "工具数量");
    assert_eq!(empty["checks"][1]["ok"], false);
}

#[test]
fn render_builtin_human_forms() {
    let version = rscmd_core::builtins::version_data();
    let (stdout, stderr) = render(&outcome("version", Ok(version), false));
    assert_eq!(stdout, format!("rscmd {}\n", env!("CARGO_PKG_VERSION")));
    assert!(stderr.is_empty());

    let data = rscmd_core::builtins::doctor_data(&TOOLS, Registry::new(&TOOLS).validate());
    let (stdout, stderr) = render(&outcome("doctor", Ok(data), false));
    assert!(stdout.contains("诊断结果: 3/3 全部通过"), "{stdout}");
    assert!(stdout.contains("[OK  ]"), "{stdout}");
    assert!(stderr.is_empty());
}

#[test]
fn doctor_text_handles_sparse_data() {
    let text = rscmd_core::builtins::doctor_text(&json!({
        "checks": [{"item": "x", "ok": false}]
    }));
    assert!(text.contains("unknown"), "{text}");
    assert!(text.contains("[FAIL] x: "), "{text}");
    assert!(!text.contains("修复:"), "{text}");
    assert!(text.contains("诊断结果: 0/0"), "{text}");
}

#[test]
fn doctor_failure_is_check_failed_not_runtime_error() {
    let bad = rscmd_core::builtins::doctor_data(&TOOLS, Err(vec!["x".to_string()]));

    let (stdout, stderr) = render(&outcome("doctor", Ok(bad.clone()), false));
    assert!(stdout.contains("[FAIL]"), "{stdout}");
    assert!(stdout.contains("修复:"), "{stdout}");
    assert!(stderr.is_empty());

    // JSON 模式：结论性失败仍写 stdout 信封，不写 stderr 错误体。
    let (stdout, stderr) = render(&outcome("doctor", Ok(bad), true));
    let envelope: Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(envelope["schema"], "rscmd-result/1");
    assert_eq!(envelope["ok"], false);
    assert!(stderr.is_empty());
}
