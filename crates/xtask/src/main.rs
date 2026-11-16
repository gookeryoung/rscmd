//! 仓库工具：注册表校验、依赖方向校验、生成物与门禁。

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

type Result<T> = std::result::Result<T, String>;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("help");
    let result = match command {
        "registry-check" => registry_check(),
        "code-check" => code_check(),
        "arch-check" => arch_check(),
        "docs-check" => docs_check(false),
        "gen-docs" => docs_check(true),
        "schema-check" => schema_check(false),
        "gen-schema" => schema_check(true),
        "check-fast" => check_fast(),
        "check" => check(),
        "check-cov" => check_cov(),
        "py-fresh-check" => py_fresh_check(),
        "py-type-check" => py_type_check(),
        "wheel-smoke" => wheel_smoke(),
        "bench" => bench(),
        "release-preflight" => release_preflight(),
        "version-check" => version_check(),
        _ => {
            print_help();
            Ok(())
        }
    };

    if let Err(err) = result {
        eprintln!("xtask {command} 失败:\n{err}");
        std::process::exit(1);
    }
}

fn print_help() {
    println!(
        "用法: cargo xtask <command>\n\
         \n\
         命令:\n\
         \x20 registry-check  校验工具注册表、元数据与文件布局\n\
         \x20 code-check      校验源码里用到的错误码都已在 ERROR_CODE_KINDS 登记\n\
         \x20 arch-check      校验 crate 依赖方向\n\
         \x20 docs-check      校验 docs/tools.md 是否为最新生成物\n\
         \x20 gen-docs        重新生成 docs/tools.md\n\
         \x20 schema-check    校验 schema/*.json 是否为最新生成物\n\
         \x20 gen-schema      重新生成 schema/*.json\n\
         \x20 check-fast      fmt + clippy + registry-check + arch-check + docs-check + schema-check\n\
         \x20 check           check-fast + 测试 + Python 测试与类型检查（若存在 .venv）\n\
         \x20 check-cov       运行 cargo llvm-cov（需已安装）\n\
         \x20 py-fresh-check  校验 python/rscmd/_native.* 是否旧于 Rust 源码\n\
         \x20 py-type-check   用 mypy 检查 python/rscmd 与 tests/py（未安装则跳过）\n\
         \x20 wheel-smoke     构建 wheel 并安装到 .venv 做导入与调用冒烟\n\
         \x20 bench           构建 release 并跑启动时间基准回归\n\
         \x20 version-check   校验 tag / Cargo / wheel 版本一致（tag 取 RSCMD_RELEASE_TAG 或 GITHUB_REF_NAME）\n\
         \x20 release-preflight 发布前本地全量预检（check + 覆盖率 + bench + wheel-smoke + 版本校验）"
    );
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("xtask 必须位于 workspace/crates/xtask")
        .to_path_buf()
}

