# rscmd

面向小模型调用的 Rust 工具集：**单一入口、稳定 JSON 信封、可自描述的 schema**。
业务逻辑全部在 Rust 侧，同时提供 CLI、Python 绑定和 MCP stdio server 三种调用方式。

- 18 个内置工具，覆盖文本、数据、加密、系统与网络
- 成功 / 错误信封版本化（`rscmd-result/1` / `rscmd-error/1`），退出码语义固定
- `--json` 机读优先，人读输出不污染 stdout 契约
- 工具元数据与参数 Schema 由代码生成，`rscmd schema --json` 可自描述
- Python 侧只做类型转换与异常映射，不复制业务逻辑

> 命令缩写以括号标注，例如 `rscmd doctor(d)`、`rscmd run(r)`。

## 安装

### 原生二进制

需要 Rust stable（`rust-version = 1.78`）：

```bash
cargo build --release -p rscmd-cli
# 产物：target/release/rscmd（Windows 为 rscmd.exe）

# 或安装到 Cargo bin 目录
cargo install --path crates/rscmd-cli
```

### Python 包

需要 Python 3.10+。已发布的 wheel 可直接安装：

```bash
pip install rscmd
```

源码开发态（editable，复用仓库内原生模块）：

```bash
uv venv .venv
uv pip install --python .venv/Scripts/python.exe maturin pytest   # Windows
# 或：uv pip install --python .venv/bin/python maturin pytest     # Unix
maturin develop

# 也可把仓库注册为全局 CLI
uv tool install --editable .
```

Rust 侧改动后必须重新执行 `maturin develop`，否则 `_native` 会静默运行旧实现；
`rscmd doctor(d)` 与 `cargo xtask py-fresh-check` 会检查该产物是否过期。

## 快速开始

```bash
rscmd doctor(d)                                  # 环境自检
rscmd tools                                      # 列出工具
rscmd run(r) case --mode snake HelloWorld        # 推荐入口：run + 工具名
rscmd case HelloWorld                            # 直达形式（等价）
rscmd --json run case --mode snake HelloWorld    # 机读信封
rscmd info(i) case --json                        # 单个工具的完整元数据
rscmd schema --json                              # 全部工具 + 内建命令 + 全局参数 schema
rscmd version(v) --json                          # 版本信息
```

输出示例：

```bash
$ rscmd run case --mode snake HelloWorld
hello_world

$ rscmd --json run case --mode snake HelloWorld
{"schema":"rscmd-result/1","ok":true,"tool":"case","data":{"text":"hello_world"}}
```

出错时错误信封固定写 stderr：

```bash
$ rscmd --json run json_fmt '{'
{"schema":"rscmd-error/1","ok":false,"tool":"json_fmt","kind":"invalid_input",
 "code":"INVALID_JSON","message":"...","field":"json","expected":"...","hint":"...",
 "exit_code":3}
```

## 命令一览

### 内建命令

| 命令 | 说明 |
|---|---|
| `rscmd run(r) <工具> [参数]` | 运行指定工具（推荐入口） |
| `rscmd tools` | 列出可用工具（`--for-llm` 过滤 GUI 工具） |
| `rscmd info(i) <工具>` | 查看单个工具的元数据 |
| `rscmd schema` | 导出全部工具的元数据与参数 Schema |
| `rscmd version(v)` | 输出版本信息 |
| `rscmd doctor(d)` | 输出运行环境自检信息 |
| `rscmd completion <SHELL>` | 生成 bash / zsh / fish / powershell 补全脚本 |
| `rscmd mcp` | 以 MCP stdio server 模式运行 |

完整列表见 [docs/commands.md](docs/commands.md)。工具别名（如 `casetool`）仍可调用，
但帮助与 `Tools:` 分组只展示规范名，别名见 [docs/tools.md](docs/tools.md)。

### 工具

