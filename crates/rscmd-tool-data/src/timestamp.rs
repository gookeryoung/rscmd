//! `timestamp`：Unix 秒与 RFC3339 互转，支持固定偏移时区与日期加减。
//!
//! 时区一律来自显式 `--offset`（`+08:00`、`-05:30`、`Z`/`utc`），不读取系统时区，
//! 也不做隐式本地转换，保证跨机器结果一致。`--add` 的持续时长按固定秒数换算
//! （`1d` = 86400 秒），因此不涉及夏令时。
//!
//! 输入可以是 Unix 秒（含负数）或 RFC3339；输出统一给出 `unix`、按 `--offset`
//! 渲染的 `rfc3339` 与规范化后的 `offset` 标签。

use clap::{ArgMatches, Command};
use rscmd_core::{
    command_for, PythonExposure, RscmdError, SideEffects, ToolCtx, ToolDef, ToolMeta,
};
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime, UtcOffset};

pub static META: ToolMeta = ToolMeta {
    name: "timestamp",
    aliases: &["timestamptool"],
    summary: "Unix 秒与 RFC3339 互转，支持固定偏移时区与日期加减",
    side_effects: SideEffects::None,
    gui: false,
    python: PythonExposure::Structured,
    examples: &[
        "rscmd timestamp 1700000000 --offset +08:00",
        "rscmd timestamp 2024-02-28T12:00:00Z --add 1d",
    ],
};

#[derive(Debug, clap::Args, serde::Deserialize, schemars::JsonSchema)]
pub struct TimestampArgs {
    /// Unix 秒（可为负数）或 RFC3339 时间
    #[arg(value_name = "VALUE", allow_negative_numbers = true)]
    pub input: String,
    /// 输出时区偏移：`+08:00`、`-05:30`、`Z` 或 `utc`；不读系统时区
    #[arg(
        long,
        value_name = "OFFSET",
        default_value = "+00:00",
        allow_hyphen_values = true
    )]
    #[serde(default = "default_offset")]
    pub offset: String,
    /// 加减时长：`1d`、`-2h`、`30m`、`1w`（单位 s/m/h/d/w，按固定秒数换算）
    #[arg(long, value_name = "DURATION", allow_hyphen_values = true)]
    #[serde(default)]
    pub add: Option<String>,
}

fn default_offset() -> String {
    "+00:00".to_string()
}

pub fn build() -> Command {
    command_for::<TimestampArgs>(&META)
}

pub fn run_cli(ctx: &mut ToolCtx, matches: &ArgMatches) -> Result<Value, RscmdError> {
    let args = <TimestampArgs as clap::FromArgMatches>::from_arg_matches(matches)
        .map_err(RscmdError::from_clap)?;
    run(ctx, args)
}

pub fn run_json(ctx: &mut ToolCtx, input: &Value) -> Result<Value, RscmdError> {
    let args: TimestampArgs = serde_json::from_value(input.clone())
        .map_err(|err| RscmdError::coded("INVALID_ARGS", err.to_string()).with_field("args"))?;
    run(ctx, args)
}

fn run(_ctx: &mut ToolCtx, args: TimestampArgs) -> Result<Value, RscmdError> {
    let offset = parse_offset(&args.offset)?;
    let mut instant = parse_instant(&args.input)?;
    if let Some(spec) = args.add.as_deref() {
        let seconds = parse_duration(spec)?;
        instant = instant
            .checked_add(Duration::seconds(seconds))
            .ok_or_else(|| {
                RscmdError::coded("INVALID_ARGS", "加减结果超出可表示范围")
                    .with_field("add")
                    .with_expected("结果仍在时间可表示范围内")
            })?;
    }
    let rendered = instant.to_offset(offset).format(&Rfc3339).map_err(|_| {
        RscmdError::coded("INVALID_ARGS", "时间超出 RFC3339 可表示范围")
            .with_field("input")
            .with_expected("0–9999 年之间的时间")
    })?;
    Ok(json!({
        "unix": instant.unix_timestamp(),
        "rfc3339": rendered,
        "offset": offset_label(offset),
    }))
}

fn parse_instant(input: &str) -> Result<OffsetDateTime, RscmdError> {
    let text = input.trim();
    if let Ok(seconds) = text.parse::<i64>() {
        return OffsetDateTime::from_unix_timestamp(seconds).map_err(|err| {
            RscmdError::coded("INVALID_ARGS", format!("Unix 秒超出可表示范围: {err}"))
                .with_field("input")
                .with_expected("可表示的 Unix 秒或 RFC3339 时间")
        });
    }
    OffsetDateTime::parse(text, &Rfc3339).map_err(|err| {
        RscmdError::coded("INVALID_ARGS", format!("无法解析时间: {err}"))
            .with_field("input")
            .with_expected("Unix 秒或 RFC3339 时间")
    })
}

