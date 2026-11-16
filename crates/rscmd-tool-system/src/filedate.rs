//! `filedate`：读取或写入文件修改时间（mtime）。
//!
//! 读模式输出 `{path, mtime_unix, mtime_ns, rfc3339}`：`mtime_unix` 为整秒（可为负），
//! `mtime_ns` 为秒内纳秒分量（0–999,999,999），`rfc3339` 一律按 UTC 渲染，不读取系统时区。
//!
//! 写模式用 `--set <UNIX秒|RFC3339>` 指定目标时间，必须显式 `--confirm`；
//! `--dry-run` 只报告计划值、不改动文件。写入后重新读取 mtime 作为结果，避免文件系统
//! 的时间粒度导致“报告值与实际值不一致”。Unix 秒与 RFC3339 都不含本地时区语义，
//! 因此跨机器结果一致。

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::{ArgMatches, Command};
use rscmd_core::{
    command_for, PythonExposure, RscmdError, SideEffects, ToolCtx, ToolDef, ToolMeta,
};
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};

pub static META: ToolMeta = ToolMeta {
    name: "filedate",
    aliases: &["filedatetool"],
    summary: "读取或写入文件修改时间（写需 --confirm，时间按 UTC 渲染）",
    side_effects: SideEffects::Write,
    gui: false,
    python: PythonExposure::Structured,
    examples: &[
        "rscmd filedate Cargo.toml",
        "rscmd filedate notes.txt --set 1700000000 --confirm",
        "rscmd filedate notes.txt --set 2023-11-14T22:13:20Z --dry-run",
    ],
};

#[derive(Debug, clap::Args, serde::Deserialize, schemars::JsonSchema)]
pub struct FiledateArgs {
    /// 目标文件路径
    #[arg(value_name = "PATH")]
    pub path: String,
    /// 写入修改时间：Unix 秒（可为负数）或 RFC3339；不给出则只读取
    #[arg(long, value_name = "VALUE", allow_hyphen_values = true)]
    #[serde(default, deserialize_with = "deserialize_set")]
    pub set: Option<String>,
}

/// CLI 侧 `--set` 恒为字符串；JSON 入口允许直接给 Unix 秒整数，这里统一成字符串。
fn deserialize_set<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;

    match Option::<Value>::deserialize(deserializer)? {
        None => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),
        Some(Value::Number(number)) => Ok(Some(number.to_string())),
        Some(other) => Err(serde::de::Error::custom(format!(
            "set 需要字符串或整数，收到 {other}"
        ))),
    }
}

pub fn build() -> Command {
    command_for::<FiledateArgs>(&META)
}

pub fn run_cli(ctx: &mut ToolCtx, matches: &ArgMatches) -> Result<Value, RscmdError> {
    let args = <FiledateArgs as clap::FromArgMatches>::from_arg_matches(matches)
        .map_err(RscmdError::from_clap)?;
    run(ctx, args)
}

pub fn run_json(ctx: &mut ToolCtx, input: &Value) -> Result<Value, RscmdError> {
    let args: FiledateArgs = serde_json::from_value(input.clone())
        .map_err(|err| RscmdError::coded("INVALID_ARGS", err.to_string()).with_field("args"))?;
    run(ctx, args)
}

fn run(ctx: &mut ToolCtx, args: FiledateArgs) -> Result<Value, RscmdError> {
    let path = Path::new(&args.path);
    let Some(spec) = args.set.as_deref() else {
        let modified = read_mtime(path)?;
        return render(&args.path, modified, None);
    };

    let target = parse_target(spec)?;
    ctx.require_confirm("写入文件修改时间")?;
    if ctx.dry_run {
        return render(&args.path, target, Some(true));
    }

    write_mtime(path, target)?;
    render(&args.path, read_mtime(path)?, Some(false))
}

