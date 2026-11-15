//! 仓库工具：注册表校验、依赖方向校验、生成物与门禁。

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

type Result<T> = std::result::Result<T, String>;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("help");
    let result = match command {
        "registry-check" => registry_check(),
        "arch-check" => arch_check(),
        "docs-check" => docs_check(false),
        "gen-docs" => docs_check(true),
        "schema-check" => schema_check(false),
        "gen-schema" => schema_check(true),
        "check-fast" => check_fast(),
        "check" => check(),
        "check-cov" => check_cov(),
        "wheel-smoke" => wheel_smoke(),
        "bench" => bench(),
        "release-preflight" => release_preflight(),
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
         \x20 arch-check      校验 crate 依赖方向\n\
         \x20 docs-check      校验 docs/tools.md 是否为最新生成物\n\
         \x20 gen-docs        重新生成 docs/tools.md\n\
         \x20 schema-check    校验 schema/*.json 是否为最新生成物\n\
         \x20 gen-schema      重新生成 schema/*.json\n\
         \x20 check-fast      fmt + clippy + registry-check + arch-check + docs-check + schema-check\n\
         \x20 check           check-fast + 测试 + Python 测试（若存在 .venv）\n\
         \x20 check-cov       运行 cargo llvm-cov（需已安装）\n\
         \x20 wheel-smoke     构建 wheel 并安装到 .venv 做导入与调用冒烟\n\
         \x20 bench           构建 release 并跑启动时间基准回归\n\
         \x20 release-preflight 发布前本地全量预检（check + 覆盖率 + bench + wheel-smoke）"
    );
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("xtask 必须位于 workspace/crates/xtask")
        .to_path_buf()
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

fn generate_tool_schemas_doc() -> String {
    let mut out = String::from(
        "# 工具参数 Schema 摘要（自动生成，勿手改）\n\n\
         本文件由 `cargo xtask gen-docs` 生成；完整 schema 见 `schema/*.json`。\n\n",
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
         - [tools.md](./tools.md) — 全部工具元数据表格\n\
         - [global-args.md](./global-args.md) — CLI 全局参数说明\n\
         - [schemas.md](./schemas.md) — 各工具参数 Schema 摘要\n\n\
         ## 快速校验\n\n\
         ```bash\n\
         cargo xtask check-fast   # fmt + clippy + registry + arch + docs + schema + deny + machete\n\
         cargo xtask bench        # 启动时间基准回归\n\
         cargo xtask release-preflight  # 发布前全量预检\n\
         ```\n\n\
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
        if actual != expected {
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
    if actual != expected {
        return Err(format!(
            "{} 已过期，请运行 `cargo xtask gen-schema`",
            path.display()
        ));
    }
    Ok(())
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
    arch_check()?;
    docs_check(false)?;
    schema_check(false)?;
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
                  assert rscmd.__version__ == '0.1.0', rscmd.__version__\n\
                  assert rscmd.invoke('case', {'text': 'HelloWorld', 'mode': 'snake'})['text'] == 'hello_world'\n\
                  try:\n\
                  \x20   rscmd.invoke('json_fmt', {'json': '{'})\n\
                  except rscmd.RscmdError as err:\n\
                  \x20   assert err.code == 'INVALID_JSON'\n\
                  else:\n\
                  \x20   raise AssertionError('json_fmt 应报错')\n\
                  print('wheel smoke 通过')";
    println!("$ python -c <wheel smoke>");
    let smoke = Command::new(&python)
        .arg("-c")
        .arg(script)
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

/// 发布前全量预检：check-fast → test → py-check → 覆盖率 → bench → wheel-smoke。
/// 任一步骤失败立即中止，避免在有问题的状态下继续发布。
pub fn release_preflight() -> Result<()> {
    println!("=== release-preflight 开始 ===");
    check_fast()?;
    run_cargo(&["test", "--workspace"])?;
    py_check()?;
    check_cov()?;
    bench()?;
    wheel_smoke()?;
    println!("=== release-preflight 通过，可以发布 ===");
    Ok(())
}
