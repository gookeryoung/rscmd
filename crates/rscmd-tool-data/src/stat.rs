//! `stat`：对一组数值做计数、求和、均值、中位数与分位数。
//!
//! 分位数采用「排序后按 `p/100 * (n-1)` 线性插值」口径，输入顺序不影响结果；
//! 0 与 100 分别对应最小值与最大值，小数分位数按相邻两点插值。

use clap::{ArgMatches, Command};
use rscmd_core::{
    command_for, PythonExposure, RscmdError, SideEffects, ToolCtx, ToolDef, ToolMeta,
};
use serde_json::{json, Value};

pub static META: ToolMeta = ToolMeta {
    name: "stat",
    aliases: &["stattool"],
    summary: "数值统计：计数、求和、均值、中位数与分位数",
    side_effects: SideEffects::None,
    gui: false,
    python: PythonExposure::Structured,
    examples: &[
        "rscmd stat 1 2 3 4 --percentile 25,90",
        "rscmd stat -2 -1 0 1 --json",
    ],
};

#[derive(Debug, clap::Args, serde::Deserialize, schemars::JsonSchema)]
pub struct StatArgs {
    /// 待统计的数值，至少一个；支持负数
    #[arg(value_name = "NUMBER", required = true, allow_negative_numbers = true)]
    pub values: Vec<f64>,
    /// 需要计算的分位数（0–100，可重复或逗号分隔）
    #[arg(long = "percentile", value_name = "P", value_delimiter = ',')]
    #[serde(default)]
    pub percentiles: Vec<f64>,
}

pub fn build() -> Command {
    command_for::<StatArgs>(&META)
}

pub fn run_cli(ctx: &mut ToolCtx, matches: &ArgMatches) -> Result<Value, RscmdError> {
    let args = <StatArgs as clap::FromArgMatches>::from_arg_matches(matches)
        .map_err(RscmdError::from_clap)?;
    run(ctx, args)
}

pub fn run_json(ctx: &mut ToolCtx, input: &Value) -> Result<Value, RscmdError> {
    let args: StatArgs = serde_json::from_value(input.clone())
        .map_err(|err| RscmdError::coded("INVALID_ARGS", err.to_string()).with_field("args"))?;
    run(ctx, args)
}

fn run(_ctx: &mut ToolCtx, args: StatArgs) -> Result<Value, RscmdError> {
    if args.values.is_empty() {
        return Err(RscmdError::coded("INVALID_ARGS", "至少需要一个数值")
            .with_field("values")
            .with_expected("有限浮点数数组"));
    }
    if args.values.iter().any(|value| !value.is_finite()) {
        return Err(RscmdError::coded("INVALID_ARGS", "数值必须是有限浮点数")
            .with_field("values")
            .with_expected("有限浮点数数组"));
    }
    if args
        .percentiles
        .iter()
        .any(|value| !value.is_finite() || !(0.0..=100.0).contains(value))
    {
        return Err(RscmdError::coded("INVALID_ARGS", "分位数必须在 0–100 之间")
            .with_field("percentiles")
            .with_expected("0–100 的有限浮点数"));
    }

    let mut sorted = args.values.clone();
    sorted.sort_by(f64::total_cmp);
    let count = sorted.len();
    let sum: f64 = sorted.iter().sum();
    let mean = sum / count as f64;
    let median = percentile(&sorted, 50.0);
    let percentiles: Vec<Value> = args
        .percentiles
        .iter()
        .map(|p| json!({"percentile": p, "value": percentile(&sorted, *p)}))
        .collect();

    Ok(json!({
        "count": count,
        "sum": sum,
        "mean": mean,
        "median": median,
        "percentiles": percentiles,
    }))
}

/// 线性插值分位数：rank = p/100 * (n-1)，落在相邻两点之间时按权重插值。
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.len() == 1 {
        return sorted[0];
    }
    let rank = p / 100.0 * (sorted.len() - 1) as f64;
    let lower = rank.floor() as usize;
    let upper = rank.ceil() as usize;
    if lower == upper {
        return sorted[lower];
    }
    let weight = rank - lower as f64;
    sorted[lower] + (sorted[upper] - sorted[lower]) * weight
}

pub fn def() -> ToolDef {
    ToolDef {
        meta: &META,
        build,
        run: run_cli,
        run_json: Some(run_json),
        schema: Some(stat_args_schema),
    }
}

