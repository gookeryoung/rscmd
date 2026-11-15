//! 工具注册表：查找、别名解析与冲突校验。

use std::collections::HashSet;

use crate::tool::{PythonExposure, ToolDef};

/// 与工具同名的内建命令；工具不得占用（否则 clap 子命令重复注册）。
pub const BUILTIN_COMMANDS: &[&str] = &["run", "tools", "info", "schema", "version", "doctor"];

pub struct Registry {
    tools: &'static [ToolDef],
}

impl Registry {
    pub const fn new(tools: &'static [ToolDef]) -> Self {
        Self { tools }
    }

    pub fn tools(&self) -> &'static [ToolDef] {
        self.tools
    }

    /// 规范名或别名均可命中。
    pub fn find(&self, name: &str) -> Option<&'static ToolDef> {
        self.tools
            .iter()
            .find(|tool| tool.meta.name == name || tool.meta.aliases.contains(&name))
    }

    /// 注册表不变量。返回全部问题，便于一次修完。
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut problems = Vec::new();
        let mut names: HashSet<&str> = HashSet::new();

        for builtin in BUILTIN_COMMANDS {
            names.insert(builtin);
        }

        for tool in self.tools {
            let meta = tool.meta;
            if meta.name.trim().is_empty() {
                problems.push("存在空工具名".to_string());
                continue;
            }
            if !meta
                .name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
            {
                problems.push(format!(
                    "工具名 `{}` 只允许小写字母、数字、`_`、`-`",
                    meta.name
                ));
            }
            if !names.insert(meta.name) {
                problems.push(format!("工具名或别名冲突: `{}`", meta.name));
            }
            if meta.summary.trim().is_empty() {
                problems.push(format!("工具 `{}` 缺少摘要", meta.name));
            }
            if meta.summary.chars().count() > 200 {
                problems.push(format!("工具 `{}` 的摘要超过 200 字符", meta.name));
            }
            if meta.examples.is_empty() {
                problems.push(format!("工具 `{}` 缺少示例", meta.name));
            }
            if meta.python == PythonExposure::Structured && tool.run_json.is_none() {
                problems.push(format!(
                    "工具 `{}` 声明为 Structured 但未提供 JSON 入口",
                    meta.name
                ));
            }
            for alias in meta.aliases {
                if alias.trim().is_empty() {
                    problems.push(format!("工具 `{}` 存在空别名", meta.name));
                    continue;
                }
                if !names.insert(alias) {
                    problems.push(format!("别名冲突: `{}`（工具 `{}`）", alias, meta.name));
                }
            }
        }

        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems)
        }
    }
}
