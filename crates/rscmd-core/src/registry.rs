//! 工具注册表：查找、别名解析与冲突校验。

use std::collections::HashSet;

use crate::tool::{PythonExposure, ToolDef};

/// 与工具同名的内建命令；工具不得占用（否则 clap 子命令重复注册）。
pub const BUILTIN_COMMANDS: &[&str] = &[
    "run",
    "tools",
    "info",
    "schema",
    "version",
    "doctor",
    "completion",
];

/// 内建命令的展示元数据：CLI 帮助与命令一览文档共用同一来源。
pub struct Builtin {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub about: &'static str,
}

/// 短别名会以 `doctor(d)` 的形式展示在帮助与文档中。
pub const BUILTINS: &[Builtin] = &[
    Builtin {
        name: "run",
        aliases: &["r"],
        about: "运行指定工具（推荐入口）",
    },
    Builtin {
        name: "tools",
        aliases: &[],
        about: "列出可用工具",
    },
    Builtin {
        name: "info",
        aliases: &["i"],
        about: "查看单个工具的元数据",
    },
    Builtin {
        name: "schema",
        aliases: &[],
        about: "导出全部工具的元数据",
    },
    Builtin {
        name: "version",
        aliases: &["v"],
        about: "输出版本信息",
    },
    Builtin {
        name: "doctor",
        aliases: &["d"],
        about: "输出运行环境自检信息",
    },
    Builtin {
        name: "completion",
        aliases: &[],
        about: "生成 shell 补全脚本（bash/zsh/fish/powershell）",
    },
];

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

        // 内建命令的名称与短别名都不可被工具占用：两者的别名都会挂到同一棵 clap 命令树上。
        for builtin in BUILTINS {
            names.insert(builtin.name);
            for alias in builtin.aliases {
                names.insert(alias);
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::ToolCtx;
    use crate::errors::RscmdError;
    use crate::tool::{CliRun, SideEffects, ToolMeta};
    use clap::{ArgMatches, Command};
    use serde_json::Value;

    fn build() -> Command {
        Command::new("fixture")
    }

    fn run(_ctx: &mut ToolCtx, _matches: &ArgMatches) -> Result<Value, RscmdError> {
        Ok(Value::Null)
    }

    /// 构造只含一个夹具工具的注册表；夹具全部泄漏成 `'static`，符合 `ToolDef` 的约束。
    fn registry(name: &'static str, aliases: &'static [&'static str]) -> &'static Registry {
        registry_with_summary(name, aliases, "测试用工具")
    }

    fn registry_with_summary(
        name: &'static str,
        aliases: &'static [&'static str],
        summary: &'static str,
    ) -> &'static Registry {
        static RUN: CliRun = run;
        let tool = ToolDef {
            meta: Box::leak(Box::new(ToolMeta {
                name,
                aliases,
                summary,
                side_effects: SideEffects::None,
                gui: false,
                python: PythonExposure::ArgvOnly,
                examples: &["rscmd fixture"],
            })),
            build,
            run: RUN,
            run_json: None,
            schema: None,
        };
        let tools: &'static [ToolDef] = Box::leak(vec![tool].into_boxed_slice());
        Box::leak(Box::new(Registry::new(tools)))
    }

    #[test]
    fn rejects_alias_taken_by_builtin_short_alias() {
        // `d` 是 doctor 的短别名，工具再占用会在 clap 命令树上撞车。
        let problems = registry("fixture", &["d"]).validate().unwrap_err();
        assert!(
            problems.iter().any(|problem| problem.contains("`d`")),
            "{problems:?}"
        );
    }

    #[test]
    fn rejects_tool_named_like_builtin() {
        let problems = registry("doctor", &[]).validate().unwrap_err();
        assert!(
            problems
                .iter()
                .any(|problem| problem.contains("工具名或别名冲突: `doctor`")),
            "{problems:?}"
        );
    }

    #[test]
    fn accepts_unreserved_alias() {
        assert!(registry("fixture", &["fx"]).validate().is_ok());
    }

    #[test]
    fn rejects_tool_without_summary() {
        let problems = registry_with_summary("fixture", &[], " ")
            .validate()
            .unwrap_err();
        assert!(
            problems.iter().any(|problem| problem.contains("缺少摘要")),
            "{problems:?}"
        );
    }

    #[test]
    fn fixture_tool_is_clap_invocable() {
        // 夹具的 build/run 也要真实走一遍，避免测试辅助代码成为未覆盖行。
        let registry = registry("fixture", &[]);
        let tool = &registry.tools()[0];
        let matches = (tool.build)()
            .try_get_matches_from(["fixture"])
            .expect("夹具命令应可解析");
        let out = (tool.run)(&mut ToolCtx::default(), &matches).expect("夹具应返回 Ok");
        assert_eq!(out, Value::Null);
    }
}
