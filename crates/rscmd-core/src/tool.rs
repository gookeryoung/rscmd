//! 工具契约：元数据 + CLI 构建 + 两种调用入口（ArgMatches / JSON）。

use clap::{ArgMatches, Command};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{json, Value};

use crate::ctx::ToolCtx;
use crate::errors::RscmdError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SideEffects {
    None,
    Read,
    Write,
    Network,
    Destructive,
}

impl SideEffects {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Read => "read",
            Self::Write => "write",
            Self::Network => "network",
            Self::Destructive => "destructive",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PythonExposure {
    Hidden,
    ArgvOnly,
    Structured,
}

impl PythonExposure {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hidden => "hidden",
            Self::ArgvOnly => "argv_only",
            Self::Structured => "structured",
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ToolMeta {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub summary: &'static str,
    pub side_effects: SideEffects,
    pub gui: bool,
    pub python: PythonExposure,
    pub examples: &'static [&'static str],
}

/// CLI 入口：从已解析的 clap 匹配中读取参数并执行。
pub type CliRun = fn(&mut ToolCtx, &ArgMatches) -> Result<Value, RscmdError>;
/// JSON 入口：从 JSON 参数对象读取参数并执行。未提供时该工具仅支持 CLI。
pub type JsonRun = fn(&mut ToolCtx, &Value) -> Result<Value, RscmdError>;

/// 工具定义。全部字段都是编译期常量或函数指针，可安全拷贝进聚合表。
#[derive(Clone, Copy)]
pub struct ToolDef {
    pub meta: &'static ToolMeta,
    pub build: fn() -> Command,
    pub run: CliRun,
    pub run_json: Option<JsonRun>,
    /// 返回该工具参数类型的 JSON Schema（顶层字符串，pretty 格式）。`None` 表示该工具不暴露 schema。
    pub schema: Option<fn() -> String>,
}

/// 用参数结构自动生成该工具的子命令。
pub fn command_for<A: clap::Args>(meta: &'static ToolMeta) -> Command {
    A::augment_args(Command::new(meta.name).about(meta.summary))
}

/// 元数据的 JSON 表示，供 tools/info/schema 与 Python 绑定复用。
pub fn meta_json(tool: &ToolDef) -> Value {
    json!({
        "name": tool.meta.name,
        "aliases": tool.meta.aliases,
        "summary": tool.meta.summary,
        "side_effects": tool.meta.side_effects.as_str(),
        "gui": tool.meta.gui,
        "python": tool.meta.python.as_str(),
        "examples": tool.meta.examples,
    })
}

/// ToolMeta 结构体的 JSON Schema，供 `schema/*.json` 生成物使用。
pub fn meta_schema() -> String {
    let schema = schemars::schema_for!(ToolMeta);
    serde_json::to_string_pretty(&schema).expect("ToolMeta schema 可序列化")
}