/// `code-check`：源码里 `RscmdError::coded("CODE", ...)` 的字面量必须都已在
/// `ERROR_CODE_KINDS` 登记，避免新增错误码时漏登记（漏登记会回退成 kind=input、退出码 2）。
fn code_check() -> Result<()> {
    let registered: BTreeSet<&str> = rscmd_core::ERROR_CODE_KINDS
        .iter()
        .map(|(code, _)| *code)
        .collect();
    let crates_dir = workspace_root().join("crates");
    let table_file = crates_dir.join("rscmd-core").join("src").join("errors.rs");
    let mut problems = Vec::new();

    for file in rust_files(&crates_dir)? {
        // xtask 自身是仓库工具，不产出信封；errors.rs 是登记表本身（含测试夹具）。
        if file.starts_with(crates_dir.join("xtask")) || file == table_file {
            continue;
        }
        let text = fs::read_to_string(&file)
            .map_err(|err| format!("读取 {} 失败: {err}", file.display()))?;
        for (line, code) in coded_literals(&text) {
            if registered.contains(code.as_str()) {
                continue;
            }
            let rel = file.strip_prefix(&crates_dir).unwrap_or(&file);
            problems.push(format!(
                "crates/{}:{line} 使用了未登记的错误码 `{code}`，请在 crates/rscmd-core/src/errors.rs 的 ERROR_CODE_KINDS 登记",
                rel.display()
            ));
        }
    }

    if problems.is_empty() {
        println!("code-check 通过: {} 个错误码均已登记", registered.len());
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

/// 递归收集目录下的 `.rs` 文件（跳过构建产物目录）。
fn rust_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in fs::read_dir(&current).map_err(|err| err.to_string())? {
            let path = entry.map_err(|err| err.to_string())?.path();
            if path.is_dir() {
                if path.file_name().and_then(|name| name.to_str()) != Some("target") {
                    stack.push(path);
                }
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// 抽出 `coded("CODE"` 形式的首个参数，返回（行号, 代码）；非字面量参数直接忽略。
fn coded_literals(text: &str) -> Vec<(usize, String)> {
    const NEEDLE: &str = "coded(\"";
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let mut rest = line;
        while let Some(position) = rest.find(NEEDLE) {
            let after = &rest[position + NEEDLE.len()..];
            let Some(end) = after.find('"') else { break };
            out.push((index + 1, after[..end].to_string()));
            rest = &after[end..];
        }
    }
    out
}

fn registry_check() -> Result<()> {
    let tools = rscmd_tools::tools();
    if tools.is_empty() {
        return Err("未注册任何工具".to_string());
    }
    rscmd_tools::registry()
        .validate()
        .map_err(|problems| problems.join("\n"))?;

    let crates_dir = workspace_root().join("crates");
    let aggregator =
        fs::read_to_string(crates_dir.join("rscmd-tools/Cargo.toml")).map_err(|err| {
            format!(
                "读取 {} 失败: {err}",
                crates_dir.join("rscmd-tools/Cargo.toml").display()
            )
        })?;

    let mut source_names = BTreeSet::new();
    let mut domains = 0usize;

    for entry in fs::read_dir(&crates_dir).map_err(|err| err.to_string())? {
        let dir = entry.map_err(|err| err.to_string())?.path();
        let dir_name = dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string();
        if !dir_name.starts_with("rscmd-tool-") {
            continue;
        }
        domains += 1;
        if !aggregator.contains(&dir_name) {
            return Err(format!(
                "工具域 `{dir_name}` 未被 crates/rscmd-tools/Cargo.toml 引用"
            ));
        }

        let src = dir.join("src");
        let lib_rs = fs::read_to_string(src.join("lib.rs"))
            .map_err(|err| format!("读取 {} 失败: {err}", src.join("lib.rs").display()))?;

        for file in fs::read_dir(&src).map_err(|err| err.to_string())? {
            let file = file.map_err(|err| err.to_string())?.path();
            if file.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            let stem = file
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default()
                .to_string();
            if stem == "lib" {
                continue;
            }
            if !lib_rs.contains(&format!("pub mod {stem};")) {
                return Err(format!(
                    "{} 未在 {}/lib.rs 中声明 `pub mod {stem};`",
                    file.display(),
                    dir_name
                ));
            }
            let text = fs::read_to_string(&file).map_err(|err| err.to_string())?;
            for name in meta_names(&text) {
                source_names.insert(name);
            }
        }
    }

    let registered: BTreeSet<String> = tools
        .iter()
        .map(|tool| tool.meta.name.to_string())
        .collect();
    if registered != source_names {
        let missing: Vec<&String> = registered.difference(&source_names).collect();
        let extra: Vec<&String> = source_names.difference(&registered).collect();
        return Err(format!(
            "注册表与源码中的 ToolMeta 不一致\n仅注册表: {missing:?}\n仅源码: {extra:?}"
        ));
    }

    println!(
        "registry-check 通过：{} 个工具，{} 个域",
        tools.len(),
        domains
    );
    Ok(())
}

fn meta_names(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_meta = false;
    for line in text.lines() {
        if line.contains("pub static META") {
            in_meta = true;
        }
        if in_meta {
            if let Some(rest) = line.split("name:").nth(1) {
                if let Some(name) = first_quoted(rest) {
                    names.push(name);
                    in_meta = false;
                }
            }
        }
    }
    names
}

fn first_quoted(text: &str) -> Option<String> {
    let start = text.find('"')?;
    let rest = &text[start + 1..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn arch_check() -> Result<()> {
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(workspace_root())
        .output()
        .map_err(|err| format!("执行 cargo metadata 失败: {err}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|err| err.to_string())?;
    let packages = metadata["packages"]
        .as_array()
        .ok_or_else(|| "cargo metadata 缺少 packages".to_string())?;

    let mut violations = Vec::new();
    for package in packages {
        let name = package["name"].as_str().unwrap_or_default();
        let dependencies = package["dependencies"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        for dependency in dependencies {
            if dependency["kind"].as_str() == Some("dev") {
                continue;
            }
            let dep_name = dependency["name"].as_str().unwrap_or_default();
            if forbidden(name, dep_name) {
                violations.push(format!("{name} -> {dep_name}"));
            }
        }
    }

    if violations.is_empty() {
        println!("arch-check 通过：依赖方向符合分层约束");
        Ok(())
    } else {
        Err(format!("发现非法依赖方向:\n{}", violations.join("\n")))
    }
}

fn forbidden(package: &str, dependency: &str) -> bool {
    let dep_is_tool = dependency.starts_with("rscmd-tool-");
    match package {
        "rscmd-core" => dependency.starts_with("rscmd-"),
        "rscmd-runtime" => {
            dep_is_tool || matches!(dependency, "rscmd-tools" | "rscmd-cli" | "rscmd-py")
        }
        _ if package.starts_with("rscmd-tool-") => {
            (dep_is_tool && dependency != package)
                || matches!(dependency, "rscmd-tools" | "rscmd-cli" | "rscmd-py")
        }
        "rscmd-tools" => matches!(dependency, "rscmd-cli" | "rscmd-py"),
        "rscmd-py" => dependency == "rscmd-cli" || dependency.starts_with("rscmd-app-"),
        "rscmd-cli" => dependency == "rscmd-py",
        _ => false,
    }
}

fn generate_docs() -> String {
    let mut out = String::from(
        "# 工具索引（自动生成，勿手改）\n\n\
         本文件由 `cargo xtask gen-docs` 生成；修改工具元数据后需重新生成。\n\n\
         | 工具 | 别名 | 摘要 | 副作用 | Python |\n\
         |---|---|---|---|---|\n",
    );
    for tool in rscmd_tools::tools() {
        let aliases = if tool.meta.aliases.is_empty() {
            "-".to_string()
        } else {
            tool.meta
                .aliases
                .iter()
                .map(|alias| format!("`{alias}`"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        out.push_str(&format!(
            "| `{}` | {} | {} | `{}` | `{}` |\n",
            tool.meta.name,
            aliases,
            tool.meta.summary.replace('|', "\\|"),
            tool.meta.side_effects.as_str(),
            tool.meta.python.as_str()
        ));
    }
    out
}

fn generate_global_args_doc() -> String {
    let schema_json = rscmd_core::global_schema();
    let schema: serde_json::Value = serde_json::from_str(&schema_json).unwrap_or_default();
    let props = schema.get("properties").and_then(|v| v.as_object());

    let mut out = String::from(
        "# CLI 全局参数（自动生成，勿手改）\n\n\
         本文件由 `cargo xtask gen-docs` 生成，Schema 来自 `GlobalArgs` 的 `JsonSchema derive`。\n\n\
         所有全局参数以 `_rscmd_` 内部 id 注册，避免与工具自身参数冲突。\n\n\
         ```\n\
         完整 JSON Schema: schema/global.json\n\
         ```\n\n\
         | 参数 | 类型 | 默认 | 说明 |\n\
         |---|---|---|---|\n",
    );

    if let Some(props) = props {
        for (name, def) in props {
            let ty = def.get("type").and_then(|v| v.as_str()).unwrap_or("—");
            let default = def
                .get("default")
                .and_then(|v| serde_json::to_string(v).ok())
                .unwrap_or_else(|| "—".to_string());
            let desc = def
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("—")
                .replace('|', "\\|");
            out.push_str(&format!("| `--{name}` | {ty} | `{default}` | {desc} |\n"));
        }
    }
    out
}

fn format_command(name: &str, aliases: &[&str]) -> String {
    if aliases.is_empty() {
        format!("`rscmd {name}`")
    } else {
        format!("`rscmd {name}({})`", aliases.join(", "))
    }
}

fn generate_commands_doc() -> String {
    let mut out = String::from(
        "# 命令一览（自动生成，勿手改）\n\n\
         本文件由 `cargo xtask gen-docs` 生成；内建命令的短别名以括号标注，如 `rscmd doctor(d)`；\n\
         工具命令只显示规范名，别名见 [tools.md](./tools.md)。\n\n\
         ## 内建命令\n\n\
         | 命令 | 说明 |\n\
         |---|---|\n",
    );
    for builtin in rscmd_core::registry::BUILTINS {
        out.push_str(&format!(
            "| {} | {} |\n",
            format_command(builtin.name, builtin.aliases),
            builtin.about
        ));
    }

    out.push_str("\n## 工具命令\n\n| 命令 | 说明 |\n|---|---|\n");
    for tool in rscmd_tools::tools() {
        out.push_str(&format!(
            "| `rscmd {}` | {} |\n",
            tool.meta.name,
            tool.meta.summary.replace('|', "\\|")
        ));
    }
    out
}

fn generate_tool_schemas_doc() -> String {
    let mut out = String::from(
        "# 工具参数 Schema 摘要（自动生成，勿手改）\n\n\
         本文件由 `cargo xtask gen-docs` 生成；完整 schema 见 `schema/*.json`。\n\n\
         需要机器可读的全量自描述时，运行 `rscmd schema --json`：\n\
         `data` 为 `rscmd-schema/1` 对象，含 `version`、`tools`、`builtins`、`global` 四个字段。\n\n\
         只列工具时运行 `rscmd tools --json`：`data` 为 `rscmd-tools/1` 对象，含 `version` 与 `tools`，\n\
         每个条目与 `schema.tools` 同源（含 `aliases` 与 `side_effects`）。\n\n",
    );
    for tool in rscmd_tools::tools() {
        let Some(schema_fn) = tool.schema else {
            continue;
        };
        let schema_json = schema_fn();
        let schema: serde_json::Value = serde_json::from_str(&schema_json).unwrap_or_default();
        let title = schema
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or(tool.meta.name);
        out.push_str(&format!("## {title}\n\n"));
        let Some(props) = schema.get("properties").and_then(|v| v.as_object()) else {
            out.push_str("（无参数）\n\n");
            continue;
        };
        out.push_str("| 参数 | 类型 | 默认 | 必填 | 说明 |\n|---|---|---|---|---|\n");
        let required = schema
            .get("required")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        for (name, def) in props {
            let ty = def.get("type").and_then(|v| v.as_str()).unwrap_or("—");
            let default = def
                .get("default")
                .and_then(|v| serde_json::to_string(v).ok())
                .unwrap_or_else(|| "—".to_string());
            let is_required = required.iter().any(|r| r.as_str() == Some(name));
            let req_mark = if is_required { "✅" } else { "-" };
            let desc = def
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("—")
                .replace('|', "\\|");
            out.push_str(&format!(
                "| `{name}` | {ty} | `{default}` | {req_mark} | {desc} |\n"
            ));
        }
        out.push('\n');
    }
    out
}

fn generate_readme() -> String {
    let tool_count = rscmd_tools::tools().len();
    format!(
        "# rscmd 文档索引（自动生成，勿手改）\n\n\
         本文件由 `cargo xtask gen-docs` 生成。\n\n\
         ## 文件列表\n\n\
         - [commands.md](./commands.md) — 内建命令与工具命令一览（含短别名）\n\
         - [tools.md](./tools.md) — 全部工具元数据表格\n\
         - [global-args.md](./global-args.md) — CLI 全局参数说明\n\
         - [schemas.md](./schemas.md) — 各工具参数 Schema 摘要\n\n\
         ## 快速校验\n\n\
         ```bash\n\
         cargo xtask check-fast        # fmt + clippy + registry + arch + docs + schema + py-fresh + deny + machete\n\
         cargo xtask py-type-check     # mypy 检查 python/rscmd 与 tests/py（未装 mypy 时跳过）\n\
         cargo xtask wheel-smoke       # 构建 wheel、安装进 .venv 并冒烟\n\
         cargo xtask bench             # 启动时间基准回归\n\
         cargo xtask release-preflight # 发布前全量预检\n\
         ```\n\n\
         ## Python 绑定\n\n\
         `python/rscmd/_native.*` 是 `maturin develop` 的产物，console script 与 `python -m rscmd`\n\
         都直接复用它。Rust 侧改动后必须重建，否则入口会静默跑旧 CLI：\n\n\
         ```bash\n\
         maturin develop              # 或 .venv/Scripts/maturin.exe develop\n\
         uv tool install --editable .\n\
         ```\n\n\
         `cargo xtask py-fresh-check` 会在 `_native.*` 旧于 Rust 源码时失败并提示重建；本地未构建时跳过。\n\n\
         类型检查选 mypy（纯 Python 实现，`pip install mypy` 即可，CI 不需要额外运行时），\n\
         配置在 `pyproject.toml` 的 `[tool.mypy]`，原生边界由 `python/rscmd/_native.pyi` 声明。\n\
         CI 的 `python` job 跑 mypy 与 pytest，`wheel-smoke` job 在三平台构建 wheel 并冒烟。\n\n\
         ## 当前状态\n\n\
         - 工具数量：**{tool_count}**\n\
         - Schema 文件：`schema/` 下 {} 个 `.json`\n",
        schema_file_count()
    )
}

fn schema_file_count() -> usize {
    let dir = workspace_root().join("schema");
    fs::read_dir(&dir)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.path().extension().and_then(|e| e.to_str()) == Some("json"))
                .count()
        })
        .unwrap_or(0)
}

type DocGen = fn() -> String;

fn docs_check(write: bool) -> Result<()> {
    let docs_dir = workspace_root().join("docs");
    let files: &[(&str, DocGen)] = &[
        ("commands.md", generate_commands_doc),
        ("tools.md", generate_docs),
        ("global-args.md", generate_global_args_doc),
        ("schemas.md", generate_tool_schemas_doc),
        ("README.md", generate_readme),
    ];

    if write {
        fs::create_dir_all(&docs_dir).map_err(|err| err.to_string())?;
        for (name, gen) in files {
            let path = docs_dir.join(name);
            fs::write(&path, gen()).map_err(|err| err.to_string())?;
            println!("已生成 {}", path.display());
        }
        return Ok(());
    }

    let mut all_ok = true;
    for (name, gen) in files {
        let path = docs_dir.join(name);
        let expected = gen();
        let actual = fs::read_to_string(&path)
            .map_err(|_| format!("缺少 {}，请运行 `cargo xtask gen-docs`", path.display()))?;
        if normalize_newlines(&actual) != expected {
            eprintln!("{} 已过期，请运行 `cargo xtask gen-docs`", path.display());
            all_ok = false;
        }
    }
    if !all_ok {
        return Err("部分 docs/*.md 已过期".to_string());
    }
    println!("docs-check 通过：{} 个文件都是最新生成物", files.len());
    Ok(())
}

fn schema_check(write: bool) -> Result<()> {
    let schema_dir = workspace_root().join("schema");
    let tools = rscmd_tools::tools();

    // 1. 生成 meta + global schema
    let meta_path = schema_dir.join("meta.json");
    let meta_schema = rscmd_core::meta_schema();
    let global_path = schema_dir.join("global.json");
    let global_schema = rscmd_core::global_schema();

    // 2. 生成每个工具的参数 schema
    let mut tool_schemas = Vec::with_capacity(tools.len());
    for tool in tools {
        let schema_fn = tool.schema.ok_or_else(|| {
            format!(
                "工具 `{}` 未注册 schema 函数（ToolDef::schema 为 None）",
                tool.meta.name
            )
        })?;
        tool_schemas.push((tool.meta.name, schema_fn()));
    }

    if write {
        fs::create_dir_all(&schema_dir).map_err(|err| err.to_string())?;
        fs::write(&meta_path, &meta_schema).map_err(|err| err.to_string())?;
        println!("已生成 {}", meta_path.display());
        fs::write(&global_path, &global_schema).map_err(|err| err.to_string())?;
        println!("已生成 {}", global_path.display());
        for (name, schema) in &tool_schemas {
            let path = schema_dir.join(format!("{name}.json"));
            fs::write(&path, schema).map_err(|err| err.to_string())?;
            println!("已生成 {}", path.display());
        }
        return Ok(());
    }

    // 校验：meta / global schema
    verify_generated(&meta_path, &meta_schema)?;
    verify_generated(&global_path, &global_schema)?;

    // 校验：每个工具 schema
    for (name, expected) in &tool_schemas {
        let path = schema_dir.join(format!("{name}.json"));
        verify_generated(&path, expected)?;
    }

    // 校验：schema 目录下没有多余文件
    let mut expected_names = BTreeSet::new();
    expected_names.insert("meta.json".to_string());
    expected_names.insert("global.json".to_string());
    for (name, _) in &tool_schemas {
        expected_names.insert(format!("{name}.json"));
    }
    let mut actual_names = BTreeSet::new();
    for entry in fs::read_dir(&schema_dir).map_err(|err| err.to_string())? {
        let entry = entry.map_err(|err| err.to_string())?;
        if entry.path().extension().and_then(|e| e.to_str()) == Some("json") {
            actual_names.insert(entry.file_name().to_string_lossy().to_string());
        }
    }
    let extra: Vec<_> = actual_names.difference(&expected_names).cloned().collect();
    if !extra.is_empty() {
        return Err(format!(
            "schema/ 目录下有多余文件: {}（请删除或检查 gen-schema）",
            extra.join(", ")
        ));
    }

    println!(
        "schema-check 通过：{} 个工具 + meta + global schema 都是最新生成物",
        tool_schemas.len()
    );
    Ok(())
}

fn verify_generated(path: &Path, expected: &str) -> Result<()> {
    let actual = fs::read_to_string(path)
        .map_err(|_| format!("缺少 {}，请运行 `cargo xtask gen-schema`", path.display()))?;
    if normalize_newlines(&actual) != expected {
        return Err(format!(
            "{} 已过期，请运行 `cargo xtask gen-schema`",
            path.display()
        ));
    }
    Ok(())
}

/// 生成物比较前归一化换行：Windows 的 `core.autocrlf` 会把检出内容变成 CRLF，
/// 而生成器统一写 LF，不归一化会在 Windows CI 上误报「已过期」。
fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn check_fast() -> Result<()> {
    run_cargo(&["fmt", "--all", "--", "--check"])?;
    run_cargo(&[
        "clippy",
        "--workspace",
        "--all-targets",
        "--",
        "-D",
        "warnings",
    ])?;
    registry_check()?;
    code_check()?;
    arch_check()?;
    docs_check(false)?;
    schema_check(false)?;
    py_fresh_check()?;
    // 外部工具缺失时显式跳过并打印提示，不静默通过。
    deny_check()?;
    run_optional("cargo-machete", "cargo machete", &["machete"])?;
    println!("check-fast 通过");
    Ok(())
}

/// 许可证 / 重复依赖 / 源检查不依赖网络；advisories 需要本地 RustSec 数据库。
fn deny_check() -> Result<()> {
    if !has_command("cargo-deny") {
        println!("跳过 `cargo deny`：未安装 cargo-deny（cargo install cargo-deny）");
        return Ok(());
    }
    run_cargo(&["deny", "check", "licenses", "bans", "sources"])?;
    if advisory_db_available() {
        run_cargo(&["deny", "check", "advisories"])
    } else {
        println!(
            "跳过 advisories：未找到本地 RustSec 数据库（联网环境请运行 `cargo deny check advisories`）"
        );
        Ok(())
    }
}

fn advisory_db_available() -> bool {
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".cargo")));
    let Some(root) = cargo_home else {
        return false;
    };
    fs::read_dir(root.join("advisory-dbs"))
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .any(|entry| entry.path().is_dir())
        })
        .unwrap_or(false)
}

fn check() -> Result<()> {
    check_fast()?;
    if has_command("cargo-nextest") {
        run_cargo(&["nextest", "run", "--workspace"])?;
    } else {
        println!("提示：未安装 cargo-nextest，回退到 cargo test");
        run_cargo(&["test", "--workspace"])?;
    }
    py_check()?;
    py_type_check()?;
    println!("check 通过");
    Ok(())
}

fn has_command(name: &str) -> bool {
    Command::new("cargo")
        .args([name.trim_start_matches("cargo-"), "--version"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn run_optional(install_name: &str, command: &str, args: &[&str]) -> Result<()> {
    let Some(bin) = cargo_plugin_path(install_name) else {
        println!("跳过 `{command}`：未安装 {install_name}（cargo install {install_name}）");
        return Ok(());
    };
    println!("$ {command}");
    let status = Command::new(&bin)
        .args(&args[1..])
        .current_dir(workspace_root())
        .status()
        .map_err(|err| format!("执行 {command} 失败: {err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{command} 退出码 {:?}", status.code()))
    }
}

/// 定位 cargo 插件可执行文件：先查 `$CARGO_HOME/bin`，再回退 PATH。
/// 直接调用可执行文件而不经 `cargo <sub>`，避免个别插件对子命令参数的错误解析。
fn cargo_plugin_path(plugin: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) {
        format!("{plugin}.exe")
    } else {
        plugin.to_string()
    };
    let mut dirs = Vec::new();
    if let Some(home) = std::env::var_os("CARGO_HOME") {
        dirs.push(PathBuf::from(home).join("bin"));
    }
    if let Some(home) = std::env::var_os("USERPROFILE") {
        dirs.push(PathBuf::from(home).join(".cargo").join("bin"));
    }
    dirs.into_iter()
        .map(|dir| dir.join(&exe))
        .find(|path| path.exists())
        .or_else(|| which(plugin).then(|| PathBuf::from(plugin)))
}

/// Python 原生产物新鲜度：本地已构建的 `_native.*` 若旧于 Rust 源码，
/// console script / `python -m` 会静默跑旧 CLI，这里提前失败并给出重建命令。
fn py_fresh_check() -> Result<()> {
    let root = workspace_root();
    let Some(native) = native_artifact(&root) else {
        println!("跳过产物新鲜度检查：未发现 python/rscmd/_native.*（本地未构建原生模块）");
        return Ok(());
    };
    let native_mtime = fs::metadata(&native)
        .and_then(|meta| meta.modified())
        .map_err(|err| format!("读取 {} 修改时间失败: {err}", native.display()))?;

    let Some((newest_path, newest_mtime)) = newest_input(&root)? else {
        println!("产物新鲜度检查通过：未找到 Rust 源文件");
        return Ok(());
    };
    if newest_mtime > native_mtime {
        return Err(format!(
            "`{}` 旧于最新源码 `{}`。\n\
             请运行 `maturin develop` 重建原生模块，否则 Python 入口会静默退回旧 CLI。",
            native.display(),
            newest_path.display()
        ));
    }
    println!("产物新鲜度检查通过：{}", native.display());
    Ok(())
}

fn native_artifact(root: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(root.join("python/rscmd")).ok()?;
    entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .find(|path| {
            path.file_stem().and_then(|stem| stem.to_str()) == Some("_native")
                && matches!(
                    path.extension().and_then(|ext| ext.to_str()),
                    Some("pyd" | "so" | "dylib")
                )
        })
}

/// 影响原生模块的输入：`crates/` 下的 Rust 源码与 Cargo.toml，加上根清单。
/// `xtask`（仓库工具）与 `rscmd-cli`（独立二进制）不参与扩展模块构建，排除以免误报。
fn newest_input(root: &Path) -> Result<Option<(PathBuf, SystemTime)>> {
    let mut newest: Option<(PathBuf, SystemTime)> = None;
    let crates_dir = root.join("crates");
    for entry in fs::read_dir(&crates_dir)
        .map_err(|err| format!("读取 {} 失败: {err}", crates_dir.display()))?
        .filter_map(|entry| entry.ok())
    {
        let dir = entry.path();
        let name = dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if !dir.is_dir() || matches!(name, "xtask" | "rscmd-cli") {
            continue;
        }
        collect_inputs(&dir, &mut newest)?;
    }
    for name in ["Cargo.toml", "pyproject.toml"] {
        let path = root.join(name);
        if let Ok(modified) = fs::metadata(&path).and_then(|meta| meta.modified()) {
            push_newest(&path, modified, &mut newest);
        }
    }
    Ok(newest)
}

fn collect_inputs(dir: &Path, newest: &mut Option<(PathBuf, SystemTime)>) -> Result<()> {
    for entry in fs::read_dir(dir)
        .map_err(|err| format!("读取 {} 失败: {err}", dir.display()))?
        .filter_map(|entry| entry.ok())
    {
        let path = entry.path();
        if path.is_dir() {
            collect_inputs(&path, newest)?;
            continue;
        }
        let is_input = match path.file_name().and_then(|name| name.to_str()) {
            Some("Cargo.toml") => true,
            Some(name) => name.ends_with(".rs"),
            None => false,
        };
        if !is_input {
            continue;
        }
        if let Ok(modified) = fs::metadata(&path).and_then(|meta| meta.modified()) {
            push_newest(&path, modified, newest);
        }
    }
    Ok(())
}

fn push_newest(path: &Path, modified: SystemTime, newest: &mut Option<(PathBuf, SystemTime)>) {
    if newest
        .as_ref()
        .map(|(_, time)| modified > *time)
        .unwrap_or(true)
    {
        *newest = Some((path.to_path_buf(), modified));
    }
}

fn py_check() -> Result<()> {
    let root = workspace_root();
    let candidates = [
        root.join(".venv/Scripts/python.exe"),
        root.join(".venv/bin/python"),
    ];
    let Some(python) = candidates.iter().find(|path| path.exists()) else {
        println!("跳过 Python 测试：未发现 .venv（可用 uv venv .venv 创建）");
        return Ok(());
    };

    println!("$ {} -m pytest tests/py -q", python.display());
    let status = Command::new(python)
        .args(["-m", "pytest", "tests/py", "-q"])
        .current_dir(&root)
        .status()
        .map_err(|err| format!("执行 Python 测试失败: {err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("Python 测试失败，退出码 {:?}", status.code()))
    }
}

/// Python 侧类型检查：配置见 `pyproject.toml` 的 `[tool.mypy]`。
/// mypy 属测试工具，本地未安装时跳过；CI 的 python job 显式安装并执行。
fn py_type_check() -> Result<()> {
    let root = workspace_root();
    let Some(python) = venv_python(&root) else {
        println!("跳过 Python 类型检查：未发现 .venv（可用 uv venv .venv 创建）");
        return Ok(());
    };
    let probe = Command::new(&python)
        .args(["-m", "mypy", "--version"])
        .output();
    if !matches!(probe, Ok(ref output) if output.status.success()) {
        println!("跳过 Python 类型检查：.venv 中未安装 mypy（uv pip install mypy）");
        return Ok(());
    }

    println!("$ {} -m mypy", python.display());
    let status = Command::new(&python)
        .args(["-m", "mypy"])
        .current_dir(&root)
        .status()
        .map_err(|err| format!("执行 mypy 失败: {err}"))?;
    if status.success() {
        println!("py-type-check 通过");
        Ok(())
    } else {
        Err(format!("Python 类型检查失败，退出码 {:?}", status.code()))
    }
}

fn check_cov() -> Result<()> {
    let probe = Command::new("cargo")
        .args(["llvm-cov", "--version"])
        .output();
    match probe {
        Ok(output) if output.status.success() => run_cargo(&[
            "llvm-cov",
            "--workspace",
            "--ignore-filename-regex",
            "rscmd-py[\\\\/]|xtask[\\\\/]",
            "--fail-under-lines",
            "95",
        ]),
        _ => Err(
            "未安装 cargo-llvm-cov；请先 `rustup component add llvm-tools-preview` 并 \
             `cargo install cargo-llvm-cov`"
                .to_string(),
        ),
    }
}

/// 构建 wheel、安装进 `.venv`，然后验证导入与一次成功/一次失败的调用。
fn wheel_smoke() -> Result<()> {
    let root = workspace_root();
    let python =
        venv_python(&root).ok_or_else(|| "未发现 .venv，请先运行 `uv venv .venv`".to_string())?;

    let maturin = maturin_bin(&root)
        .ok_or_else(|| "未找到 maturin，请先 `uv pip install maturin`".to_string())?;

    let dist = root.join("dist");
    fs::create_dir_all(&dist).map_err(|err| err.to_string())?;

    println!("$ maturin build --release --out dist");
    let status = Command::new(&maturin)
        .args(["build", "--release", "--out"])
        .arg(&dist)
        .current_dir(&root)
        .status()
        .map_err(|err| format!("执行 maturin 失败: {err}"))?;
    if !status.success() {
        return Err(format!("maturin build 退出码 {:?}", status.code()));
    }

    let wheel =
        newest_wheel(&dist).ok_or_else(|| format!("{} 下没有找到 wheel", dist.display()))?;

    println!("$ 安装 {}", wheel.display());
    let installed = if which("uv") {
        Command::new("uv")
            .arg("pip")
            .arg("install")
            .arg("--python")
            .arg(&python)
            .arg("--force-reinstall")
            .arg("--no-deps")
            .arg(&wheel)
            .current_dir(&root)
            .status()
    } else {
        Command::new(&python)
            .args(["-m", "pip", "install", "--force-reinstall", "--no-deps"])
            .arg(&wheel)
            .current_dir(&root)
            .status()
    }
    .map_err(|err| format!("安装 wheel 失败: {err}"))?;
    if !installed.success() {
        return Err(format!("wheel 安装退出码 {:?}", installed.code()));
    }

    let script = "import rscmd\n\
                  assert rscmd.__version__ == '__EXPECTED_VERSION__', rscmd.__version__\n\
                  assert rscmd.invoke('case', {'text': 'HelloWorld', 'mode': 'snake'})['text'] == 'hello_world'\n\
                  assert rscmd.run_argv(['tools'])['data']['schema'] == 'rscmd-tools/1'\n\
                  try:\n\
                  \x20   rscmd.invoke('json_fmt', {'json': '{'})\n\
                  except rscmd.RscmdError as err:\n\
                  \x20   assert err.code == 'INVALID_JSON'\n\
                  else:\n\
                  \x20   raise AssertionError('json_fmt 应报错')\n\
                  print('wheel smoke 通过')";
    let script = script.replace("__EXPECTED_VERSION__", env!("CARGO_PKG_VERSION"));
    println!("$ python -c <wheel smoke>");
    let smoke = Command::new(&python)
        .arg("-c")
        .arg(script)
        // Windows 控制台默认 cp1252/cp936，脚本里的中文会触发 UnicodeEncodeError；
        // 固定 UTF-8 输出，与 CI 日志编码一致。
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUTF8", "1")
        .current_dir(&root)
        .status()
        .map_err(|err| format!("执行 wheel 冒烟失败: {err}"))?;
    if !smoke.success() {
        return Err(format!("wheel 冒烟退出码 {:?}", smoke.code()));
    }
    println!("wheel-smoke 通过：{}", wheel.display());
    Ok(())
}

fn venv_python(root: &Path) -> Option<PathBuf> {
    [
        root.join(".venv/Scripts/python.exe"),
        root.join(".venv/bin/python"),
    ]
    .into_iter()
    .find(|path| path.exists())
}

fn maturin_bin(root: &Path) -> Option<PathBuf> {
    let candidates = [
        root.join(".venv/Scripts/maturin.exe"),
        root.join(".venv/bin/maturin"),
    ];
    if let Some(path) = candidates.into_iter().find(|path| path.exists()) {
        return Some(path);
    }
    which("maturin").then(|| PathBuf::from("maturin"))
}

fn which(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn newest_wheel(dir: &Path) -> Option<PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("whl"))
        .max_by_key(|path| fs::metadata(path).and_then(|meta| meta.modified()).ok())
}

/// 版本一致性校验：tag（可选）与 Cargo workspace 版本一致；`dist/*.whl` 文件名版本一致；
/// `CHANGELOG.md` 存在该版本的段落标题。
fn version_check() -> Result<()> {
    let root = workspace_root();
    let expected = workspace_version(&root)?;

    let changelog = fs::read_to_string(root.join("CHANGELOG.md"))
        .map_err(|err| format!("读取 CHANGELOG.md 失败: {err}"))?;
    if !changelog_has_version(&changelog, &expected) {
        return Err(format!(
            "CHANGELOG.md 缺少 [{expected}] 段落，请按版本补写更新日志后再发布"
        ));
    }
    println!("CHANGELOG 校验通过: [{expected}]");

    let tag = std::env::var("RSCMD_RELEASE_TAG")
        .ok()
        .or_else(|| std::env::var("GITHUB_REF_NAME").ok())
        .filter(|tag| !tag.trim().is_empty());
    match tag.as_deref() {
        Some(tag) => {
            let trimmed = tag.trim();
            let actual = trimmed.strip_prefix('v').unwrap_or(trimmed);
            if actual != expected {
                return Err(format!("tag {trimmed} 与 Cargo 版本 {expected} 不一致"));
            }
            println!("tag 校验通过: {trimmed}");
        }
        None => println!("跳过 tag 校验：未设置 RSCMD_RELEASE_TAG 或 GITHUB_REF_NAME"),
    }

    let dist = root.join("dist");
    let mut wheels = 0usize;
    if dist.is_dir() {
        let mut names: Vec<String> = fs::read_dir(&dist)
            .map_err(|err| format!("读取 {} 失败: {err}", dist.display()))?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".whl"))
            .collect();
        names.sort();
        for name in &names {
            let version = wheel_version(name)
                .ok_or_else(|| format!("无法从 wheel 文件名解析版本: {name}"))?;
            if version != expected {
                return Err(format!(
                    "wheel {name} 版本 {version} 与 Cargo 版本 {expected} 不一致"
                ));
            }
            wheels += 1;
        }
    }
    println!("版本校验通过: Cargo {expected}，wheel {wheels} 个");
    Ok(())
}

/// 读取根 `Cargo.toml` 中 `[workspace.package] version`。
fn workspace_version(root: &Path) -> Result<String> {
    let text = fs::read_to_string(root.join("Cargo.toml"))
        .map_err(|err| format!("读取 Cargo.toml 失败: {err}"))?;
    let mut in_package = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[workspace.package]";
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some(rest) = line.strip_prefix("version") {
            if let Some(value) = rest.trim_start().strip_prefix('=') {
                let value = value.trim().trim_matches('"');
                if !value.is_empty() {
                    return Ok(value.to_string());
                }
            }
        }
    }
    Err("Cargo.toml 缺少 [workspace.package].version".to_string())
}

/// 从 `rscmd-0.1.0-cp310-abi3-win_amd64.whl` 中取出 `0.1.0`。
fn wheel_version(file_name: &str) -> Option<&str> {
    let rest = file_name.strip_prefix("rscmd-")?;
    let (version, _) = rest.split_once('-')?;
    (!version.is_empty()).then_some(version)
}

/// CHANGELOG 是否存在 `## [<version>]` 段落（允许其后跟日期等说明，如 `## [0.1.0] - 2026-11-16`）。
fn changelog_has_version(text: &str, version: &str) -> bool {
    let prefix = format!("[{version}]");
    text.lines().any(|line| {
        let Some(rest) = line.trim_start().strip_prefix("## ") else {
            return false;
        };
        let rest = rest.trim_start();
        match rest.strip_prefix(&prefix) {
            // `[0.1.0]` 之后必须是行尾或空格，避免把 `[0.1.00]` 当成命中。
            Some(tail) => tail.is_empty() || tail.starts_with(' '),
            None => false,
        }
    })
}

fn run_cargo(args: &[&str]) -> Result<()> {
    println!("$ cargo {}", args.join(" "));
    let status = Command::new("cargo")
        .args(args)
        .current_dir(workspace_root())
        .status()
        .map_err(|err| format!("执行 cargo {} 失败: {err}", args.join(" ")))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "cargo {} 退出码 {:?}",
            args.join(" "),
            status.code()
        ))
    }
}