fn stat_args_schema() -> String {
    let schema = schemars::schema_for!(StatArgs);
    serde_json::to_string_pretty(&schema).expect("StatArgs schema 可序列化")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(args: &[&str]) -> ArgMatches {
        build().try_get_matches_from(args).expect("参数应可解析")
    }

    fn run_values(values: &[f64], percentiles: &[f64]) -> Result<Value, RscmdError> {
        run(
            &mut ToolCtx::default(),
            StatArgs {
                values: values.to_vec(),
                percentiles: percentiles.to_vec(),
            },
        )
    }

    fn close(actual: &Value, expected: f64) -> bool {
        (actual.as_f64().unwrap() - expected).abs() < 1e-9
    }

    #[test]
    fn computes_summary_and_interpolated_percentiles() {
        let out = run_values(&[1.0, 2.0, 3.0, 4.0], &[0.0, 25.0, 50.0, 75.0, 90.0, 100.0]).unwrap();
        assert_eq!(out["count"], 4);
        assert!(close(&out["sum"], 10.0));
        assert!(close(&out["mean"], 2.5));
        assert!(close(&out["median"], 2.5));
        let expected = [1.0, 1.75, 2.5, 3.25, 3.7, 4.0];
        for (index, value) in expected.iter().enumerate() {
            assert!(
                close(&out["percentiles"][index]["value"], *value),
                "p{} 期望 {value}，实得 {}",
                out["percentiles"][index]["percentile"],
                out["percentiles"][index]["value"]
            );
            assert!(close(
                &out["percentiles"][index]["percentile"],
                [0.0, 25.0, 50.0, 75.0, 90.0, 100.0][index]
            ));
        }
    }

    #[test]
    fn input_order_does_not_matter() {
        let ordered = run_values(&[1.0, 2.0, 3.0], &[25.0]).unwrap();
        let shuffled = run_values(&[3.0, 1.0, 2.0], &[25.0]).unwrap();
        assert_eq!(ordered, shuffled);
    }

    #[test]
    fn single_value_is_its_own_median() {
        let out = run_values(&[7.5], &[0.0, 50.0, 100.0]).unwrap();
        assert_eq!(out["count"], 1);
        assert!(close(&out["sum"], 7.5));
        assert!(close(&out["mean"], 7.5));
        assert!(close(&out["median"], 7.5));
        for index in 0..3 {
            assert!(close(&out["percentiles"][index]["value"], 7.5));
        }
    }

    #[test]
    fn negative_values_are_supported() {
        let out = run_values(&[-2.0, -1.0, 0.0, 1.0], &[0.0, 25.0, 50.0]).unwrap();
        assert!(close(&out["sum"], -2.0));
        assert!(close(&out["mean"], -0.5));
        assert!(close(&out["median"], -0.5));
        assert!(close(&out["percentiles"][0]["value"], -2.0));
        assert!(close(&out["percentiles"][1]["value"], -1.25));
        assert!(close(&out["percentiles"][2]["value"], -0.5));
    }

    #[test]
    fn no_percentiles_means_empty_array() {
        let out = run_values(&[1.0, 2.0], &[]).unwrap();
        assert_eq!(out["percentiles"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn rejects_empty_values() {
        let err = run_values(&[], &[]).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("values"));
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn rejects_non_finite_values_and_percentiles() {
        let err = run_values(&[1.0, f64::NAN], &[]).unwrap_err();
        assert_eq!(err.field.as_deref(), Some("values"));

        let err = run_values(&[1.0], &[-1.0]).unwrap_err();
        assert_eq!(err.field.as_deref(), Some("percentiles"));

        let err = run_values(&[1.0], &[100.5]).unwrap_err();
        assert_eq!(err.field.as_deref(), Some("percentiles"));

        let err = run_values(&[1.0], &[f64::INFINITY]).unwrap_err();
        assert_eq!(err.field.as_deref(), Some("percentiles"));
    }

    #[test]
    fn cli_and_json_paths_agree() {
        let cli = run_cli(
            &mut ToolCtx::default(),
            &matches(&["stat", "1", "2", "3", "4", "--percentile", "25,90"]),
        )
        .unwrap();
        let json_out = run_json(
            &mut ToolCtx::default(),
            &json!({"values": [1.0, 2.0, 3.0, 4.0], "percentiles": [25.0, 90.0]}),
        )
        .unwrap();
        assert_eq!(cli, json_out);
    }

    #[test]
    fn cli_normalizes_negative_numbers_and_requires_values() {
        let out = run_cli(
            &mut ToolCtx::default(),
            &matches(&["stat", "-2", "-1", "0"]),
        )
        .unwrap();
        assert!(close(&out["mean"], -1.0));
        assert!(build().try_get_matches_from(["stat"]).is_err());
    }

    #[test]
    fn json_entry_applies_defaults() {
        let out = run_json(&mut ToolCtx::default(), &json!({"values": [2, 4, 6]})).unwrap();
        assert_eq!(out["count"], 3);
        assert!(close(&out["median"], 4.0));
        assert_eq!(out["percentiles"].as_array().unwrap().len(), 0);

        let err = run_json(&mut ToolCtx::default(), &json!({})).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("args"));
    }

    #[test]
    fn def_exposes_schema() {
        let tool = def();
        assert_eq!(tool.meta.name, "stat");
        assert_eq!(tool.meta.aliases, &["stattool"]);
        let schema = (tool.schema.expect("应提供 schema"))();
        assert!(schema.contains("\"values\""), "{schema}");
        assert!(schema.contains("\"percentiles\""), "{schema}");
    }
}