| 域 | 工具 | 说明 |
|---|---|---|
| text | `case` | 命名风格转换（snake/camel/pascal/kebab） |
| text | `wc` | 统计行数、词数、字符数与字节数 |
| text | `regex` | 正则匹配、分组捕获与替换 |
| data | `csv` | CSV 与 JSON 互转，自动判定表头与分隔符 |
| data | `json_fmt` | 校验并格式化 JSON，非法输入返回可自纠错误 |
| data | `stat` | 计数、求和、均值、中位数与分位数 |
| data | `timestamp` | Unix 秒与 RFC3339 互转，支持固定偏移与日期加减 |
| data | `toml` | 校验 TOML 并转换为 JSON / YAML |
| data | `yaml` | 校验 YAML 并转换为 JSON / TOML |
| crypto | `hash` | 计算文本或文件的 SHA-256 |
| crypto | `codec` | base64 / hex / URL 文本编解码 |
| system | `archive` | 把目录打包成 zip（写需 `--confirm`） |
| system | `env` | 只读查询环境变量，支持名字与前缀过滤 |
| system | `filedate` | 读取或写入文件修改时间（写需 `--confirm`） |
| system | `filesearch` | 按文件名 glob 与内容子串检索文件 |
| system | `path` | 路径规范化、拼接、相对/绝对路径判定 |
| system | `which` | 在 PATH 中查找可执行文件 |
| net | `urlcheck` | 探测 URL 的 HTTP 状态码、重定向链与耗时 |

## 全局参数

| 参数 | 说明 |
|---|---|
| `--json` | 以 JSON 信封输出结果 |
| `--quiet` | 人读模式下不输出成功结果 |
| `--verbose` / `-v` | 在 stderr 输出工具名与耗时跟踪 |
| `--dry-run` | 仅预演，不产生副作用 |
| `--confirm` | 确认执行有副作用的操作 |
| `--workers <N>` | 并行度，0 表示自动 |
| `--limit <N>` | 结果条数上限，0 表示不限制 |
| `--timeout <SECONDS>` | 单次调用超时秒数，0 表示不限制 |

全局参数可放在子命令前后。完整 Schema 见 [schema/global.json](schema/global.json)，
说明见 [docs/global-args.md](docs/global-args.md)。

## 输出契约

- 成功：`{"schema":"rscmd-result/1","ok":true,"tool":"<工具>","data":<结果>}`
- 失败：`{"schema":"rscmd-error/1","ok":false,"tool":"<工具>","kind":"...","code":"...","message":"...","exit_code":<码>}`
  - 可选字段：`field` / `expected` / `hint`
- `rscmd doctor(d)` 结论不通过时输出 `ok:false` 的结果信封并退出 5
- `rscmd tools` 的 `data` 为 `{"schema":"rscmd-tools/1","version":"...","tools":[...]}`
- `rscmd schema` 的 `data` 为 `rscmd-schema/1`，含 `version` / `tools` / `builtins` / `global`
- 成功结果写 stdout，错误写 stderr；`--verbose` 只写 stderr

退出码：

| 退出码 | kind | 含义 |
|---|---|---|
| 0 | — | 成功 |
| 1 | `runtime` | 运行期失败（IO、网络、超时、锁等） |
| 2 | `usage` | 用法错误（未知命令、缺少参数、未确认写操作） |
| 3 | `invalid_input` | 输入非法（参数、JSON、编码、正则、解析失败等） |
| 4 | `not_found` | 目标不存在（工具名等） |
| 5 | `check_failed` | 检查类命令结论不通过（如 `doctor`） |
| 130 | `interrupted` | 用户按下 Ctrl+C |

错误码到大类的映射集中在 `ERROR_CODE_KINDS`，新增错误码必须登记。

## Python 绑定