fn parse_offset(spec: &str) -> Result<UtcOffset, RscmdError> {
    let text = spec.trim();
    if text.eq_ignore_ascii_case("z") || text.eq_ignore_ascii_case("utc") {
        return Ok(UtcOffset::UTC);
    }
    let invalid = || {
        RscmdError::coded("INVALID_ARGS", format!("无法识别的时区偏移: {spec}"))
            .with_field("offset")
            .with_expected("+HH:MM、-HH:MM、Z 或 utc")
    };
    let bytes = text.as_bytes();
    if bytes.len() != 6 || (bytes[0] != b'+' && bytes[0] != b'-') || bytes[3] != b':' {
        return Err(invalid());
    }
    let hours: i8 = text[1..3].parse().map_err(|_| invalid())?;
    let minutes: i8 = text[4..6].parse().map_err(|_| invalid())?;
    if hours > 23 || minutes > 59 {
        return Err(invalid());
    }
    let sign = if bytes[0] == b'-' { -1 } else { 1 };
    UtcOffset::from_hms(sign * hours, sign * minutes, 0).map_err(|_| invalid())
}

fn offset_label(offset: UtcOffset) -> String {
    let total = offset.whole_seconds();
    let sign = if total < 0 { '-' } else { '+' };
    let absolute = total.abs();
    format!("{sign}{:02}:{:02}", absolute / 3600, (absolute % 3600) / 60)
}

fn parse_duration(spec: &str) -> Result<i64, RscmdError> {
    let invalid = || {
        RscmdError::coded("INVALID_ARGS", format!("无法识别的时长: {spec}"))
            .with_field("add")
            .with_expected("整数加单位，如 1d、-2h、30m、1w")
    };
    let text = spec.trim();
    let (sign, rest) = if let Some(rest) = text.strip_prefix('-') {
        (-1i64, rest)
    } else {
        (1, text.strip_prefix('+').unwrap_or(text))
    };
    let unit = rest.chars().last().ok_or_else(invalid)?;
    let number = &rest[..rest.len() - unit.len_utf8()];
    let value: i64 = number.parse().map_err(|_| invalid())?;
    let multiplier = match unit {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        'd' => 86400,
        'w' => 604800,
        _ => return Err(invalid()),
    };
    value
        .checked_mul(multiplier)
        .and_then(|seconds| seconds.checked_mul(sign))
        .ok_or_else(|| {
            RscmdError::coded("INVALID_ARGS", format!("时长超出范围: {spec}"))
                .with_field("add")
                .with_expected("较小的整数时长")
        })
}

pub fn def() -> ToolDef {
    ToolDef {
        meta: &META,
        build,
        run: run_cli,
        run_json: Some(run_json),
        schema: Some(timestamp_args_schema),
    }
}

