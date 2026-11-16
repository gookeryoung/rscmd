//! `csv`：CSV 与 JSON 互转。
//!
//! - 默认方向 `--to json`：把 CSV 解析为 JSON 行；自动判定表头与分隔符。
//! - `--to csv`：把 JSON（对象数组或数组的数组）写成 CSV。
//! - CSV 值在 JSON 中一律是字符串；`--to csv` 时对象列按列名字典序输出，
//!   `null`/缺失列写成空字符串，嵌套对象/数组写成其 JSON 文本。
//! - 表头自动判定：首行所有单元格非空、互不重复且都不是数字时视为表头；
//!   `--no-header` 强制按数据行处理。分隔符 `auto` 在 CSV→JSON 时取首行
//!   出现次数最多的候选（`,` `;` `\t` `|`），JSON→CSV 时固定为逗号。
//! - 输入统一按 UTF-8 处理，非 UTF-8 文件返回 `INVALID_ENCODING`；开头的 BOM 会被去掉。
//! - 条数上限沿用全局 `--limit`（写入 `ToolCtx.limit`，0 或未指定表示不限制），
//!   JSON 入口另提供同义 `limit` 字段。

use std::collections::HashSet;
use std::path::PathBuf;

use clap::{ArgMatches, Command};
use rscmd_core::{
    command_for, PythonExposure, RscmdError, SideEffects, ToolCtx, ToolDef, ToolMeta,
};
use serde_json::{json, Map, Value};

pub static META: ToolMeta = ToolMeta {
    name: "csv",
    aliases: &["csvtool"],
    summary: "CSV 与 JSON 互转，自动判定表头与分隔符",
    side_effects: SideEffects::Read,
    gui: false,
    python: PythonExposure::Structured,
    examples: &[
        "rscmd csv --file data.csv --to json --limit 20",
        "rscmd csv --text '[{\"a\":1,\"b\":2}]' --to csv",
    ],
};

#[derive(Debug, clap::Args, serde::Deserialize, schemars::JsonSchema)]
pub struct CsvArgs {
    /// 输入文件路径（与 --text 二选一）
    #[arg(value_name = "INPUT")]
    #[serde(default)]
    pub input: Option<PathBuf>,
    /// 直接传入文本（与 INPUT 二选一）
    #[arg(long, value_name = "TEXT")]
    #[serde(default)]
    pub text: Option<String>,
    /// 转换方向：`json` = CSV→JSON，`csv` = JSON→CSV
    #[arg(long, value_name = "TARGET", default_value = "json")]
    #[serde(default = "default_to")]
    pub to: String,
    /// 分隔符：`auto`（默认）、`comma`、`semicolon`、`tab`、`pipe` 或单个 ASCII 字符
    #[arg(long, value_name = "DELIM", default_value = "auto")]
    #[serde(default = "default_delimiter")]
    pub delimiter: String,
    /// CSV→JSON：首行不是表头，按数组输出
    #[arg(long)]
    #[serde(default)]
    pub no_header: bool,
    /// 最多输出的数据行数，0 或不提供表示不限制；CLI 走全局 `--limit`（ToolCtx.limit）
    #[arg(skip)]
    #[serde(default)]
    pub limit: Option<usize>,
}

fn default_to() -> String {
    "json".to_string()
}

fn default_delimiter() -> String {
    "auto".to_string()
}

pub fn build() -> Command {
    command_for::<CsvArgs>(&META)
}

pub fn run_cli(ctx: &mut ToolCtx, matches: &ArgMatches) -> Result<Value, RscmdError> {
    let args = <CsvArgs as clap::FromArgMatches>::from_arg_matches(matches)
        .map_err(RscmdError::from_clap)?;
    run(ctx, args)
}

pub fn run_json(ctx: &mut ToolCtx, input: &Value) -> Result<Value, RscmdError> {
    let args: CsvArgs = serde_json::from_value(input.clone())
        .map_err(|err| RscmdError::coded("INVALID_ARGS", err.to_string()).with_field("args"))?;
    run(ctx, args)
}