/// 输出统一字段；写模式（含 dry-run）追加 `dry_run`。
fn render(path: &str, time: OffsetDateTime, dry_run: Option<bool>) -> Result<Value, RscmdError> {
    let utc = time.to_offset(UtcOffset::UTC);
    let rfc3339 = utc.format(&Rfc3339).map_err(|err| {
        RscmdError::coded("IO", format!("文件修改时间无法按 RFC3339 渲染: {err}"))
            .with_field("path")
            .with_expected("0–9999 年之间的时间")
    })?;
    let total = utc.unix_timestamp_nanos();
    let seconds = total.div_euclid(1_000_000_000);
    let nanos = total.rem_euclid(1_000_000_000);
    let mut out = json!({
        "path": path,
        "mtime_unix": i64::try_from(seconds).unwrap_or(i64::MAX),
        "mtime_ns": u32::try_from(nanos).unwrap_or(0),
        "rfc3339": rfc3339,
    });
    if let Some(flag) = dry_run {
        out["dry_run"] = json!(flag);
    }
    Ok(out)
}

fn read_mtime(path: &Path) -> Result<OffsetDateTime, RscmdError> {
    let metadata = fs::metadata(path).map_err(io_error)?;
    offset_from_system_time(metadata.modified().map_err(io_error)?)
}

fn write_mtime(path: &Path, target: OffsetDateTime) -> Result<(), RscmdError> {
    let file = fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(io_error)?;
    file.set_modified(system_time_from(target)?)
        .map_err(io_error)
}

/// `--set` 支持 Unix 秒与 RFC3339；两者都是绝对时刻，不涉及本地时区。
fn parse_target(spec: &str) -> Result<OffsetDateTime, RscmdError> {
    let text = spec.trim();
    let invalid = || {
        RscmdError::coded("INVALID_ARGS", format!("无法解析修改时间: {spec}"))
            .with_field("set")
            .with_expected("Unix 秒或 RFC3339 时间")
    };
    let instant = match text.parse::<i64>() {
        Ok(seconds) => OffsetDateTime::from_unix_timestamp(seconds).map_err(|_| invalid())?,
        Err(_) => OffsetDateTime::parse(text, &Rfc3339).map_err(|_| invalid())?,
    };
    // 提前校验 RFC3339 可表示范围，避免写入后无法渲染。
    instant.format(&Rfc3339).map_err(|_| invalid())?;
    Ok(instant)
}

fn offset_from_system_time(time: SystemTime) -> Result<OffsetDateTime, RscmdError> {
    let total = match time.duration_since(UNIX_EPOCH) {
        Ok(elapsed) => {
            i128::from(elapsed.as_secs()) * 1_000_000_000 + i128::from(elapsed.subsec_nanos())
        }
        Err(err) => {
            let before = err.duration();
            -i128::from(before.as_secs()) * 1_000_000_000 - i128::from(before.subsec_nanos())
        }
    };
    OffsetDateTime::from_unix_timestamp_nanos(total).map_err(|err| {
        RscmdError::coded("IO", format!("文件修改时间超出可表示范围: {err}")).with_field("path")
    })
}

fn system_time_from(target: OffsetDateTime) -> Result<SystemTime, RscmdError> {
    let total = target.unix_timestamp_nanos();
    let magnitude = total.unsigned_abs();
    let seconds = u64::try_from(magnitude / 1_000_000_000).unwrap_or(u64::MAX);
    let nanos = (magnitude % 1_000_000_000) as u32;
    let offset = Duration::new(seconds, nanos);
    let time = if total >= 0 {
        UNIX_EPOCH.checked_add(offset)
    } else {
        UNIX_EPOCH.checked_sub(offset)
    };
    time.ok_or_else(|| {
        RscmdError::coded("INVALID_ARGS", "目标时间超出系统时间可表示范围")
            .with_field("set")
            .with_expected("更接近当前的时间")
    })
}

fn io_error(err: std::io::Error) -> RscmdError {
    RscmdError::coded("IO", format!("文件操作失败: {err}"))
        .with_field("path")
        .with_expected("可访问且可写的文件路径")
}

pub fn def() -> ToolDef {
    ToolDef {
        meta: &META,
        build,
        run: run_cli,
        run_json: Some(run_json),
        schema: Some(filedate_args_schema),
    }
}