/// 启动时间基准回归：构建 release profile，跑 `rscmd --version` N 次，
/// 输出 p50/p95/p99，若 median 超过基线 × 2 则失败。
///
/// 基线（2026-11-15，P0 实测）：median ~6.8ms，阈值设 20ms。
/// 每次 P 级别的大改动应更新此基线。
const BENCH_ITERATIONS: usize = 30;
const BENCH_BASELINE_MEDIAN_MS: u128 = 10;
const BENCH_FAILURE_MULTIPLIER: u128 = 2;

fn bench() -> Result<()> {
    run_cargo(&["build", "--release", "-p", "rscmd-cli"])?;

    let exe = workspace_root()
        .join("target")
        .join("release")
        .join(if cfg!(windows) { "rscmd.exe" } else { "rscmd" });
    if !exe.exists() {
        return Err(format!("找不到 release 二进制: {}", exe.display()));
    }

    let mut samples = Vec::with_capacity(BENCH_ITERATIONS);
    println!(
        "\n基准：{} 次 {} --version",
        BENCH_ITERATIONS,
        exe.display()
    );
    println!(
        "基线 median ≤ {}ms，超过 {}ms 触发失败",
        BENCH_BASELINE_MEDIAN_MS,
        BENCH_BASELINE_MEDIAN_MS * BENCH_FAILURE_MULTIPLIER
    );

    for i in 0..BENCH_ITERATIONS {
        let start = std::time::Instant::now();
        let status = Command::new(&exe)
            .arg("--version")
            .status()
            .map_err(|err| format!("执行 {} 失败: {err}", exe.display()))?;
        let elapsed = start.elapsed().as_millis();
        if !status.success() {
            return Err(format!(
                "bench 中第 {i} 次 `--version` 退出码 {:?}",
                status.code()
            ));
        }
        samples.push(elapsed);
    }

    samples.sort();
    let median = percentile(&samples, 50);
    let p95 = percentile(&samples, 95);
    let p99 = percentile(&samples, 99);
    let min = samples.first().copied().unwrap_or(0);
    let max = samples.last().copied().unwrap_or(0);
    let avg = samples.iter().sum::<u128>() as f64 / samples.len() as f64;

    println!("\n  min   p50   p95   p99   max   avg");
    println!(
        "  {}    {}    {}    {}    {}    {:.1}",
        min, median, p95, p99, max, avg
    );

    let threshold = BENCH_BASELINE_MEDIAN_MS * BENCH_FAILURE_MULTIPLIER;
    if median > threshold {
        return Err(format!(
            "median {median}ms 超过阈值 {threshold}ms（基线 {BENCH_BASELINE_MEDIAN_MS}ms × {BENCH_FAILURE_MULTIPLIER}）"
        ));
    }
    println!("bench 通过");
    Ok(())
}

