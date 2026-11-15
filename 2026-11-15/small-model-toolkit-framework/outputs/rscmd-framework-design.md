# 面向小模型调用的工具集（rscmd）框架结构方案

- 版本：v1（结构方案，不含实现代码）
- 日期：2026-11-15
- 参考基线：`F:\Dev\fcmd`（实际代码与 ADR，见第 1 节）
- 硬约束：主体用 Rust 设计与实现；同时具备发布为 Python 包的能力
- 命名说明：下文以 `rscmd` 作为工作区/命令/包的暂定名，落地时可整体替换为最终产品名

---

## 0. 结论摘要

1. **单一 Rust workspace，三层基础 crate + 按域划分的工具 crate + 两个交付适配器**：
   `rscmd-core`（契约）→ `rscmd-runtime`（基础件）→ `rscmd-tool-*`（工具域）→
   `rscmd-tools`（聚合）→ `rscmd-cli`（原生 CLI）/ `rscmd-py`（Python 扩展）。
2. **工具集合在编译期确定，不做运行时扫描或动态库插件**。新增“已有域内的工具”只改域 crate
   内部数组，不改 CLI/聚合/Python 绑定；新增域才加一行 feature 与一行汇总。与 fcmd 的
   “清单快路径 + 全量发现回退”相比，Rust 侧把“发现”前移到编译期，启动时只做一次微秒级汇总。
3. **一个多调用二进制 `rscmd`**（`rscmd <tool> ...`），保留 `argv[0]` 别名分发能力，
   不生成每工具一个 exe；Python 侧通过 PyO3 扩展**进程内调用**，不为每次调用起子进程。
4. **Python 包是薄绑定，不是第二套实现**：wheel 内含 `rscmd._native`（abi3），
   暴露 `invoke / run_argv / tools / describe / __version__`；人类可读渲染、补全、诊断、
   TUI/GUI 只留在原生侧。
5. **版本单一来源**：`[workspace.package].version`，maturin 动态读取，Python
   `rscmd.__version__` 由原生侧返回；Rust 二进制与 Python wheel 同 tag、同提交发布。
6. **契约沿用 fcmd 已验证的口径**：成功/错误/自描述三种 JSON 信封、统一退出码
   `0/1/2/3/4/5/130`、默认人读 + `--json` 增量、错误带 `code/field/expected/hint`。
7. **门禁按层与按工具都可独立执行**：`cargo fmt/clippy/nextest/llvm-cov -p <crate>`、
   `pytest -k <tool>`，并用架构测试与 `xtask registry-check` 固化目录/依赖/注册不变量。

---

## 1. 事实基础：fcmd 的实际结构与约定

以下是设计依据，均来自 `F:\Dev\fcmd` 现有代码与文档，不是假设。