fn filedate_args_schema() -> String {
    let schema = schemars::schema_for!(FiledateArgs);
    serde_json::to_string_pretty(&schema).expect("FiledateArgs schema 可序列化")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rscmd_runtime::fs::temp_dir;

    fn matches(args: &[&str]) -> ArgMatches {
        build().try_get_matches_from(args).expect("参数应可解析")
    }

    fn stamp_file() -> (rscmd_runtime::fs::TempDir, String) {
        let dir = temp_dir().expect("应创建临时目录");
        let path = dir.path().join("stamp.txt");
        fs::write(&path, "x").expect("应写入临时文件");
        let display = path.to_string_lossy().into_owned();
        (dir, display)
    }

    #[test]
    fn reads_mtime_with_utc_rendering() {
        let (_dir, path) = stamp_file();
        let out = run_json(&mut ToolCtx::default(), &json!({"path": path})).unwrap();
        assert_eq!(out["path"], path);
        assert!(out["mtime_unix"].as_i64().unwrap() > 0);
        assert!(out["mtime_ns"].as_u64().unwrap() < 1_000_000_000);
        let rfc3339 = out["rfc3339"].as_str().expect("应有 rfc3339");
        assert!(rfc3339.ends_with('Z'), "{rfc3339}");
        assert!(!out.as_object().unwrap().contains_key("dry_run"));
    }

    #[test]
    fn writes_whole_second_mtime_after_confirm() {
        let (_dir, path) = stamp_file();
        let mut ctx = ToolCtx {
            confirm: true,
            ..ToolCtx::default()
        };
        let out = run_json(&mut ctx, &json!({"path": path, "set": 1700000000})).unwrap();
        assert_eq!(out["mtime_unix"], 1700000000);
        assert_eq!(out["mtime_ns"], 0);
        assert_eq!(out["rfc3339"], "2023-11-14T22:13:20Z");
        assert_eq!(out["dry_run"], json!(false));

        let read = run_json(&mut ToolCtx::default(), &json!({"path": path})).unwrap();
        assert_eq!(read["mtime_unix"], 1700000000);
    }

    #[test]
    fn write_requires_confirm() {
        let (_dir, path) = stamp_file();
        let before = run_json(&mut ToolCtx::default(), &json!({"path": path})).unwrap();
        let err = run_json(
            &mut ToolCtx::default(),
            &json!({"path": path, "set": 1700000000}),
        )
        .unwrap_err();
        assert_eq!(err.code, "CONFIRM_REQUIRED");
        assert_eq!(err.exit_code(), 2);
        let after = run_json(&mut ToolCtx::default(), &json!({"path": path})).unwrap();
        assert_eq!(before["mtime_unix"], after["mtime_unix"]);
        assert_eq!(before["mtime_ns"], after["mtime_ns"]);
    }

    #[test]
    fn dry_run_reports_plan_without_touching_file() {
        let (_dir, path) = stamp_file();
        let before = run_json(&mut ToolCtx::default(), &json!({"path": path})).unwrap();
        let mut ctx = ToolCtx {
            dry_run: true,
            ..ToolCtx::default()
        };
        let out = run_json(&mut ctx, &json!({"path": path, "set": 1700000000})).unwrap();
        assert_eq!(out["dry_run"], json!(true));
        assert_eq!(out["mtime_unix"], 1700000000);
        assert_eq!(out["rfc3339"], "2023-11-14T22:13:20Z");
        let after = run_json(&mut ToolCtx::default(), &json!({"path": path})).unwrap();
        assert_eq!(before["mtime_unix"], after["mtime_unix"]);
    }

    #[test]
    fn rfc3339_target_is_normalized_to_utc() {
        let (_dir, path) = stamp_file();
        let mut ctx = ToolCtx {
            confirm: true,
            ..ToolCtx::default()
        };
        let out = run_json(
            &mut ctx,
            &json!({"path": path, "set": "2023-11-15T06:13:20+08:00"}),
        )
        .unwrap();
        assert_eq!(out["mtime_unix"], 1700000000);
        assert_eq!(out["rfc3339"], "2023-11-14T22:13:20Z");
    }

    #[test]
    fn negative_unix_target_is_supported() {
        let parsed = parse_target("-1").unwrap();
        assert_eq!(parsed.unix_timestamp(), -1);
        assert_eq!(parsed.format(&Rfc3339).unwrap(), "1969-12-31T23:59:59Z");
    }

    #[test]
    fn invalid_set_is_invalid_args() {
        for spec in ["not-a-time", "", "2023-13-01T00:00:00Z", "1.5"] {
            let err = parse_target(spec).unwrap_err();
            assert_eq!(err.code, "INVALID_ARGS", "{spec}");
            assert_eq!(err.field.as_deref(), Some("set"), "{spec}");
            assert_eq!(err.exit_code(), 3, "{spec}");
        }
    }

    #[test]
    fn missing_path_is_io_error() {
        let (_dir, path) = stamp_file();
        let missing = format!("{path}.missing");
        let err = run_json(&mut ToolCtx::default(), &json!({"path": missing})).unwrap_err();
        assert_eq!(err.code, "IO");
        assert_eq!(err.exit_code(), 1);
        assert_eq!(err.field.as_deref(), Some("path"));
    }

    #[test]
    fn cli_and_json_paths_agree() {
        let (_dir, path) = stamp_file();
        let cli = run_cli(
            &mut ToolCtx::default(),
            &matches(&["filedate", path.as_str()]),
        )
        .unwrap();
        let json_out = run_json(&mut ToolCtx::default(), &json!({"path": path})).unwrap();
        assert_eq!(cli, json_out);
    }

    #[test]
    fn cli_write_accepts_negative_set_values() {
        let (_dir, path) = stamp_file();
        let mut ctx = ToolCtx {
            confirm: true,
            ..ToolCtx::default()
        };
        let out = run_cli(
            &mut ctx,
            &matches(&["filedate", path.as_str(), "--set", "-1"]),
        )
        .unwrap();
        assert_eq!(out["mtime_unix"], -1);
        assert_eq!(out["rfc3339"], "1969-12-31T23:59:59Z");
    }

    #[test]
    fn json_entry_requires_path() {
        let err = run_json(&mut ToolCtx::default(), &json!({})).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("args"));
    }

    #[test]
    fn json_entry_rejects_non_scalar_set() {
        let (_dir, path) = stamp_file();
        let err =
            run_json(&mut ToolCtx::default(), &json!({"path": path, "set": true})).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("args"));
    }

    #[test]
    fn json_entry_treats_null_set_as_read() {
        let (_dir, path) = stamp_file();
        let out = run_json(&mut ToolCtx::default(), &json!({"path": path, "set": null})).unwrap();
        assert!(out["mtime_unix"].as_i64().unwrap() > 0);
        assert!(!out.as_object().unwrap().contains_key("dry_run"));
    }

    #[test]
    fn render_rejects_year_outside_rfc3339() {
        // year -9999..=9999 都能构造，但 RFC3339 只接受 0–9999 年。
        let ancient = OffsetDateTime::from_unix_timestamp(-100_000_000_000).unwrap();
        let err = render("x", ancient, None).unwrap_err();
        assert_eq!(err.code, "IO");
        assert_eq!(err.field.as_deref(), Some("path"));
    }

    #[test]
    fn offset_from_system_time_rejects_far_future() {
        // SystemTime 在 Windows 上可到 30828 年，而 time 只到 9999 年。
        let far = UNIX_EPOCH + Duration::from_secs(300_000_000_000);
        let err = offset_from_system_time(far).unwrap_err();
        assert_eq!(err.code, "IO");
        assert_eq!(err.field.as_deref(), Some("path"));
    }

    #[cfg(windows)]
    #[test]
    fn system_time_from_rejects_pre_1601_on_windows() {
        let ancient = OffsetDateTime::from_unix_timestamp(-50_000_000_000).unwrap();
        let err = system_time_from(ancient).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("set"));
    }

    #[test]
    fn def_exposes_schema_and_metadata() {
        let tool = def();
        assert_eq!(tool.meta.name, "filedate");
        assert_eq!(tool.meta.aliases, &["filedatetool"]);
        assert_eq!(tool.meta.side_effects, SideEffects::Write);
        assert!(tool.run_json.is_some());
        let schema = (tool.schema.expect("应提供 schema"))();
        for field in ["\"path\"", "\"set\""] {
            assert!(schema.contains(field), "{field} 不在 schema: {schema}");
        }
    }
}