fn run(ctx: &mut ToolCtx, args: CsvArgs) -> Result<Value, RscmdError> {
    let limit = args.limit.or(ctx.limit).filter(|value| *value > 0);
    let raw = read_input(&args)?;
    // 去掉 BOM，让 CSV 表头与 JSON 解析在带 BOM 的文件上保持一致。
    let input = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
    match args.to.as_str() {
        "json" => csv_to_json(input, &args, limit),
        "csv" => json_to_csv(input, &args, limit),
        other => Err(
            RscmdError::coded("INVALID_MODE", format!("未知的转换方向: {other}"))
                .with_field("to")
                .with_expected("json 或 csv"),
        ),
    }
}

fn read_input(args: &CsvArgs) -> Result<String, RscmdError> {
    match (args.input.as_ref(), args.text.as_ref()) {
        (Some(path), None) => {
            let bytes = std::fs::read(path).map_err(|err| {
                RscmdError::coded("IO", format!("读取输入文件失败: {err}")).with_field("input")
            })?;
            String::from_utf8(bytes).map_err(|_| {
                RscmdError::coded("INVALID_ENCODING", "输入文件不是 UTF-8 文本")
                    .with_field("input")
                    .with_expected("UTF-8 文本")
            })
        }
        (None, Some(text)) => Ok(text.clone()),
        (Some(_), Some(_)) => Err(
            RscmdError::coded("INVALID_ARGS", "INPUT 与 --text 只能二选一")
                .with_field("input")
                .with_expected("只给 INPUT 或只给 --text"),
        ),
        (None, None) => Err(RscmdError::coded("INVALID_ARGS", "缺少输入")
            .with_field("input")
            .with_expected("INPUT 路径或 --text 文本")),
    }
}

fn csv_to_json(input: &str, args: &CsvArgs, limit: Option<usize>) -> Result<Value, RscmdError> {
    let delimiter = resolve_delimiter(input, &args.delimiter)?;
    let mut reader = ::csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .from_reader(input.as_bytes());
    let mut records = reader.records().peekable();

    let first = match records.next() {
        None => {
            return Ok(empty_result("csv_to_json", delimiter, false));
        }
        Some(Ok(record)) => to_fields(&record),
        Some(Err(err)) => return Err(csv_error(err)),
    };
    let has_header = !args.no_header && records.peek().is_some() && looks_like_header(&first);
    let mut rows: Vec<Value> = Vec::new();
    let header = if has_header {
        Some(first)
    } else {
        // 首行不是表头：它本身也是数据。
        rows.push(row_value(&first, None));
        None
    };

    let mut truncated = false;
    for record in records {
        let record = record.map_err(csv_error)?;
        if limit.is_some_and(|value| rows.len() >= value) {
            // 还能读到下一条数据行 → 说明确实被上限截断。
            truncated = true;
            break;
        }
        let fields = to_fields(&record);
        rows.push(row_value(&fields, header.as_deref()));
    }

    Ok(json!({
        "direction": "csv_to_json",
        "delimiter": (delimiter as char).to_string(),
        "has_header": has_header,
        "count": rows.len(),
        "truncated": truncated,
        "rows": rows,
    }))
}

fn row_value(fields: &[String], header: Option<&[String]>) -> Value {
    match header {
        Some(header) => {
            let mut map = Map::new();
            for (index, name) in header.iter().enumerate() {
                map.insert(
                    name.clone(),
                    Value::String(fields.get(index).cloned().unwrap_or_default()),
                );
            }
            Value::Object(map)
        }
        None => Value::Array(fields.iter().cloned().map(Value::String).collect()),
    }
}