```python
import rscmd

rscmd.__version__
tools = rscmd.tools()                                  # 全部工具摘要
meta = rscmd.describe("case")                          # 单个工具完整元数据
text = rscmd.invoke("case", {"text": "HelloWorld", "mode": "snake"})
envelope = rscmd.run_argv(["--json", "doctor"])        # 按 CLI 语义调用
code, stdout, stderr = rscmd.cli_main(["version"])     # CLI 级入口
```

失败时抛出 `rscmd.RscmdError`，可通过 `.code` / `.kind` / `.exit_code` /
`.field` / `.expected` / `.hint` 读取结构化字段。

```python
try:
    rscmd.invoke("json_fmt", {"json": "{"})
except rscmd.RscmdError as err:
    assert err.code == "INVALID_JSON"
    assert err.kind == "invalid_input"
    assert err.exit_code == 3
```

## MCP 与补全

```bash
# MCP stdio server：逐行 JSON-RPC，支持 initialize / ping / tools/list / tools/call
rscmd mcp

# shell 补全
rscmd completion bash        > rscmd.bash
rscmd completion zsh         > _rscmd
rscmd completion fish        > rscmd.fish
rscmd completion powershell  > rscmd.ps1
```

MCP 只暴露有 JSON 入口的非 GUI 工具，`inputSchema` 直接复用工具 Schema，
协议版本 2025-06-18。Rust 二进制与 Python console script 共用同一套 stdio 循环，
venv 内也可直接 `python -m rscmd mcp`；`rscmd.cli_main` 一次性返回输出，不承载该模式。

## 开发

```bash
make help            # 查看全部目标
make check-docs      # L0：文档生成物 + ADR 校验
make check-fast      # L1：fmt + clippy + 注册表/架构/文档/schema/deny/machete
make flow            # L2：check-fast + cargo test
make check           # 全量：check-fast + Rust 测试 + pytest + mypy
make cov             # llvm-cov，行覆盖率阈值 95%
make gen-docs        # 重新生成 docs/*.md
make gen-schema      # 重新生成 schema/*.json
make wheel-smoke     # 构建 wheel、装进 .venv 并冒烟
make bench           # 启动时间基准回归
make version-check   # tag / Cargo / wheel / CHANGELOG 版本一致
make push            # 推送所有 remote 与 tags（不自动打 tag）
```

完整门禁还需要 `cargo-nextest`、`cargo-llvm-cov`、`maturin`、`pytest`、`mypy`。
门禁分级与提交约定见 [AGENTS.md](AGENTS.md)，AI 驱动开发流见
[docs/AI_DRIVEN_DEV_FLOW.md](docs/AI_DRIVEN_DEV_FLOW.md)。

### 生成物

以下文件由工具生成，**请勿手改**：

- `docs/*.md`：`make gen-docs`
- `schema/*.json`：`make gen-schema`
- `python/rscmd/_native.*`：`maturin develop`

### 目录结构

```
crates/
  rscmd-cli        可执行入口（dispatch / 信号 / MCP）
  rscmd-core       调度、信封、错误、schema、内建命令
  rscmd-py         PyO3 绑定
  rscmd-runtime    fs / net / proc / lock / cache / home 等运行时
  rscmd-tools      工具聚合与 feature 开关
  rscmd-tool-*     text / data / crypto / system / net 工具域
  xtask            构建、生成物与门禁脚本
python/rscmd/      Python 包与原生边界类型声明
schema/            生成的 JSON Schema
docs/              生成文档 + ADR / 需求 / 方案
tests/py/          Python 契约测试
```

## 文档

- [docs/README.md](docs/README.md) — 文档索引（自动生成）
- [docs/commands.md](docs/commands.md) — 命令一览
- [docs/tools.md](docs/tools.md) — 工具元数据与别名
- [docs/global-args.md](docs/global-args.md) — 全局参数
- [docs/schemas.md](docs/schemas.md) — 工具参数 Schema 摘要
- [docs/adr/README.md](docs/adr/README.md) — 架构决策记录
- [CHANGELOG.md](CHANGELOG.md) — 更新日志

## 许可

MIT