| 事实 | 位置 | 对本方案的映射 |
|---|---|---|
| 分层：`core`（任务图/执行引擎）、`gui`（Tk 基座）、`infras`（纯函数基础件）、`tools/<域>`（业务）、`apps/<应用>`（独立桌面应用）、`cli`（唯一组合根） | [0001-layering.md](file:///F:/Dev/fcmd/docs/adr/0001-layering.md) | Rust：`core` → `runtime` → `tool-*` → `cli`/`py` |
| 包内模块深度 ≤ 2 层；单文件域不建子包；`tools/<域>` 保留域层 | [0003-package-layout.md](file:///F:/Dev/fcmd/docs/adr/0003-package-layout.md) | Rust：工具域 crate 内 `src/<tool>.rs` 平铺，不再嵌套模块目录 |
| 工具发现：`fcmd.tools` / `fcmd.apps` 下非 `_` 前缀模块即工具，模块名即工具名；域子包只下探一层；`apps/<应用>` 可用 `__tool_name__` 整包注册 | [cli/_discovery.py](file:///F:/Dev/fcmd/src/fcmd/cli/_discovery.py#L62-L151) | Rust：域 crate 内 `pub static TOOLS`，聚合 crate 再汇总 |
| 启动快路径：生成清单 `_manifest_data.py` + 布局/内容指纹，未命中或过期回退全量发现；“清单只影响速度，不影响语义” | [cli/_manifest.py](file:///F:/Dev/fcmd/src/fcmd/cli/_manifest.py#L1-L20) | Rust：编译期确定工具集，启动只做一次微秒级汇总，无需回退逻辑 |
| 每工具 console script 统一入口，按 `sys.argv[0]` basename 推断工具名 | [cli/_entry.py](file:///F:/Dev/fcmd/src/fcmd/cli/_entry.py#L20-L56) | Rust：`rscmd` 二进制按第一个参数分发；可选 `argv[0]` 别名 |
| 结果信封 `fcmd-result/1`、`fcmd-error/1`、`fcmd-schema/1`；机器错误字段 `code/kind/field/expected/hint` | [core/result.py](file:///F:/Dev/fcmd/src/fcmd/core/result.py#L40-L54) | Rust：`rscmd-*/1` 同构信封 |
| 退出码统一为 0/1/2/3/4/5/130，且 `kind` 与退出码一一对应 | [core/exit_codes.py](file:///F:/Dev/fcmd/src/fcmd/core/exit_codes.py#L45-L65) | Rust：直接沿用数值与语义 |
| 自描述：`fcmd info --json` 输出参数 schema 与示例；GUI 工具在 schema 中带 `gui` 标记 | [cli/_schema.py](file:///F:/Dev/fcmd/src/fcmd/cli/_schema.py#L29-L55) | Rust：`rscmd info --json` / `rscmd tools --json`，`ToolMeta.gui` 供调用方跳过 |
| 冷启动保护：包级 `__getattr__` 懒加载 + 慢导入诊断 `FCMD_IMPORT_DEBUG` | [\_lazyload.py](file:///F:/Dev/fcmd/src/fcmd/_lazyload.py) | Rust 无需懒导入；Python 绑定保持“导入即轻” |
| 展示层薄封装 rich，markup 可关闭 | [console.py](file:///F:/Dev/fcmd/src/fcmd/console.py) | Rust：`rscmd-cli` 内 `render.rs`，`--no-color` / 非 TTY 降级 |
| 门禁分级 L0/L1/L2，覆盖率 95%，pytest 标记 `slow` 与 xdist `loadfile` | [AGENTS.md](file:///F:/Dev/fcmd/AGENTS.md)、[Makefile](file:///F:/Dev/fcmd/Makefile)、[.coveragerc](file:///F:/Dev/fcmd/.coveragerc) | Rust：`make check-docs/check-fast/flow/check` + 按 crate 过滤 |
| 目录形状用测试守卫：深度、空目录、`__init__.py`、工具名唯一、域不嵌套、测试镜像 | [tests/test_layout.py](file:///F:/Dev/fcmd/tests/test_layout.py) | Rust：`cargo xtask registry-check` + `cargo xtask arch-check` |
| 规模实测：`src/fcmd/tools` 下 88 个公开工具模块、`src/fcmd/apps` 下 35 个公开模块、`pyproject.toml` 121 个 console script 入口、240 个 `test_*.py`、23 个 GUI 工具 | 本次统计 | 说明 fcmd 的“域 + 模块”组织在数十级工具量下可维护 |

---

## 2. 目标、约束与不变量

### 2.1 目标

- **可维护**：每个工具可独立阅读、独立测试、独立回归；新增工具不触碰 CLI 主流程。
- **高性能**：原生二进制启动开销以毫秒计；Python 调用不因子进程而多付 30–60ms。
- **模块独立**：工具之间零实现依赖；公共能力只经审核过的共享层复用。
- **双交付**：同一份 Rust 实现同时产出原生 CLI 与 Python wheel，不维护两套逻辑。
- **模型友好**：稳定 JSON 契约、可自纠错误、有界输出、显式副作用声明。

### 2.2 不变量（违反即门禁红灯）

| 编号 | 不变量 |
|---|---|
| I1 | 依赖方向单向：`core ← runtime ← tool-* ← tools ← {cli, py}`，不得反向、不得成环 |
| I2 | 工具 crate 之间不得互相依赖（共享能力只能下沉到 `runtime`/`core`） |
| I3 | `core` 只依赖标准库与少量契约依赖（serde/clap/thiserror），不得依赖 `runtime`、工具、`cli`、`py` |
| I4 | `rscmd-py` 不得依赖 `rscmd-cli`（Python 不承担人类渲染/补全/诊断） |
| I5 | 工具名全局唯一；别名不得与规范名或内建命令冲突 |
| I6 | 每个工具必须声明副作用级别与 Python 暴露级别；未声明视为门禁失败 |
| I7 | 只有 `rscmd-tools` 可以做“全量工具聚合”；`cli`/`py` 只消费聚合结果 |
| I8 | 生成物（schema/工具索引）只能整体重生成，不得手改 |

---

## 3. 总体架构

```
                      ┌──────────────────────────────┐
                      │  交付适配器（薄）              │
                      │  rscmd-cli      rscmd-py     │
                      │  人读/JSON/TTY   PyO3/结构化  │
                      └───────────┬──────────────────┘
                                  │ 只依赖聚合结果
                      ┌───────────▼──────────────────┐
                      │  rscmd-tools（聚合 + feature）│
                      │  编译期确定工具集，首次汇总   │
                      └───────────┬──────────────────┘
             ┌────────────────────┼────────────────────┐
             ▼                    ▼                    ▼
      rscmd-tool-text      rscmd-tool-fs        rscmd-tool-data ...
      （一域一 crate，内部按工具平铺）
             └────────────────────┼────────────────────┘
                                  ▼
                      ┌──────────────────────────────┐
                      │  rscmd-runtime（基础件）      │
                      │  fs/text/data/net/proc/并发   │
                      └───────────┬──────────────────┘
                                  ▼
                      ┌──────────────────────────────┐
                      │  rscmd-core（契约）           │
                      │  ToolDef/注册/信封/错误/退出码 │
                      └──────────────────────────────┘
```

一次调用（CLI 或 Python）的数据流：

```
argv / JSON 参数
  → 参数解析（clap 或 serde）
  → 工具 run(&mut ToolCtx, Args) -> Result<Value, RscmdError>
  → 统一收口：成功信封 / 错误信封 / 退出码
  → 渲染（人读 or JSON；Python 直接返回 dict / 抛 RscmdError）
```

关键点：**业务逻辑只有一份**。`cli` 与 `py` 只负责“入参形态”和“结果呈现”，不复制工具实现。

---

## 4. 仓库目录结构草图（逐层职责）

```
rscmd/
├─ Cargo.toml                     # workspace 成员、统一版本、共享依赖、release profile
├─ Cargo.lock                     # 提交入库（可执行产物的可复现性）
├─ rust-toolchain.toml            # 固定 stable 版本，CI 与本地一致
├─ rustfmt.toml / clippy.toml
├─ deny.toml                      # 许可证/漏洞/重复依赖策略
├─ Makefile                       # 门禁入口（与 fcmd 同级语义：docs/fast/flow/check/bench）
├─ pyproject.toml                 # maturin 构建入口（Python 交付）
│
├─ crates/
│  ├─ rscmd-core/                 # 【契约层】不碰具体业务
│  │  ├─ src/lib.rs
│  │  ├─ src/tool.rs              # ToolDef / ToolMeta / SideEffects / PythonExposure
│  │  ├─ src/registry.rs          # 查找、别名解析、冲突检测（纯函数）
│  │  ├─ src/envelope.rs          # result/error/schema 三种信封
│  │  ├─ src/exit.rs              # ExitCode + kind ↔ code 映射
│  │  ├─ src/errors.rs            # RscmdError（code/kind/field/expected/hint）
│  │  ├─ src/ctx.rs               # ToolCtx：输出、dry-run、确认、限制、取消
│  │  └─ tests/                   # 契约黄金测试（信封、退出码、注册冲突）
│  │
│  ├─ rscmd-runtime/              # 【基础件层】纯能力，可被任意工具复用
│  │  ├─ src/fs/                  # 遍历、白名单、原子写、临时文件、路径规范化
│  │  ├─ src/text/                # 编码探测、分词、大小写/命名风格、diff
│  │  ├─ src/data/                # JSON/YAML/TOML/CSV 读写与流式转换
│  │  ├─ src/net/                 # HTTP 客户端、超时、重试、下载校验
│  │  ├─ src/proc/                # 子进程封装、超时、输出有界
│  │  ├─ src/concurrency/         # 线程池/并行度、确定性归并、取消令牌
│  │  └─ src/cache/               # 可选缓存（默认关闭，显式开启）
│  │
│  ├─ rscmd-tool-text/            # 【工具域层】文本类，一域一 crate
│  │  ├─ src/lib.rs               # pub static TOOLS: &[ToolDef]
│  │  ├─ src/case.rs              # 一个文件一个工具
│  │  ├─ src/regex.rs
│  │  └─ tests/
│  ├─ rscmd-tool-fs/              # 文件/目录类
│  ├─ rscmd-tool-data/            # 结构化数据类
│  ├─ rscmd-tool-calc/            # 计算/统计类
│  ├─ rscmd-tool-crypto/          # 哈希/编码/加解密类
│  ├─ rscmd-tool-net/             # 网络类
│  │
│  ├─ rscmd-tools/                # 【聚合层】唯一知道“有哪些工具域”的地方
│  │  ├─ Cargo.toml               # 每个域 crate 作为 optional dependency + feature
│  │  └─ src/lib.rs               # tools()：按 feature 汇总各域 TOOLS（首次访问一次）
│  │
│  ├─ rscmd-app-<应用>/           # 【可选】多入口套件/TUI；仅 rscmd-cli 依赖
│  │
│  ├─ rscmd-cli/                  # 【交付适配器 A】原生 CLI
│  │  ├─ src/main.rs              # argv[0]/argv[1] 分发，catch_unwind，退出码
│  │  ├─ src/app.rs               # 全局参数、内建命令路由
│  │  ├─ src/render.rs            # 人读表格/颜色/截断；非 TTY 降级
│  │  ├─ src/builtins/            # tools / info / schema / doctor / completion / version
│  │  └─ tests/                   # 跨工具黄金测试（信封、退出码、确定性）
│  │
│  ├─ rscmd-py/                   # 【交付适配器 B】PyO3 cdylib
│  │  └─ src/lib.rs               # invoke / run_argv / tools / describe / version
│  │
│  └─ xtask/                      # 仓库工具：registry-check、arch-check、文档/schema 生成
│
├─ python/
│  └─ rscmd/
│     ├─ __init__.py              # 重新导出原生 API，补类型标注
│     ├─ __main__.py              # console script 入口：in-process 调原生 CLI
│     ├─ _native.pyi              # 扩展模块的类型存根
│     └─ py.typed
│
├─ tests/
│  └─ py/                         # Python 绑定测试（pytest）
│
├─ benches/
│  ├─ startup.rs                  # 启动预算（criterion）
│  └─ throughput.rs               # I/O 与并发吞吐
│
├─ docs/
│  ├─ adr/                        # 长期决策（含本方案的落地状态与交接）
│  ├─ requirements/
│  ├─ analysis/
│  └─ tools.md                    # 由 xtask 生成的工具索引（勿手改）
│
└─ .github/workflows/
   ├─ ci.yml                      # lint/typecheck/test/coverage/wheel-smoke
   └─ release.yml                 # tag → 原生二进制 + wheel + sdist
```

### 4.1 每层职责边界

| 层 | 负责 | 明确不负责 |
|---|---|---|
| `rscmd-core` | 工具契约、注册表查询、信封、退出码、错误分类、运行上下文 | 具体业务、文件遍历、网络、渲染 |
| `rscmd-runtime` | 可复用的纯能力与基础操作（I/O、数据格式、并发、网络、进程） | 工具语义、CLI 参数、打印、JSON 信封 |
| `rscmd-tool-*` | 参数结构、校验、业务编排、结果构造 | 跨域复用逻辑（下沉 runtime）、人类渲染 |
| `rscmd-tools` | 聚合与 feature 选择、工具索引 | 业务实现 |
| `rscmd-cli` | 全局参数、内建命令、人读渲染、TTY/颜色/截断、退出码落地 | 业务实现、Python 绑定 |
| `rscmd-py` | 类型转换、GIL 策略、异常映射、版本暴露 | 人类渲染、补全、诊断、TUI/GUI |

---

## 5. Crate 划分与依赖方向

### 5.1 划分原则

1. **契约与能力分离**：`core` 定义“工具长什么样”，`runtime` 提供“工具用什么做”。
2. **一域一 crate**：域内工具平铺为模块（对应 fcmd 的 `tools/<域>/<工具>.py`）。
   域 crate 是依赖隔离、编译单元与测试过滤的最小边界。
3. **单工具升级为独立 crate 的条件**（满足任一）：
   - 引入重量级/平台特定的可选依赖（如 OCR、媒体编解码）；
   - 单工具实现超过约 2k 行，或需要自己的 `build.rs`；
   - 需要独立 feature 以控制二进制体积。
4. **独立应用套件**（多入口、共享状态、TUI/GUI）放 `rscmd-app-<名>`，仅 `rscmd-cli` 可依赖；
   `rscmd-py` 不暴露 GUI/TUI 能力（对应 fcmd 用 `GUI_TOOLS` 标注弹窗类工具、让调用方跳过）。

### 5.2 依赖矩阵

| crate | 允许依赖 | 禁止依赖 |
|---|---|---|
| `rscmd-core` | `std`、`serde`、`serde_json`、`clap`、`thiserror` | `runtime`、`tool-*`、`tools`、`cli`、`py` |
| `rscmd-runtime` | `core`、通用基础库（`rayon`/`walkdir`/`memchr`/`serde_*` 等） | `tool-*`、`tools`、`cli`、`py` |
| `rscmd-tool-<域>` | `core`、`runtime`、本域专用库 | 其他 `tool-*`、`tools`、`cli`、`py` |
| `rscmd-tools` | 全部 `tool-*` | `cli`、`py` |
| `rscmd-cli` | `core`、`runtime`、`tools`、`app-*`、渲染库 | `py` |
| `rscmd-py` | `core`、`runtime`、`tools`、`pyo3` | `cli`、`app-*` |
| `xtask` | 任意（仅开发期，不进产物） | 不参与运行时依赖图 |

### 5.3 依赖方向的强制机制

- **`cargo xtask arch-check`**：用 `cargo metadata` 读取实际依赖图，断言：
  - 不存在反向边与环；
  - 任一 `rscmd-tool-*` 的依赖闭包中不含其他 `rscmd-tool-*`；
  - `rscmd-py` 的依赖闭包不含 `rscmd-cli`；
- **`cargo deny check`**：许可证白名单、安全公告、同一 crate 多版本告警。
- **`cargo machete`**：未使用的依赖（防止“随手加依赖”）。
- **`cargo xtask registry-check`**：工具名/别名唯一、feature 与域目录一致、
  每个工具都登记了 `ToolMeta` 且有按命名约定的测试文件、生成物是最新的。

### 5.4 workspace 骨架示例

```toml
[workspace]
resolver = "2"
members = ["crates/*"]

[workspace.package]
version = "0.1.0"          # 唯一版本来源
edition = "2021"
rust-version = "1.78"
license = "MIT"

[workspace.dependencies]
rscmd-core    = { path = "crates/rscmd-core" }
rscmd-runtime = { path = "crates/rscmd-runtime" }
clap    = { version = "4", features = ["derive"] }
serde   = { version = "1", features = ["derive"] }
serde_json = "1"

[profile.release]
lto = "thin"
codegen-units = 1
strip = "symbols"
panic = "unwind"            # 保留 unwind，供调度边界 catch_unwind 兜底
```

```toml
# crates/rscmd-core/Cargo.toml
[package]
name = "rscmd-core"
version.workspace = true
edition.workspace = true
```

---

## 6. 工具模型：契约、注册、发现

### 6.1 工具契约

每个工具由三部分组成，全部是编译期常量或函数指针，便于静态注册与内省：

```rust
// rscmd-core/src/tool.rs
pub struct ToolMeta {
    pub name: &'static str,              // 规范名，全局唯一（如 "case"）
    pub aliases: &'static [&'static str],
    pub summary: &'static str,           // ≤ 200 字符，给列表与模型看
    pub side_effects: SideEffects,       // None/Read/Write/Network/Destructive
    pub gui: bool,                       // 模型调用过滤用
    pub python: PythonExposure,          // Hidden/ArgvOnly/Structured
    pub examples: &'static [&'static str],
}

#[derive(Clone, Copy)]
pub struct ToolDef {
    pub meta: &'static ToolMeta,
    pub build: fn() -> clap::Command,
    pub run: fn(&mut ToolCtx, &clap::ArgMatches) -> Result<serde_json::Value, RscmdError>,
}

// 类型化包装：参数结构同时支持 CLI 与 JSON 两种入参
pub fn make_def<A>(
    meta: &'static ToolMeta,
    run: fn(&mut ToolCtx, A) -> Result<serde_json::Value, RscmdError>,
) -> ToolDef
where
    A: clap::Args + serde::de::DeserializeOwned + 'static,
{ /* 用 clap 解析或 serde 反序列化后统一调用 run */ }
```

工具入口必须是**无状态、可重入**的：不捕获全局可变状态；并行度、dry-run、输出上限、
取消令牌等运行期依赖全部从 `ToolCtx` 读取。这样同一工具可以被 CLI 串行调用、
被 Python 多线程调用、被未来批量执行器并行调用，而无需额外加锁。

工具实现（示例，仅示意结构）：

```rust
// rscmd-tool-text/src/case.rs
pub static META: ToolMeta = ToolMeta {
    name: "case",
    aliases: &["casetool"],
    summary: "命名风格转换（snake/camel/pascal/kebab）",
    side_effects: SideEffects::None,
    gui: false,
    python: PythonExposure::Structured,
    examples: &["rscmd case --mode snake HelloWorld"],
};

#[derive(clap::Args, serde::Deserialize)]
pub struct CaseArgs {
    /// 待转换的字符串
    pub text: String,
    /// 转换目标，非法值在 run 内统一返回 INVALID_MODE（退出码 3）
    #[arg(long, default_value = "snake")]
    pub mode: String,
}

pub fn def() -> ToolDef {
    make_def(&META, |ctx, args: CaseArgs| {
        let out = rscmd_runtime::text::convert_case(&args.text, args.mode);
        Ok(serde_json::json!({ "text": out }))
    })
}
```

> 落地说明：`def()` 无法写成 `const`（包含 `clap::Command` 构造与函数指针），
> 且泛型闭包不能直接存进 `ToolDef` 的函数指针字段。实际实现为每个工具提供
> `run_cli`（`FromArgMatches`）与 `run_json`（`serde_json::from_value`）两个薄入口，
> 再在 `def()` 中组装；两条入口最终调用同一个纯函数 `run`，业务逻辑仍只有一份。

### 6.2 注册：编译期确定的静态表

```rust
// crates/rscmd-tool-text/src/lib.rs —— 域内工具总表（新增工具只改这里）
pub mod case;
pub mod regex;

pub fn tools() -> &'static [ToolDef] {
    static TOOLS: OnceLock<Vec<ToolDef>> = OnceLock::new();
    TOOLS.get_or_init(|| vec![case::def(), regex::def()])
}
```

> `def()` 不是 `const`，所以域内用 `OnceLock<Vec<ToolDef>>` 做“首次访问一次性组装”，
> 语义仍是编译期确定、运行期只读；汇总层同理。

```rust
// crates/rscmd-tools/src/lib.rs —— 全仓唯一“域汇总”，不含具体工具名
use std::sync::OnceLock;

pub fn tools() -> &'static [ToolDef] {
    static ALL: OnceLock<Vec<ToolDef>> = OnceLock::new();
    ALL.get_or_init(|| {
        let mut v: Vec<ToolDef> = Vec::new();
        #[cfg(feature = "text")]
        v.extend_from_slice(rscmd_tool_text::tools());
        #[cfg(feature = "fs")]
        v.extend_from_slice(rscmd_tool_fs::tools());
        #[cfg(feature = "data")]
        v.extend_from_slice(rscmd_tool_data::tools());
        // ... 其余域
        v
    })
}
```

```toml
# crates/rscmd-tools/Cargo.toml
[features]
default = ["text", "fs", "data", "calc", "crypto", "net"]
slim = ["text", "data"]
text = ["dep:rscmd-tool-text"]
fs   = ["dep:rscmd-tool-fs"]
all-tools = ["text", "fs", "data", "calc", "crypto", "net"]

[dependencies]
rscmd-tool-text = { path = "../rscmd-tool-text", optional = true }
```

`TOOLS` 汇总发生在首次访问时（数十项 `ToolDef` 拷贝，微秒级），之后只读；
仍然没有目录扫描、没有模块导入、没有动态库加载。新增“已有域内的工具”**不需要改聚合层**，
新增“域”才需要加一行 feature 与一行 `extend_from_slice`。

`resolve(name)` 由 `rscmd-tools` 基于 `core` 的注册表类型实现（`Registry::new(tools())`），
本质是对数十项的线性查找，一次调用只做一次；不引入哈希表初始化成本。

### 6.3 “发现”机制与内建命令

- **运行时没有发现**：没有 `scan`、没有 import、没有动态库加载。工具是否存在由编译期依赖决定。
- **对外发现**通过内建命令暴露：
  - `rscmd tools` / `rscmd tools --json`：工具摘要（名、别名、摘要、副作用、是否 GUI）；
  - `rscmd tools --for-llm`：过滤掉 `gui = true` 的工具，供模型编排直接消费；
  - `rscmd info <tool> [--json]`：单工具完整 schema（参数、默认值、示例）；
  - `rscmd schema [--json]`：全量 schema（供文档生成与模型上下文装载）；
  - `rscmd completion <shell>`：补全脚本；
  - `rscmd doctor`：安装完整性、版本、平台能力自检。
- **可选 argv[0] 别名**：同一二进制被硬链接/复制为 `rscmd-case` 时，按 basename 分发
  （对应 fcmd `_entry.run_named`）。默认安装只提供 `rscmd`，不生成符号链接农场。

### 6.4 为什么不做运行时动态插件

| 方案 | 启动 | 隔离 | 发布复杂度 | 结论 |
|---|---|---|---|---|
| 编译期静态表 | 零扫描（首次汇总一次，微秒级） | 编译期保证 | 低 | **采用** |
| `linkme`/`inventory` 分布式注册 | 近零（有链接器/ctor 差异） | 中 | 中 | 不采用（跨平台链接行为需额外验证，收益仅是少一行数组） |
| 运行时 `dlopen` 插件 | 有扫描/加载成本 | 弱（ABI 脆弱） | 高（每插件单独发布） | 不采用 |
| 子进程 JSON-RPC 插件 | 每调用数十 ms | 强 | 中 | 留作后续 ADR，不进 P0 |

### 6.5 统一输出与退出码

退出码与 fcmd 完全一致（数值与语义都不改）：

| 码 | 名称 | 触发场景 |
|---|---|---|
| 0 | `SUCCESS` | 执行成功，结果已给出 |
| 1 | `FAILURE` | 运行期失败（I/O、子进程、内部异常、panic 兜底） |
| 2 | `USAGE` | 调用方用错命令（未知工具/子命令、缺参数、未知选项） |
| 3 | `INVALID_INPUT` | 参数值不合法（枚举/格式/范围/互斥冲突） |
| 4 | `NOT_FOUND` | 目标不存在（文件/目录/记录） |
| 5 | `CHECK_FAILED` | 检查类工具结论为“不通过”（不是错误） |
| 130 | `INTERRUPTED` | 用户中断（Ctrl+C） |

`--json` 时 stdout 只放结果信封，stderr 只放错误信封：

```json
{"schema":"rscmd-result/1","ok":true,"tool":"case","data":{"text":"hello_world"}}
```

```json
{"schema":"rscmd-error/1","ok":false,"tool":"fs_move","kind":"not_found",
 "code":"FILE_NOT_FOUND","message":"源文件不存在","field":"src",
 "expected":"已存在的文件","hint":"确认路径后重试","exit_code":4}
```

默认（无 `--json`）输出人读文本；两种模式共享同一份结果对象与同一套退出码，
不得出现“人读成功、机读失败”或反之。

---

## 7. 共享代码的复用方式与边界（避免大泥球）

### 7.1 准入规则（`runtime` 收录标准）

一个能力进入 `rscmd-runtime` 必须同时满足：

1. 至少被 **3 个工具**使用，或承载**跨工具不变量**（如“写文件必须原子”）。
2. 不包含任何具体工具的领域概念（如“重命名规则”“扫描规则”）；
3. 不打印、不读环境变量做全局副作用（配置显式传入）；
4. 有独立单测与文档注释，API 变更按 semver 对待；
5. 不依赖 `clap`、不构造 JSON 信封、不决定退出码。

### 7.2 反泥球规则

- **禁止 `utils` 桶**：按主题分模块（`fs` / `text` / `data` / `net` / `concurrency`），
  每个模块有明确负责人式文档与用例。
- **两次重复不抽象**：同一段逻辑第二次出现时保留，第三次出现才评审下沉；
  下沉时以“被调用方”的真实需求定 API，而不是把两份代码拼在一起。
- **共享层不得反向长出业务分支**：`runtime` 里出现 `if tool == "..."` 一律拒绝。
- **工具之间零引用**：需要复用某工具能力时，先判断它是“基础能力”（下沉 `runtime`）
  还是“业务语义”（复制并各自演进，或抽出新的基础原语）。
- **feature 不外溢**：域 crate 的 feature 只控制自身可选依赖，不改变其他域的行为。

### 7.3 复用形态

| 形态 | 示例 | 放置 |
|---|---|---|
| 纯函数 | 命名风格转换、哈希、单位换算 | `runtime::text` / `runtime::crypto` |
| 基础操作对象 | 原子写、临时目录、下载器、进程封装 | `runtime::fs` / `runtime::net` / `runtime::proc` |
| 并发原语 | 有界并行 map、确定性归并、取消令牌 | `runtime::concurrency` |
| 契约类型 | 信封、错误、退出码、工具元数据 | `core` |
| 测试辅助 | 临时目录、金样、断言宏 | `core::testing`（`#[cfg(feature = "testing")]`） |

---

## 8. Python 发布形态（Rust 主体 + Python 薄绑定）

### 8.1 产物形态

| 产物 | 内容 | 分发渠道 |
|---|---|---|
| 原生二进制 | `rscmd`（单文件，全工具静态链接） | GitHub Releases（tar.gz/zip + sha256） |
| Python wheel | `rscmd/_native`（PyO3 扩展）+ 纯 Python 薄壳 | PyPI |
| sdist（可选） | Rust 源码 + Python 源码，安装时编译 | PyPI（兜底，不承诺无 Rust 环境可用） |

**wheel 不内置原生二进制**：避免同一能力两份产物、体积翻倍、平台脚本路径差异。
Python 侧走进程内扩展；需要独立二进制的高级用户从 Releases 安装。

> 命名冲突：wheel 的 console script 与原生二进制都叫 `rscmd`。二者不要求共存于同一 PATH；
> 只装 wheel 的用户直接用 console script，只用原生的用户装 Releases。文档需明确 PATH 优先级，
> 并在 `rscmd doctor` 中输出“当前入口来自 Python 还是原生二进制”。

### 8.2 目录与构建

```toml
# pyproject.toml（仓库根）
[build-system]
requires = ["maturin>=1.7,<2"]
build-backend = "maturin"

[project]
name = "rscmd"
dynamic = ["version"]              # 从 Cargo.toml 读取，不维护第二份版本
requires-python = ">=3.10"
license = { text = "MIT" }
classifiers = ["Programming Language :: Rust", "Programming Language :: Python :: 3"]

[project.scripts]
rscmd = "rscmd.__main__:main"      # in-process 调原生 CLI，不起子进程

[tool.maturin]
manifest-path = "crates/rscmd-py/Cargo.toml"
python-source = "python"
module-name = "rscmd._native"
bindings = "pyo3"
features = ["python", "all-tools"]
strip = true
```

```toml
# crates/rscmd-py/Cargo.toml
[lib]
name = "_native"
crate-type = ["cdylib"]

[features]
default = ["python"]
python = []                                  # 标记位，供 maturin --features 选择
all-tools = ["rscmd-tools/all-tools"]

[dependencies]
pyo3 = { version = "0.2x", features = ["abi3-py310", "extension-module"] }
rscmd-tools = { path = "../rscmd-tools" }
```

`abi3-py310` 使同一 wheel 覆盖 Python 3.10+，把 wheel 矩阵从“每 Python 版本一份”
收敛为“每平台/架构一份”。

### 8.3 Python API 暴露面

```python
import rscmd

rscmd.__version__                     # 原生 env!("CARGO_PKG_VERSION")
rscmd.tools()                         # 摘要列表（dict）
rscmd.describe("case")                # rscmd-schema/1 的单工具文档
rscmd.invoke("case", {"text": "HelloWorld", "mode": "snake"})
rscmd.run_argv(["case", "snake", "HelloWorld"])
```

`invoke` / `run_argv` 成功时返回 `rscmd-result/1` 的 `data`（或完整信封，按参数选择），
失败时抛 `RscmdError`，不返回“退出码 0 但内容为空”的含糊结果。
`RscmdError` 由 `pyo3::create_exception!` 在原生侧创建，字段直接来自同一个 Rust 错误结构，
避免 Python 侧再维护一套错误分类。

| 能力 | 是否暴露给 Python | 理由 |
|---|---|---|
| 数据进/数据出的工具（文本、数据格式、哈希、计算、路径、只读扫描） | 是（`Structured`） | 无交互、易序列化、对模型/脚本价值最高 |
| 元数据：`tools/describe/schema` | 是 | 让 Python 侧编排器动态发现参数 |
| 写操作类工具 | 是，但默认 dry-run，需显式 `confirm=True` 才落盘 | 默认安全，避免静默破坏 |
| 长耗时/大 I/O 工具 | 是，调用期间释放 GIL | 不阻塞宿主线程 |
| 人类渲染（表格/颜色/进度条） | 否 | 属于 CLI 呈现，Python 应拿结构化数据 |
| 补全脚本、`doctor`、自更新 | 否 | 终端/安装期能力 |
| TUI/GUI 应用 | 否 | 无头环境不可用，模型调用会阻塞（对应 fcmd `GUI_TOOLS` 的标注） |
| 交互式确认（y/n 提示） | 否 | 统一改为返回值 + 错误码 `CONFIRM_REQUIRED` |

Python 侧错误映射：

```python
try:
    rscmd.invoke("fs_move", {"src": "a", "dst": "b", "dry_run": False, "confirm": True})
except rscmd.RscmdError as e:
    e.code        # "FILE_NOT_FOUND"
    e.kind        # "not_found"
    e.exit_code   # 4
    e.field       # "src"
    e.expected    # "已存在的文件"
    e.hint        # "确认路径存在"
```

### 8.4 版本与发布节奏同步

- **唯一版本源**：根 `Cargo.toml` 的 `[workspace.package].version`。
- Rust 各 crate 用 `version.workspace = true`；maturin 用 `dynamic = ["version"]` 读取同一值；
  Python 通过 `#[pyfunction] fn version()` 返回编译期版本。
- **发布判定（缺一不可）**：
  1. git tag（`vX.Y.Z`）去掉 `v` 等于 workspace 版本；
  2. wheel 元数据版本等于同一值；
  3. `python -c "import rscmd; assert rscmd.__version__ == '<X.Y.Z>'"` 通过。
  CI 在发布前用一个脚本同时校验三者，任一不一致即中止。
- **节奏**：原生二进制与 wheel 由**同一个 tag、同一 CI 运行**产出，不设独立版本线。
  即使只修 Python 绑定，也必须走一次完整版本号递增——保证“同一版本号 = 同一提交 = 同一行为”。
- **预发布映射**：Cargo 的 `X.Y.Z-alpha.N` 在 wheel 中必须映射为 PEP 440 的 `X.Y.ZaN`
  （以及 `-beta.N` → `bN`、`-rc.N` → `rcN`），由发布脚本转换并断言，禁止手工填两套。
- **变更记录**：`CHANGELOG.md` 按“Rust API / CLI 契约 / Python API / 性能”分段，
  一次发布一节，和 tag 一一对应。

### 8.5 wheel 矩阵与构建流程

| 平台 | 目标 | 备注 |
|---|---|---|
| Linux x86_64 | `x86_64-unknown-linux-gnu` | manylinux 兼容，maturin 自动处理 |
| Linux aarch64 | `aarch64-unknown-linux-gnu` | 交叉构建或原生 runner |
| Windows x64 | `x86_64-pc-windows-msvc` | 主开发平台 |
| macOS x86_64 + arm64 | `x86_64-apple-darwin` / `aarch64-apple-darwin` | 可选 `universal2` |

流程（tag 触发）：

```
本地/CI 构建：
  cargo build --release -p rscmd-cli
  maturin build --release --features python,all-tools

发布：
cargo test/clippy/fmt（全 workspace）
  → maturin build --release --features python,all-tools（各目标）
  → 每个 wheel 安装到干净 venv，跑 smoke：import、version、invoke 一个工具
  → cargo build --release（各原生目标）→ 打包 + sha256
  → 版本三方一致校验
  → 上传 PyPI（OIDC）+ GitHub Release
```

产物命名固定为：

```
rscmd-vX.Y.Z-x86_64-pc-windows-msvc.zip
rscmd-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz
rscmd-vX.Y.Z-aarch64-apple-darwin.tar.gz
rscmd-vX.Y.Z-<target>.sha256
rscmd-X.Y.Z-cp310-abi3-<platform>.whl
```

同一次运行内先产出全部产物并完成校验，再统一上传；任一目标失败则整次发布中止，
不允许“部分平台已发布、部分平台缺失”。

---

## 9. 依赖与打包策略

### 9.1 Rust 依赖策略

- 优先标准库；新增依赖需在 PR 说明“为什么 std 不够”，并过 `cargo deny`/`cargo machete`。
- 短生命周期 CLI **不引入 async 运行时**；并行用 `rayon`，需要事件循环的工具单独评估。
- 解析/序列化统一 `serde` 生态；JSON 输出统一 `serde_json`（保持键序稳定）。
- 错误：`core` 用 `thiserror` 定义错误分类；业务错误不滥造错误类型，统一映射到 `RscmdError`。
- 平台相关依赖用 `cfg(target_os)` 隔离，并保证 Linux CI 也能编译（防止只在本机通过）。

### 9.2 feature 与产物

| 配置 | 用途 | 内容 |
|---|---|---|
| `default` | 常规安装 | 全部稳定工具 |
| `slim` | 体积敏感场景 | 仅 `text` + `data` |
| `all-tools` | Python wheel / 全量发布 | 全部稳定工具 |
| `python` | 扩展模块 | 开启 PyO3 绑定 |
| `testing` | 测试辅助 | 金样断言、临时目录工具 |

注意：Rust 编译产物的工具选择是**构建期**的，不能像 fcmd 的 Python extras 那样
“缺依赖就跳过某个工具”。因此 Python 侧不提供 per-tool extras，统一一个 wheel；
体积通过 `strip`、`lto`、`opt-level` 与可选 `slim` 构建控制。

### 9.3 生成物

| 生成物 | 生成命令 | 校验 |
|---|---|---|
| `docs/tools.md` | `cargo xtask gen-docs` | CI 重生成后 `git diff --exit-code` |
| `schema/*.json`（可选，P1） | `cargo xtask gen-schema` | 同上 + 黄金测试 |
| `python/rscmd/_native.pyi` | `cargo xtask gen-stub`（P1） | 同上 |

生成物只能整体重生成、不得手改（同 fcmd `_manifest_data.py` 的纪律）。

---

## 10. 性能设计取舍

### 10.1 启动开销

| 设计选择 | 影响 | 取舍 |
|---|---|---|
| 编译期静态注册表 | 无扫描、无 import、无反射；首次访问汇总一次 | 新增工具需改数组（可被 xtask 校验） |
| 单二进制多调用 | 一次链接、一个安装项 | 不用每工具 exe（省安装项与体积） |
| 不在 `main` 前做全局初始化 | 无 ctor/无配置加载 | 配置在 `ToolCtx` 构造时按需读 |
| 懒加载重资源（正则编译、HTTP 客户端、大表） | 只有用到的工具付成本 | 工具内 `OnceLock` 缓存 |
| 不做运行时插件 | 无 `dlopen`/目录扫描 | 放弃第三方扩展性（见 6.4） |

**验收目标（P0 实测后写死，不用未测数字对外宣称）**：

| 场景 | 目标 | 测量方法 |
|---|---|---|
| `rscmd --version`（Windows 冷启动） | ≤ 10 ms | `hyperfine --warmup 3 'rscmd --version'` |
| `rscmd case --mode snake HelloWorld` | ≤ 20 ms | 同上 |
| `rscmd tools --json` | ≤ 25 ms | 同上 |
| `import rscmd`（Python） | ≤ 60 ms | `python -X importtime -c "import rscmd"` |
| Python 单次 `invoke` 额外开销 | ≤ 5 ms | 与直接 Rust 调用对比 |

**P0 实测（Windows 11 x64，release，`strip=symbols`，20 次取中位数）**：

| 场景 | 实测 | 备注 |
|---|---|---|
| `rscmd --version` | 6.8 ms（min 6.4） | PowerShell `Stopwatch`，含进程创建 |
| `rscmd case --mode snake HelloWorld` | 6.8 ms（min 6.5） | 同上 |
| `rscmd tools --json` | 6.9 ms（min 6.5） | 3 个工具、2 个域 |
| `import rscmd` | 15.3 ms | 其中 `rscmd._native` 仅 0.9 ms |
| Python 单次 `invoke` | 1.03 µs | 2000 次循环均值，进程内调用 |
| `rscmd.exe` 体积 | 821 KB | 未含 Python wheel 内的 cdylib |

结论：启动开销主要来自进程创建而非框架本身；Python 侧没有子进程成本。测量方法与原始数字记录在本节，后续用 `benches/startup.rs`（P2）守护回归。

### 10.2 并发与 I/O

- **默认单线程**：小输入下并发只会增加调度与内存开销。
- **显式并行**：需要遍历大量文件/条目的工具提供 `--workers`（`0` = 自动，
  上限 `min(cores, 8)`），由 `runtime::concurrency` 统一实现。
- **确定性**：并行结果必须按输入序或显式排序键归并，保证同一输入输出逐字节一致
  （模型与测试都依赖这一点）。
- **流式处理**：大文件按块读写；需要全量的场景用 `mmap`（哈希、只读扫描）。
- **白名单**：文件扫描类工具默认按扩展名白名单跳过非文本文件，并在结果中区分
  `skipped_by_extension` / `skipped_by_size` / `skipped_by_rule`（fcmd filescan 的经验）。
- **有界输出**：所有可能产生大结果的工具支持 `--limit` / `--max-bytes`，
  JSON 中带 `truncated: true`，避免把模型上下文打爆。

### 10.3 二进制体积

- `lto = "thin"`、`codegen-units = 1`、`strip = "symbols"`。
- 重依赖工具（OCR/媒体）默认不进 `slim`；全量构建单独测量体积并记录在发布说明。
- `panic = "unwind"` 而非 `abort`：调度层用 `catch_unwind` 把工具 panic 转成
  `kind = "runtime"`（退出码 1），避免单个工具崩溃拖垮批量调用；代价是少量体积。

---

## 11. 测试与门禁

### 11.1 分层测试

| 层 | 测试类型 | 位置 | 命令 |
|---|---|---|---|
| `core` | 契约单测 + 黄金信封 | `crates/rscmd-core/tests/` | `cargo nextest run -p rscmd-core` |
| `runtime` | 纯函数/边界/并发确定性 | 各模块 `#[cfg(test)]` + `tests/` | `cargo nextest run -p rscmd-runtime` |
| 工具域 | 参数校验、业务结果、JSON 输出 | `crates/rscmd-tool-*/tests/` | `cargo nextest run -p rscmd-tool-text` |
| `cli` | 跨工具黄金测试（stdout/stderr/退出码） | `crates/rscmd-cli/tests/` | `cargo nextest run -p rscmd-cli` |
| `py` | 绑定 API、异常字段、GIL、版本 | `tests/py/` | `pytest tests/py -k <tool>` |
| 架构 | 依赖方向、命名唯一、副作用声明 | `crates/xtask/` | `cargo xtask arch-check` |

### 11.2 门禁分级（沿用 fcmd 的 L0/L1/L2 语义）

| 级别 | 触发范围 | 必跑 |
|---|---|---|
| L0 文档 | 仅 `*.md`、`docs/` | `make check-docs`（生成物未过期；P0 尚无 ADR 目录，编号校验随 ADR 建立再加） |
| L1 配置 | `Cargo.toml`、`pyproject.toml`、workflow、`Makefile` | `make check-fast`（fmt + clippy + registry-check + arch-check + deny 快速项） |
| L2 代码 | `crates/**`、`python/**`、`tests/**` | `make flow`（check-fast → nextest）；收尾跑 `make check` |

`make check` 全量包含：

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check licenses bans sources   # advisories 需本地 RustSec 数据库，缺失时打印跳过原因
cargo machete
cargo xtask registry-check
cargo xtask arch-check
cargo xtask docs-check
cargo nextest run --workspace            # 未安装 nextest 时回退 cargo test 并打印提示
python -m pytest tests/py -q             # 仅在存在 .venv 时执行
```

覆盖率与 wheel 冒烟因耗时较长，作为独立门禁执行（都可在本仓库直接跑通）：

```
make cov          # cargo llvm-cov --workspace --ignore-filename-regex 'rscmd-py|xtask' --fail-under-lines 95
make wheel-smoke  # maturin build --release → 安装进 .venv → import + 成功/失败两种调用
```

> 覆盖率口径：`rscmd-py`（被 pytest 覆盖，Rust 侧不可见）与 `xtask`（仓库工具，靠自身与门禁输出校验）
> 显式排除，其余业务代码 95% 行覆盖硬性要求。P0 实测 `crates/` 行覆盖率 98.38%。
> 风险表中的“95% 难达标”预案即按此落地：排除薄胶水，不放水业务代码。

### 11.3 按工具独立执行

```bash
# 只跑文本域
cargo nextest run -p rscmd-tool-text
# 只跑某个工具（测试名约定：test_<tool>_*）
cargo nextest run -p rscmd-tool-text -E 'test(test_case_)'
# 只算该 crate 的覆盖率
cargo llvm-cov -p rscmd-tool-text --fail-under-lines 95
# Python 侧只跑对应工具
pytest tests/py -k case
# 单工具启动/吞吐基准
cargo bench --bench startup -- case
```

Makefile 提供薄封装：

```make
test-tool:
	cargo test -p rscmd-tool-$(TOOL)

cov-tool:
	cargo llvm-cov -p rscmd-tool-$(TOOL) --fail-under-lines 95

py-test-tool:
	python -m pytest tests/py -q -k $(TOOL)
```

> 工具链未安装时：`check-fast` 会显式打印跳过原因（`cargo deny` / `cargo machete`），
> `check` 在无 `cargo-nextest` 时回退到 `cargo test`；`cov` 缺 `cargo-llvm-cov` 时直接失败并给出安装命令。
> 任何跳过都会打印在输出里，不静默通过。

### 11.4 性能门禁

- `benches/startup.rs`：对 `--version`、`tools --json`、典型工具做基准，
  与仓库内基线文件比较；低于基线一定比例即失败（参考 fcmd `bench_filescan.py --check`）。
- 基准不进主测试门禁（避免 CI 抖动），由 `make bench` / 定时任务跑。
- 性能改动必须给出同机、同口径的前后对比（启动、吞吐、内存峰值），并注明测量命令。

### 11.5 CI 与发布门禁

- `ci.yml`：Linux + Windows + macOS 矩阵；Rust 全量门禁 + Python 测试 + wheel 构建 smoke。
- 路径过滤：`crates/**`、`python/**`、`Cargo.toml`、`Cargo.lock`、`pyproject.toml`、
  `tests/**`、workflow 自身。
- `release.yml`：仅 tag 触发；构建原生二进制与 wheel；版本三方一致；人工审批后上传。
- 禁止 `--no-verify`；提交信息用 conventional commits（`feat/fix/refactor/test/docs/chore`）。

---

## 12. 新增一个工具的完整步骤（示例：`case` 文本工具）

### 12.1 已有域内新增（最常见）

1. **建文件**：`crates/rscmd-tool-text/src/case.rs`。
2. **写契约**：`ToolMeta`（名、别名、摘要、副作用、Python 暴露级别、示例）。
3. **写参数**：`CaseArgs` 同时派生 `clap::Args` 与 `serde::Deserialize`。
4. **写逻辑**：`run(&mut ToolCtx, CaseArgs)`，纯逻辑优先调用 `rscmd_runtime::text`。
5. **挂载**：在 `crates/rscmd-tool-text/src/lib.rs` 增加 `pub mod case;` 与
   `tools()` 中的一行 `case::def()`。
6. **测试**：
   - 单测：命名风格转换的边界（空串、Unicode、混合大小写）；
   - CLI 黄金测试：加在 `crates/rscmd-cli/tests/`，覆盖 `rscmd case --mode snake HelloWorld`
     的 stdout + 退出码 0；
   - JSON 测试：`--json` 信封字段与 `data.text`；
   - 错误路径：非法 `--mode` → 退出码 3 + `INVALID_INPUT`（如 `INVALID_MODE`）。
7. **声明 Python 暴露**：`PythonExposure::Structured` 时补 `tests/py/test_case.py`。
8. **门禁**：
   ```bash
   make check-fast
   cargo nextest run -p rscmd-tool-text
   cargo llvm-cov -p rscmd-tool-text --fail-under-lines 95
   maturin develop --release && pytest tests/py -k case   # 若暴露给 Python
   make flow
   ```
9. **生成物与文档**：`cargo xtask gen-docs`（工具索引）、更新 `CHANGELOG.md`。

> 关键点：**不需要改 `rscmd-cli`、不需要改 `rscmd-py`、不需要改聚合 crate**。
> CLI 自动通过聚合表看到新工具；Python 自动通过同一张表暴露 `invoke`。

### 12.2 新域新增

在 12.1 基础上增加：

1. `crates/rscmd-tool-<域>/`（`Cargo.toml` + `src/lib.rs` + `tests/`）。
2. 在 `crates/rscmd-tools/Cargo.toml` 增加 optional dependency 与 feature。
3. 在 `crates/rscmd-tools/src/lib.rs` 增加 `#[cfg(feature = "…")]` 聚合行。
4. 跑 `cargo xtask registry-check`（会校验 feature/目录/注册表一致）。
5. 同步 `docs/tools.md` 与发布说明。

### 12.3 单工具升级为独立 crate

满足 5.1 的条件时，把 `src/<tool>.rs` 拆为 `crates/rscmd-tool-<tool>/`，
聚合层改为依赖新 crate；其余步骤不变。测试与门禁命令的 `-p` 目标随之改变。

---

## 13. 分期落地建议

| 阶段 | 内容 | 验收 |
|---|---|---|
| P0 骨架 | workspace、`core` 契约、`runtime` 最小骨架、3–5 个文本/数据工具、`cli`、`xtask registry-check`/`arch-check`、L0/L1/L2 门禁 | `make check` 全绿；启动实测数字入库 |
| P1 双交付 | `runtime` 能力沉淀、Python 扩展、`maturin develop`、wheel 构建 smoke、`tools/info --json` | wheel 可安装并调用；版本三方一致 |
| P2 强化 | schema 生成（schemars）、基准与防回归、并发工具、流式 API、发布流水线 | tag 触发的 release 成功产出二进制 + wheel |

每阶段独立可回滚；P1 的 Python 绑定必须建立在 P0 已稳定的 CLI/JSON 契约之上。

### 13.1 P0 实施记录（2026-11-15）

**已落地**

- workspace：8 个 crate（`core`/`runtime`/`tool-text`/`tool-data`/`tools`/`cli`/`py`/`xtask`）+ `python/rscmd` 包 + `tests/py`；
  统一版本 `0.1.0`，`publish = false`，`clap 4` / `serde 1` / `pyo3 0.24 (abi3-py310)`。
- core：`ToolDef/ToolMeta`、`Registry`（查找/别名/冲突校验）、`dispatch`（内建 5 命令 + 全局参数 + 退出码）、
  `render`（人读/JSON 双通道、quiet）、统一 `RscmdError` 与 `rscmd-*/1` 信封。
- 工具：`case`、`wc`（文本域）、`json_fmt`（数据域），均同时提供 CLI 与 JSON 入口。
- 门禁：`registry-check`、`arch-check`、`docs-check`、`gen-docs`、`check-fast`、`check`、`check-cov`、`wheel-smoke`；
  Makefile 薄封装与 `cargo xtask` 别名可用。
- Python：maturin 构建 `rscmd-0.1.0-cp310-abi3-win_amd64.whl`，`invoke` / `run_argv` / `tools` / `describe` / `__version__`，
  `python -m rscmd` 与 console script 均走同一份 dispatch。

**门禁实测（本机，Windows 11 / Rust 1.96 / Python 3.13）**

| 项 | 结果 |
|---|---|
| `cargo check --workspace --all-targets` | 通过 |
| `cargo nextest run --workspace` | 60 个测试通过 |
| `cargo xtask registry-check` | 3 个工具、2 个域，通过 |
| `cargo xtask arch-check` | 依赖方向通过 |
| `cargo xtask docs-check` | `docs/tools.md` 为最新生成物 |
| `make check` | fmt / clippy(-D warnings) / deny(licenses,bans,sources) / machete / 三项结构检查 / nextest / pytest 全通过 |
| `make cov` | 行覆盖率 98.38%（排除 `rscmd-py`、`xtask`） |
| `make wheel-smoke` | wheel 安装 + 成功/失败调用冒烟通过 |
| `pytest tests/py` | 7 passed |

**与本文档的偏差（均有实测依据）**

1. **Python 绑定提前到 P0**：按需求方要求，P0 需验证“Python 能安装并调用同一能力”，因此只做了最小暴露面
   （`invoke/run_argv/tools/describe/version`）；stub 生成、sdist、发布流水线仍在 P1。
2. **域内工具表用 `OnceLock<Vec<ToolDef>>` 而非 `static TOOLS: &[ToolDef]`**：`def()` 含 `clap::Command` 构造，
   不是 `const`；语义不变（编译期确定、首次访问一次性汇总）。
3. **`case` 的 `mode` 使用字符串 + 手工校验**：`clap::ValueEnum` 的非法值会走 clap 错误（退出码 2），
   与退出码表承诺的 `INVALID_MODE`/退出码 3 不一致；改为在 `run` 内校验，CLI 与 JSON 入口错误一致。
4. **全局参数 id 加 `_rscmd_` 前缀**：`json_fmt` 的位置参数名为 `json`，与全局 `--json` 在 clap 中 id 冲突会 panic；
   前缀化后两者共存。
5. **`cargo deny check` 在离线环境只跑 licenses/bans/sources**：advisories 需要从 GitHub 拉取 RustSec 数据库，
   本机网络不可达；有本地数据库时自动补跑并打印，缺失时显式跳过而不是静默通过。
6. **覆盖率排除 `rscmd-py` 与 `xtask`**：前者由 pytest 覆盖、Rust 插桩不可见；后者是仓库工具。
   排除后 98.37%，业务代码未放宽阈值。

**未做（按分期留待 P1/P2）**：`runtime` 的 fs/net/proc/并发模块、schemars schema 生成、stub 生成、
sdist、CI/release workflow、基准回归、`argv[0]` 别名安装、更多工具域。


---

## 14. 与 fcmd 的对照

| fcmd | rscmd | 说明 |
|---|---|---|
| `core` + `infras` 分层 | `core` + `runtime` | 同构；Rust 用 crate 边界强制 |
| `tools/<域>/<工具>.py` | `rscmd-tool-<域>/src/<工具>.rs` | 同构，模块即工具 |
| `apps/<应用>/` | `rscmd-app-<应用>/` | 仅 CLI 依赖，Python 不暴露 |
| `@fx.tool` 装饰器注册 | `ToolDef` 静态表 | 编译期替代运行期注册 |
| `_discovery.py` 扫描 | 无扫描，静态表 | 启动更快，新增工具改动更少 |
| `_manifest_data.py` + 指纹回退 | 编译期表 + `xtask registry-check` | 不再需要“清单过期回退”逻辑 |
| `_entry.run_named` argv[0] 推断 | `main` 支持 argv[0] 别名 | 能力保留，默认不铺符号链接 |
| `fcmd-result/1` 等信封 | `rscmd-result/1` 等 | 字段与语义直接沿用 |
| 退出码 0/1/2/3/4/5/130 | 同 | 数值与含义不变 |
| `info --json` schema | `info/schema --json` | 增加全量 schema 导出 |
| **DAG 任务编排（`needs/strategy/cmd`）** | **P0 不做** | 单工具调用不需要；后续如需再立 ADR，避免一开始背执行引擎复杂度 |
| Python extras 按域可选 | 单一 wheel + feature 构建 | 编译型产物无法“缺依赖即跳过工具” |
| hatchling + uv | maturin + cargo | Python 侧只做绑定 |

---

## 15. 风险与待验证点

| 风险 | 影响 | 处理 |
|---|---|---|
| PyO3 abi3 + clap/serde 组合的编译与类型摩擦 | 拖慢 P1 | P0 末做一次最小 spike：一个工具同时跑 CLI 与 `invoke` |
| maturin 读取 workspace 继承版本 | 版本不同步 | 在 P1 加“tag/ wheel / `__version__` 三方一致”CI 检查 |
| maturin sdist 对 workspace 路径依赖的打包完整性 | 源码安装失败 | 先在 P1 验证；不满足则不做 sdist，只发 wheel + 原生二进制 |
| Rust 编译时间高于 fcmd 的即时迭代 | 开发体验 | 按 `-p <crate>` 局部检查；CI 用缓存；避免不必要的泛型单态化 |
| 95% 覆盖率在 Rust 侧难达标 | 门禁频繁红 | 生成代码与 `main.rs` 薄胶水用 `--ignore-filename-regex` 明确排除，业务代码不得放水 |
| Windows 路径/编码/换行差异 | 跨平台行为不一致 | CI 三平台矩阵；路径统一 `PathBuf`；输出统一 UTF-8 + `\n` |
| 全量工具二进制体积 | 安装/分发成本 | 记录体积基线；提供 `slim` 构建 |
| 工具函数 panic | 批量调用中断 | 调度边界 `catch_unwind` → `RUNTIME_ERROR`，并有测试覆盖 |
| PyPI 名称被占用 | 无法发布 | P0 前先查名并确定备用名（如 `rscmd-cli`） |

---

## 附录 A：面向小模型调用的工具契约清单

每个工具合并前必须逐条满足（可作为 PR 检查表）：

- [ ] `ToolMeta.summary` ≤ 200 字符，说明“做什么 + 关键约束”。
- [ ] 至少 1 个可直接复制的 `examples`。
- [ ] `--json` 输出 `rscmd-result/1`；错误输出 `rscmd-error/1` 且退出码与 `kind` 一致。
- [ ] 错误包含 `code`；可定位时补 `field` / `expected` / `hint`。
- [ ] 输出确定性：同一输入逐字节一致（并行结果显式排序）。
- [ ] 输出有界：支持 `--limit` 或 `--max-bytes`，截断时 `truncated: true`。
- [ ] 写操作默认 `--dry-run`，真正执行需 `--apply`/`--yes`；无交互提示。
- [ ] 声明 `side_effects`；`Destructive` 必须同时有 dry-run 与确认测试。
- [ ] 无 GUI 依赖；`gui = true` 的工具不出现在 `tools --for-llm` 列表。
- [ ] 有对应单测 + CLI 黄金测试；覆盖率计入所在 crate 的 95% 门禁。
- [ ] 若暴露给 Python：`invoke` 路径与 CLI 路径共用同一 `run` 函数，且有异常字段测试。

## 附录 B：关键命令速查

```bash
# 开发内环
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run -p rscmd-tool-text
cargo xtask registry-check

# 全量门禁
make check

# Python
maturin develop --release
pytest tests/py -m "not slow"
python -c "import rscmd; print(rscmd.__version__)"

# 发布（人工确认 tag）
cargo set-version --workspace 0.2.0
cargo xtask gen-docs
git commit -am "chore(release): v0.2.0"
git tag v0.2.0 && git push origin v0.2.0
```