fn percentile(sorted: &[u128], p: u8) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() - 1) as f64 * p as f64 / 100.0).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// 发布前全量预检：check-fast → test → py-check → 覆盖率 → bench → wheel-smoke → 版本校验。
/// 任一步骤失败立即中止，避免在有问题的状态下继续发布。
pub fn release_preflight() -> Result<()> {
    println!("=== release-preflight 开始 ===");
    check_fast()?;
    run_cargo(&["test", "--workspace"])?;
    py_check()?;
    check_cov()?;
    bench()?;
    wheel_smoke()?;
    version_check()?;
    println!("=== release-preflight 通过，可以发布 ===");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{changelog_has_version, coded_literals, normalize_newlines, wheel_version};

    #[test]
    fn normalizes_crlf_before_compare() {
        assert_eq!(normalize_newlines("a\r\nb\r\n"), "a\nb\n");
        assert_eq!(normalize_newlines("a\nb"), "a\nb");
    }

    #[test]
    fn parses_wheel_version() {
        assert_eq!(
            wheel_version("rscmd-0.1.0-cp310-abi3-win_amd64.whl"),
            Some("0.1.0")
        );
        assert_eq!(
            wheel_version("rscmd-2.0.0-cp310-abi3-manylinux_2_17_x86_64.whl"),
            Some("2.0.0")
        );
    }

    #[test]
    fn rejects_foreign_wheel_name() {
        assert_eq!(wheel_version("other-0.1.0-py3-none-any.whl"), None);
        assert_eq!(wheel_version("rscmd.whl"), None);
    }

    #[test]
    fn finds_changelog_section_with_or_without_date() {
        let text = "# 更新日志\n\n## [0.1.0] - 2026-11-16\n\n- 内容\n";
        assert!(changelog_has_version(text, "0.1.0"));
        assert!(!changelog_has_version(text, "0.2.0"));

        let bare = "## [0.2.0]\n- 内容\n";
        assert!(changelog_has_version(bare, "0.2.0"));
    }

    #[test]
    fn changelog_section_match_is_whole_version() {
        let text = "## [0.1.00] - 2026-11-16\n### [0.1.0] 子标题\n";
        assert!(!changelog_has_version(text, "0.1.0"));
    }

    #[test]
    fn collects_coded_code_literals_with_line_numbers() {
        let source = "let a = RscmdError::coded(\"USAGE\", \"x\");\n\
                      // 注释里的 coded(\"IGNORED\") 也算字面量\n\
                      let b = RscmdError::coded(\"TIMEOUT\", format!(\"y\"));\n\
                      let c = RscmdError::coded(code, \"动态\");\n";
        let found = coded_literals(source);
        assert_eq!(
            found,
            vec![
                (1, "USAGE".to_string()),
                (2, "IGNORED".to_string()),
                (3, "TIMEOUT".to_string())
            ]
        );
    }

    #[test]
    fn coded_literals_handles_multiple_per_line_and_unterminated() {
        let source = "coded(\"A\") ... coded(\"B\"); coded(\"C\n";
        let found = coded_literals(source);
        assert_eq!(found, vec![(1, "A".to_string()), (1, "B".to_string())]);
    }
}