fn json_to_csv(input: &str, args: &CsvArgs, limit: Option<usize>) -> Result<Value, RscmdError> {
    // JSON 里逗号普遍存在，auto 在这里固定按逗号处理。
    let delimiter = resolve_delimiter("", &args.delimiter)?;
    let parsed: Value = serde_json::from_str(input).map_err(|err| {
        RscmdError::coded("INVALID_JSON", err.to_string())
            .with_field("input")
            .with_expected("合法的 JSON 文本")
    })?;
    let Value::Array(items) = parsed else {
        return Err(RscmdError::coded("INVALID_ARGS", "JSON 顶层必须是数组")
            .with_field("input")
            .with_expected("对象数组或数组的数组"));
    };

    let header = match items.first() {
        Some(Value::Object(map)) => Some(map.keys().cloned().collect::<Vec<String>>()),
        Some(Value::Array(_)) | None => None,
        Some(_) => {
            return Err(RscmdError::coded("INVALID_ARGS", "行必须是对象或数组")
                .with_field("input")
                .with_expected("对象数组或数组的数组"));
        }
    };

    let truncated = limit.is_some_and(|value| items.len() > value);
    let data: Vec<&Value> = match limit {
        Some(value) => items.iter().take(value).collect(),
        None => items.iter().collect(),
    };

    let mut writer = ::csv::WriterBuilder::new()
        .delimiter(delimiter)
        .from_writer(Vec::new());
    if let Some(header) = &header {
        writer.write_record(header).map_err(csv_error)?;
    }
    for item in &data {
        let row: Vec<String> = match (&header, item) {
            (Some(header), Value::Object(map)) => {
                header.iter().map(|key| cell_text(map.get(key))).collect()
            }
            (None, Value::Array(values)) => values.iter().map(cell_text_value).collect(),
            _ => {
                return Err(
                    RscmdError::coded("INVALID_ARGS", "同一数组里的行类型必须一致")
                        .with_field("input")
                        .with_expected("对象数组或数组的数组"),
                );
            }
        };
        writer.write_record(&row).map_err(csv_error)?;
    }
    let bytes = writer.into_inner().map_err(|err| {
        RscmdError::coded("IO", format!("写出 CSV 失败: {err}")).with_field("input")
    })?;
    let text = String::from_utf8(bytes).map_err(|_| {
        RscmdError::coded("INVALID_ENCODING", "CSV 输出不是 UTF-8").with_field("output")
    })?;

    Ok(json!({
        "direction": "json_to_csv",
        "delimiter": (delimiter as char).to_string(),
        "has_header": header.is_some(),
        "count": data.len(),
        "truncated": truncated,
        "csv": text,
    }))
}

fn cell_text(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(value) => cell_text_value(value),
    }
}