fn timestamp_args_schema() -> String {
    let schema = schemars::schema_for!(TimestampArgs);
    serde_json::to_string_pretty(&schema).expect("TimestampArgs schema 可序列化")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(args: &[&str]) -> ArgMatches {
        build().try_get_matches_from(args).expect("参数应可解析")
    }

    fn run_value(input: &str, offset: &str, add: Option<&str>) -> Result<Value, RscmdError> {
        run(
            &mut ToolCtx::default(),
            TimestampArgs {
                input: input.to_string(),
                offset: offset.to_string(),
                add: add.map(str::to_string),
            },
        )
    }

    fn rfc3339(value: &Value) -> &str {
        value["rfc3339"].as_str().expect("应有 rfc3339")
    }

    #[test]
    fn converts_unix_seconds_to_rfc3339() {
        let out = run_value("0", "+00:00", None).unwrap();
        assert_eq!(out["unix"], 0);
        assert_eq!(rfc3339(&out), "1970-01-01T00:00:00Z");
        assert_eq!(out["offset"], "+00:00");
    }

    #[test]
    fn negative_unix_seconds_are_supported() {
        let out = run_value("-1", "+00:00", None).unwrap();
        assert_eq!(out["unix"], -1);
        assert_eq!(rfc3339(&out), "1969-12-31T23:59:59Z");
    }

    #[test]
    fn rfc3339_input_is_normalized_to_requested_offset() {
        let out = run_value("2023-11-14T22:13:20+08:00", "+00:00", None).unwrap();
        assert_eq!(rfc3339(&out), "2023-11-14T14:13:20Z");

        let out = run_value("2023-11-14T14:13:20Z", "+08:00", None).unwrap();
        assert_eq!(rfc3339(&out), "2023-11-14T22:13:20+08:00");
        assert_eq!(out["offset"], "+08:00");
    }

    #[test]
    fn negative_offsets_are_supported() {
        let out = run_value("2023-11-14T14:13:20Z", "-05:30", None).unwrap();
        assert_eq!(rfc3339(&out), "2023-11-14T08:43:20-05:30");
        assert_eq!(out["offset"], "-05:30");
    }

    #[test]
    fn z_and_utc_aliases_mean_utc() {
        for spec in ["Z", "utc", "UTC"] {
            let out = run_value("0", spec, None).unwrap();
            assert_eq!(out["offset"], "+00:00", "{spec}");
            assert_eq!(rfc3339(&out), "1970-01-01T00:00:00Z");
        }
    }

    #[test]
    fn adds_and_subtracts_across_leap_day() {
        let out = run_value("2024-02-28T12:00:00Z", "+00:00", Some("1d")).unwrap();
        assert_eq!(rfc3339(&out), "2024-02-29T12:00:00Z");

        let out = run_value("2024-03-01T00:30:00Z", "+00:00", Some("-2h")).unwrap();
        assert_eq!(rfc3339(&out), "2024-02-29T22:30:00Z");
    }

    #[test]
    fn duration_units_are_fixed_seconds() {
        let out = run_value("2024-01-01T00:00:00Z", "+00:00", Some("1w")).unwrap();
        assert_eq!(rfc3339(&out), "2024-01-08T00:00:00Z");

        let out = run_value("2024-01-01T00:00:00Z", "+00:00", Some("+90m")).unwrap();
        assert_eq!(rfc3339(&out), "2024-01-01T01:30:00Z");

        let out = run_value("2024-01-01T00:00:00Z", "+00:00", Some("30s")).unwrap();
        assert_eq!(rfc3339(&out), "2024-01-01T00:00:30Z");
    }

    #[test]
    fn rejects_invalid_instants() {
        let err = run_value("not-a-time", "+00:00", None).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("input"));

        let err = run_value("99999999999999999", "+00:00", None).unwrap_err();
        assert_eq!(err.field.as_deref(), Some("input"));

        let err = run_value("", "+00:00", None).unwrap_err();
        assert_eq!(err.field.as_deref(), Some("input"));
    }

    #[test]
    fn rejects_invalid_offsets() {
        for spec in ["8:00", "+25:00", "+08:60", "abc", "+08", ""] {
            let err = run_value("0", spec, None).unwrap_err();
            assert_eq!(err.code, "INVALID_ARGS", "{spec}");
            assert_eq!(err.field.as_deref(), Some("offset"), "{spec}");
        }
    }

    #[test]
    fn rejects_invalid_durations() {
        for spec in ["1x", "d", "", "1d2h", "1.5d"] {
            let err = run_value("0", "+00:00", Some(spec)).unwrap_err();
            assert_eq!(err.code, "INVALID_ARGS", "{spec}");
            assert_eq!(err.field.as_deref(), Some("add"), "{spec}");
        }
    }

    #[test]
    fn rejects_overflowing_addition() {
        let err = run_value("9999-12-31T23:59:59Z", "+00:00", Some("1w")).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("add"));
    }

    #[test]
    fn cli_and_json_paths_agree() {
        let cli = run_cli(
            &mut ToolCtx::default(),
            &matches(&[
                "timestamp",
                "2024-02-28T12:00:00Z",
                "--offset",
                "+08:00",
                "--add",
                "1d",
            ]),
        )
        .unwrap();
        let json_out = run_json(
            &mut ToolCtx::default(),
            &json!({
                "input": "2024-02-28T12:00:00Z",
                "offset": "+08:00",
                "add": "1d"
            }),
        )
        .unwrap();
        assert_eq!(cli, json_out);
        assert_eq!(rfc3339(&cli), "2024-02-29T20:00:00+08:00");
    }

    #[test]
    fn cli_accepts_negative_numbers_and_hyphen_values() {
        let out = run_cli(&mut ToolCtx::default(), &matches(&["timestamp", "-1"])).unwrap();
        assert_eq!(out["unix"], -1);

        let out = run_cli(
            &mut ToolCtx::default(),
            &matches(&["timestamp", "0", "--offset", "-05:30"]),
        )
        .unwrap();
        assert_eq!(out["offset"], "-05:30");
    }

    #[test]
    fn json_entry_requires_input() {
        let err = run_json(&mut ToolCtx::default(), &json!({})).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("args"));

        let out = run_json(&mut ToolCtx::default(), &json!({"input": "0"})).unwrap();
        assert_eq!(out["offset"], "+00:00");
    }

    #[test]
    fn def_exposes_schema() {
        let tool = def();
        assert_eq!(tool.meta.name, "timestamp");
        assert_eq!(tool.meta.aliases, &["timestamptool"]);
        let schema = (tool.schema.expect("应提供 schema"))();
        for field in ["\"input\"", "\"offset\"", "\"add\""] {
            assert!(schema.contains(field), "{field} 不在 schema: {schema}");
        }
    }
}