fn cell_text_value(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn empty_result(direction: &str, delimiter: u8, has_header: bool) -> Value {
    json!({
        "direction": direction,
        "delimiter": (delimiter as char).to_string(),
        "has_header": has_header,
        "count": 0,
        "truncated": false,
        "rows": [],
    })
}

fn to_fields(record: &::csv::StringRecord) -> Vec<String> {
    record.iter().map(str::to_string).collect()
}

fn looks_like_header(fields: &[String]) -> bool {
    if fields.is_empty() {
        return false;
    }
    let mut seen = HashSet::new();
    fields.iter().all(|cell| {
        let cell = cell.trim();
        !cell.is_empty() && cell.parse::<f64>().is_err() && seen.insert(cell.to_string())
    })
}

/// 解析分隔符；`auto` 时按首行（引号外）出现次数选择，平局取逗号。
fn resolve_delimiter(input: &str, spec: &str) -> Result<u8, RscmdError> {
    let spec = spec.trim();
    let delimiter = match spec {
        "auto" => detect_delimiter(input),
        "comma" | "," => b',',
        "semicolon" | ";" => b';',
        "tab" | "\\t" => b'\t',
        "pipe" | "|" => b'|',
        other => {
            let mut chars = other.chars();
            match (chars.next(), chars.next()) {
                (Some(ch), None) if ch.is_ascii() => ch as u8,
                _ => {
                    return Err(RscmdError::coded(
                        "INVALID_ARGS",
                        format!("无法识别的分隔符: {spec}"),
                    )
                    .with_field("delimiter")
                    .with_expected("auto、comma、semicolon、tab、pipe 或单个 ASCII 字符"));
                }
            }
        }
    };
    if delimiter == b'"' || delimiter == b'\n' || delimiter == b'\r' {
        return Err(
            RscmdError::coded("INVALID_ARGS", format!("不能使用 {spec} 作为分隔符"))
                .with_field("delimiter")
                .with_expected("auto、comma、semicolon、tab、pipe 或单个 ASCII 字符"),
        );
    }
    Ok(delimiter)
}

fn detect_delimiter(input: &str) -> u8 {
    let candidates = [b',', b';', b'\t', b'|'];
    let mut counts = [0usize; 4];
    let mut in_quotes = false;
    for ch in input.chars() {
        if ch == '"' {
            in_quotes = !in_quotes;
            continue;
        }
        if ch == '\n' && !in_quotes {
            break;
        }
        if !in_quotes {
            for (index, candidate) in candidates.iter().enumerate() {
                if ch == *candidate as char {
                    counts[index] += 1;
                }
            }
        }
    }
    let mut best = 0;
    for index in 1..candidates.len() {
        if counts[index] > counts[best] {
            best = index;
        }
    }
    candidates[best]
}

fn csv_error(err: ::csv::Error) -> RscmdError {
    RscmdError::coded("INVALID_ARGS", format!("CSV 解析失败: {err}"))
        .with_field("input")
        .with_expected("合法的 CSV 文本")
}

pub fn def() -> ToolDef {
    ToolDef {
        meta: &META,
        build,
        run: run_cli,
        run_json: Some(run_json),
        schema: Some(csv_args_schema),
    }
}

fn csv_args_schema() -> String {
    let schema = schemars::schema_for!(CsvArgs);
    serde_json::to_string_pretty(&schema).expect("CsvArgs schema 可序列化")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(args: &[&str]) -> ArgMatches {
        build().try_get_matches_from(args).expect("参数应可解析")
    }

    fn args() -> CsvArgs {
        CsvArgs {
            input: None,
            text: None,
            to: default_to(),
            delimiter: default_delimiter(),
            no_header: false,
            limit: None,
        }
    }

    fn run_text(text: &str, mutate: impl FnOnce(&mut CsvArgs)) -> Result<Value, RscmdError> {
        let mut input = args();
        input.text = Some(text.to_string());
        mutate(&mut input);
        run(&mut ToolCtx::default(), input)
    }

    fn lines(value: &Value) -> Vec<String> {
        value
            .as_str()
            .expect("应为字符串")
            .lines()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn parses_csv_with_header_into_objects() {
        let out = run_text("name,age\nAlice,30\nBob,25\n", |_| {}).unwrap();
        assert_eq!(out["direction"], "csv_to_json");
        assert_eq!(out["delimiter"], ",");
        assert_eq!(out["has_header"], true);
        assert_eq!(out["count"], 2);
        assert_eq!(out["truncated"], false);
        assert_eq!(out["rows"][0]["name"], "Alice");
        assert_eq!(out["rows"][0]["age"], "30");
        assert_eq!(out["rows"][1]["name"], "Bob");
    }

    #[test]
    fn numeric_first_row_is_data_not_header() {
        let out = run_text("1,2\n3,4\n", |_| {}).unwrap();
        assert_eq!(out["has_header"], false);
        assert_eq!(out["count"], 2);
        assert_eq!(out["rows"][0], json!(["1", "2"]));
        assert_eq!(out["rows"][1], json!(["3", "4"]));
    }

    #[test]
    fn no_header_flag_keeps_first_row_as_data() {
        let out = run_text("name,age\nAlice,30\n", |input| {
            input.no_header = true;
        })
        .unwrap();
        assert_eq!(out["has_header"], false);
        assert_eq!(out["count"], 2);
        assert_eq!(out["rows"][0], json!(["name", "age"]));
    }

    #[test]
    fn auto_detects_semicolon_and_tab() {
        let out = run_text("a;b\n1;2\n", |_| {}).unwrap();
        assert_eq!(out["delimiter"], ";");
        assert_eq!(out["rows"][0]["a"], "1");

        let out = run_text("a\tb\n1\t2\n", |_| {}).unwrap();
        assert_eq!(out["delimiter"], "\t");
        assert_eq!(out["rows"][0]["b"], "2");
    }

    #[test]
    fn quoted_fields_support_delimiters_newlines_and_quotes() {
        let out = run_text(
            "\"a,b\",\"line1\nline2\",\"say \"\"hi\"\"\"\n1,2,3\n",
            |_| {},
        )
        .unwrap();
        assert_eq!(out["has_header"], true);
        assert_eq!(out["rows"][0]["a,b"], "1");
        assert_eq!(out["rows"][0]["line1\nline2"], "2");
        assert_eq!(out["rows"][0]["say \"hi\""], "3");
    }

    #[test]
    fn strips_bom_and_handles_crlf() {
        let out = run_text("\u{feff}a,b\r\n1,2\r\n", |_| {}).unwrap();
        assert_eq!(out["has_header"], true);
        assert_eq!(out["rows"][0]["a"], "1");
        assert_eq!(out["rows"][0]["b"], "2");
    }

    #[test]
    fn empty_input_is_an_empty_result() {
        let out = run_text("", |_| {}).unwrap();
        assert_eq!(out["count"], 0);
        assert_eq!(out["has_header"], false);
        assert_eq!(out["rows"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn limit_truncates_rows_from_context_and_json() {
        let mut ctx = ToolCtx::new(false, false, false, Some(1), 0, false);
        let mut input = args();
        input.text = Some("a\n1\n2\n".to_string());
        let out = run(&mut ctx, input).unwrap();
        assert_eq!(out["count"], 1);
        assert_eq!(out["truncated"], true);

        let out = run_text("a\n1\n", |input| input.limit = Some(5)).unwrap();
        assert_eq!(out["count"], 1);
        assert_eq!(out["truncated"], false);
    }

    #[test]
    fn converts_objects_to_csv_with_sorted_columns() {
        let out = run_text(r#"[{"b":2,"a":1},{"a":3,"b":null}]"#, |input| {
            input.to = "csv".to_string();
        })
        .unwrap();
        assert_eq!(out["direction"], "json_to_csv");
        assert_eq!(out["has_header"], true);
        assert_eq!(lines(&out["csv"]), ["a,b", "1,2", "3,"]);
    }

    #[test]
    fn converts_arrays_to_csv_without_header() {
        let out = run_text(r#"[[1,2],["x","y"]]"#, |input| {
            input.to = "csv".to_string();
        })
        .unwrap();
        assert_eq!(out["has_header"], false);
        assert_eq!(lines(&out["csv"]), ["1,2", "x,y"]);
    }

    #[test]
    fn nested_values_are_written_as_json_text() {
        let out = run_text(r#"[{"a":{"x":1}}]"#, |input| {
            input.to = "csv".to_string();
        })
        .unwrap();
        assert!(
            out["csv"].as_str().unwrap().contains("\"\"x\"\""),
            "{}",
            out["csv"]
        );
    }

    #[test]
    fn json_rows_with_unequal_lengths_fail() {
        let err = run_text("[[1],[2,3]]", |input| input.to = "csv".to_string()).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("input"));
    }

    #[test]
    fn invalid_json_is_reported_as_invalid_json() {
        let err = run_text("{", |input| input.to = "csv".to_string()).unwrap_err();
        assert_eq!(err.code, "INVALID_JSON");
        assert_eq!(err.field.as_deref(), Some("input"));
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn empty_json_array_produces_empty_csv() {
        let out = run_text("[]", |input| input.to = "csv".to_string()).unwrap();
        assert_eq!(out["count"], 0);
        assert_eq!(out["csv"], "");
    }

    #[test]
    fn rejects_both_and_missing_inputs() {
        let mut input = args();
        input.input = Some(PathBuf::from("a.csv"));
        input.text = Some("a,b".to_string());
        let err = run(&mut ToolCtx::default(), input).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("input"));

        let err = run(&mut ToolCtx::default(), args()).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("input"));
    }

    #[test]
    fn non_utf8_file_is_invalid_encoding() {
        let dir = rscmd_runtime::fs::temp_dir().unwrap();
        let path = dir.path().join("bad.csv");
        std::fs::write(&path, [0xff, 0xfe, b'a']).unwrap();
        let mut input = args();
        input.input = Some(path);
        let err = run(&mut ToolCtx::default(), input).unwrap_err();
        assert_eq!(err.code, "INVALID_ENCODING");
        assert_eq!(err.field.as_deref(), Some("input"));
    }

    #[test]
    fn rejects_unknown_mode_and_delimiter() {
        let err = run_text("a,b", |input| input.to = "xml".to_string()).unwrap_err();
        assert_eq!(err.code, "INVALID_MODE");
        assert_eq!(err.field.as_deref(), Some("to"));

        let err = run_text("a,b", |input| input.delimiter = "xx".to_string()).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("delimiter"));

        let err = run_text("a,b", |input| input.delimiter = "\"".to_string()).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("delimiter"));
    }

    #[test]
    fn explicit_delimiter_accepts_single_ascii_character() {
        let out = run_text("a^b\n1^2\n", |input| input.delimiter = "^".to_string()).unwrap();
        assert_eq!(out["delimiter"], "^");
        assert_eq!(out["rows"][0]["a"], "1");
    }

    #[test]
    fn csv_reader_errors_are_invalid_args() {
        let err = run_text("a,b\n1\n", |_| {}).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("input"));
    }

    #[test]
    fn missing_file_is_io_error() {
        let mut input = args();
        input.input = Some(PathBuf::from("definitely-not-a-real-csv-xyz.csv"));
        let err = run(&mut ToolCtx::default(), input).unwrap_err();
        assert_eq!(err.code, "IO");
        assert_eq!(err.field.as_deref(), Some("input"));
    }

    #[test]
    fn json_top_level_must_be_array() {
        let err = run_text(r#"{"a":1}"#, |input| input.to = "csv".to_string()).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("input"));

        let err = run_text("[[1],{\"a\":1}]", |input| input.to = "csv".to_string()).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("input"));

        let err = run_text("[1]", |input| input.to = "csv".to_string()).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("input"));
    }

    #[test]
    fn missing_object_keys_become_empty_cells() {
        let out = run_text(r#"[{"a":1,"b":2},{"a":3}]"#, |input| {
            input.to = "csv".to_string()
        })
        .unwrap();
        assert_eq!(lines(&out["csv"]), ["a,b", "1,2", "3,"]);
    }

    #[test]
    fn json_to_csv_honours_limit() {
        let out = run_text(r#"[{"a":1},{"a":2}]"#, |input| {
            input.to = "csv".to_string();
            input.limit = Some(1);
        })
        .unwrap();
        assert_eq!(out["count"], 1);
        assert_eq!(out["truncated"], true);
        assert_eq!(lines(&out["csv"]), ["a", "1"]);
    }

    #[test]
    fn rejects_control_and_non_ascii_delimiters() {
        for spec in ["\n", "\r", "→"] {
            let err = run_text("a,b", |input| input.delimiter = spec.to_string()).unwrap_err();
            assert_eq!(err.code, "INVALID_ARGS", "{spec}");
            assert_eq!(err.field.as_deref(), Some("delimiter"), "{spec}");
        }
    }

    #[test]
    fn single_row_without_header_is_data() {
        let out = run_text("only\n", |_| {}).unwrap();
        assert_eq!(out["has_header"], false);
        assert_eq!(out["rows"][0], json!(["only"]));
    }

    #[test]
    fn cli_and_json_paths_agree() {
        let cli = run_cli(
            &mut ToolCtx::default(),
            &matches(&["csv", "--text", "a,b\n1,2\n"]),
        )
        .unwrap();
        let json_out = run_json(&mut ToolCtx::default(), &json!({"text": "a,b\n1,2\n"})).unwrap();
        assert_eq!(cli, json_out);
    }

    #[test]
    fn json_entry_applies_defaults() {
        let out = run_json(&mut ToolCtx::default(), &json!({"text": "a,b\n1,2\n"})).unwrap();
        assert_eq!(out["has_header"], true);
        assert_eq!(out["count"], 1);

        let err = run_json(&mut ToolCtx::default(), &json!({})).unwrap_err();
        assert_eq!(err.code, "INVALID_ARGS");
        assert_eq!(err.field.as_deref(), Some("input"));
    }

    #[test]
    fn reads_input_from_file() {
        let dir = rscmd_runtime::fs::temp_dir().unwrap();
        let path = dir.path().join("data.csv");
        std::fs::write(&path, "a,b\n1,2\n").unwrap();
        let mut input = args();
        input.input = Some(path);
        let out = run(&mut ToolCtx::default(), input).unwrap();
        assert_eq!(out["rows"][0]["b"], "2");
    }

    #[test]
    fn def_exposes_schema() {
        let tool = def();
        assert_eq!(tool.meta.name, "csv");
        assert_eq!(tool.meta.aliases, &["csvtool"]);
        let schema = (tool.schema.expect("应提供 schema"))();
        for field in ["\"to\"", "\"delimiter\"", "\"no_header\"", "\"limit\""] {
            assert!(schema.contains(field), "{field} 不在 schema: {schema}");
        }
    }
}
