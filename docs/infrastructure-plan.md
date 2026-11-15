# rscmd 底层能力补齐计划（对照 fcmd）

- 日期：2026-11-15
- 范围：底层能力，不含业务工具。覆盖命令注册与分发、结果/错误信封、退出码、参数与 schema 投影、配置与状态目录、日志与输出层级、并发与超时、文件/网络基础设施、测试与门禁、打包发布。
- 性质：**本轮只出计划，不改代码**。工作区中已有的统一入口改动保持原样，不在本计划内重复评估。

## 0. 结论先行

rscmd 在「统一入口、信封、退出码、schema 投影、fs/net 运行时」上已有可用骨架，与 fcmd 的差距主要不在功能，而在**门禁与发布链的完整性**以及**少量契约/DX 细节**。

| 优先级 | 事项 | 粗估 |
|---|---|---|
| P0 | CI 触发分支与门禁错位、覆盖率未达标、PyPI wheel 非跨平台、发布版本无一致性校验、Python 入口产物可能陈旧 | 约 3–4 人日 |
| P1 | schema 输出包装、错误码→kind 中央映射、`--verbose`、全局 `--timeout` 与 Ctrl+C 退出码、CHANGELOG/版本流程 | 约 3–4.5 人日 |
| P2 | shell completion、MCP 薄封装、网络重试/缓存、Python 侧类型检查与多平台 wheel 测试、配置/状态目录、信封 `subcommand` 字段 | 按需，约 5–7 人日 |

建议顺序：**P0 全部完成后进入 P1**；P1 内先契约（错误映射、schema）后 DX（verbose、timeout）；P2 只在出现真实需求时启动。

## 1. 已确认事实（直接引用，不再重复核对）

以下为实施本计划前已核对/实测的结论，本计划不重新验证：

1. **统一入口已存在**：`rscmd run <工具>` + 工具直达 `rscmd <工具>`，内建 `tools/info/schema/version/doctor`（含短别名）；无参数打印帮助并 `exit=0`；`run` 已纳入保留命令，工具不得占用。
2. **doctor 已具备检查语义**：检查项为「工具注册表 / 工具数量 / 工作目录」，输出 `checks[{item,ok,detail,fix}]` 与汇总；结论不通过时 stdout 写 `ok:false` 信封、退出码 5，不写 stderr。
3. **覆盖率现状**：阈值 95%，HEAD 基线 93.21%（1518 行 / 103 未覆盖），当前 93.70%（1651 行 / 104 未覆盖）；主因是既有 `net.rs`（158 行 / 54 未覆盖，65.82%），新增 `builtins.rs` 为 98.44%。门禁在 HEAD 上本就是红的。
4. **Python 入口陈旧问题已实测**：`.venv` 为 editable 安装，`rscmd.pth` 指向仓库 `python/`；`python/rscmd/_native.pyd` 未重建时，console script 与 `python -m rscmd` 会表现为旧 CLI（实测 `unrecognized subcommand 'run'`、无参数报 `MISSING_COMMAND`）。已用 `maturin develop` 重建、`uv tool install --editable .` 安装到 PATH，并验证四种调用形式可用。
5. **CI/发布现状**：`.github/workflows/ci.yml` 触发分支为 `main`，仓库当前分支为 `master`，且 `origin/master` 上游跟踪分支已不存在；CI 只跑 `check-fast` + `cargo test` + `bench`，不跑 `check-cov` 与 Python 测试。`release.yml` 由 `v*` tag 触发，wheel 只在 ubuntu 上构建一次。
6. **xtask 能力**：`check-fast`（fmt/clippy/registry/arch/docs/schema/deny/machete）、`check`（+Rust 测试+Python 测试）、`check-cov`（95，忽略 `rscmd-py/`、`xtask/`）、`bench`（30 次 `--version`，基线 10ms × 2）、`wheel-smoke`、`release-preflight`（check-fast + test + py-check + bench + wheel-smoke，**不含 cov**）。
7. **fcmd 关键事实**：命令注册为「`fcmd` 主路由 + 每工具 console script + 动态发现 + 清单快路径」；信封为 `fcmd-result/1` / `fcmd-error/1`；退出码表与 rscmd 完全一致；参数由函数签名投影为 argparse 并生成 `param_schema`；无集中配置/状态目录；日志基于 Rich 与 `print_verbose`；并发/超时由任务图 `RunConfig`/`RetryPolicy` 承担；门禁为 Makefile + 95% 覆盖率 + ruff + pyrefly；发布为 hatchling + uv build + twine/PyPI OIDC + bump-my-version。

## 2. 逐项对照

### 2.1 命令注册与分发

- **fcmd**：`pyproject [project.scripts]` 注册 `fcmd` 与大量工具级脚本；`cli/main.py` 做「版本 → 内建命令 → 工具名快路径 → 全量发现」路由；`cli/_discovery.py` 扫描 `fcmd.tools`/`fcmd.apps`；`cli/_manifest.py` 用生成清单 `_manifest_data` 只加载目标工具，清单过期回退全量发现；`cli/_builtins/__init__.py` 集中分发内建命令；`cli/_entry.py` 处理工具独立入口。
- **rscmd**：单入口 `rscmd`；`dispatch.rs::root_command` 注册 `run`、各工具直达子命令与 5 个内建命令；`run_matches` 两级分发（`run` 嵌套/直达等同）；`registry.rs::BUILTIN_COMMANDS` 做保留名校验；`rscmd-py` 暴露 `run_argv`/`cli_main`。
- **差距**：rscmd 无动态发现、无清单快路径、无每工具脚本——这些是 Python 启动成本与分发包形态的产物；rscmd 为静态注册表，编译期即可确定。真正缺的是 shell completion 与 MCP 桥接（见 P2）。
- **结论**：**不补发现/清单机制**；P2 再评估 completion 与 MCP 薄封装。

### 2.2 结果与错误信封

- **fcmd**：`core/result.py` 定义 `fcmd-result/1`（`schema/ok/tool/subcommand/data`）与 `fcmd-error/1`（`schema/ok/tool/subcommand/kind/code/message/exit_code` 等）；`emit_result`/`emit_error` 统一出口；`--json` 走机读，人读走 Rich。
- **rscmd**：`envelope.rs` 定义 `rscmd-result/1`（`schema/ok/tool/data`）与 `rscmd-error/1`（`schema/ok/tool/kind/code/message/field/expected/hint/exit_code`）；`render.rs` 统一决定 stdout/stderr 与人读/机读；doctor 的结论性失败走 `ok:false` 信封。
- **差距**：rscmd 无 `subcommand` 字段（当前工具无子命令，暂无实际信息量）；fcmd 有具体错误码→kind 的中央映射表，rscmd 由构造点逐个指定 kind，存在漂移风险；两边都缺少“信封字段清单”的文档化契约。
- **结论**：信封主体已对齐，**P1 补错误码映射与契约测试**；`subcommand` 字段列为 P2，等出现子命令再加。

### 2.3 退出码体系

- **fcmd**：`core/exit_codes.py` 唯一定义：0 成功、1 运行失败、2 用法错误、3 输入非法、4 目标不存在、5 检查不通过、130 中断；`kind ↔ exit_code` 一一对应，`ERROR_CODE_KINDS` 登记具体错误码，未登记回退 `input`。
- **rscmd**：`errors.rs::ErrorKind` 数值与上表完全一致（Runtime=1/Usage=2/InvalidInput=3/NotFound=4/CheckFailed=5/Interrupted=130）；`Outcome::exit_code` 处理 doctor 的 5；错误信封带 `exit_code`。
- **差距**：数值与语义已一致；缺「具体 code → kind」的单一映射与覆盖测试；缺 Ctrl+C 到 130 的实际信号接线（见 2.7）。
- **结论**：**P1 补映射表与测试**，不改码值。

### 2.4 参数与 schema 投影

- **fcmd**：`core/toolkit/args.py` 从 `@fx.tool` 函数签名生成 argparse 参数与 `param_schema()`；`cli/_schema.py` 汇总工具元数据、子命令 schema、GUI 标记，形成 `fcmd-schema/1` 与 `fcmd info --json` 契约。
- **rscmd**：clap derive 生成 CLI，schemars 生成 JSON Schema；`schema/{meta,global,case,wc,json_fmt}.json` 为生成物，`gen-schema`/`schema-check` 保证与代码一致；`rscmd tools/info/schema` 提供机读元数据，`tools --for-llm` 支持过滤。
- **差距**：rscmd 的 schema 能力更强（编译期类型、生成物校验），但 `rscmd schema` 直接返回工具元数据数组，没有版本化信封，也未包含内建命令与全局参数；fcmd 有 `fcmd-schema/1` 包装。另外 rscmd 目前没有 schema 文档之外的契约说明。
- **结论**：**P1 把 schema 输出包装为 `rscmd-schema/1`**（含 tools/builtins/global），同步重生成 `docs/schemas.md`；不引入 fcmd 的签名反射。

### 2.5 配置与状态目录

- **fcmd**：无集中配置文件、无 XDG/APPDATA 目录约定；仅有 `FCMD_IMPORT_DEBUG` 这类诊断环境变量，执行相关配置通过 `RunConfig`（cwd/env）传递。
- **rscmd**：全仓无 config/state/cache/data_dir 约定；`ToolCtx` 仅携带 `dry_run/confirm/quiet/limit/workers`。
- **差距**：两边都为空，不存在“需要追赶”的成熟方案。
- **结论**：**现在不补**。等第一个需要持久化状态/缓存下载的工具出现时，先写 ADR 定义 `RSCMD_HOME`（含 `cache/`、`state/`、`logs/`）与优先级，再实现；列入 P2 触发条件。

### 2.6 日志与输出层级

- **fcmd**：`console.py` 封装 Rich，提供 `print_error`/`print_verbose`；`verbose` 在任务执行器中打印生命周期与返回码；没有 `--log-file`、没有 tracing；非 TTY/CI 降级未形成统一策略。
- **rscmd**：`render.rs` 只有 human / `--json` / `--quiet` 三态；无 `--verbose`、无日志文件、无 tracing；错误固定写 stderr，成功结果写 stdout；doctor/version 有人读渲染。
- **差距**：排障时看不到中间过程；模型/CI 场景缺少“保留 stdout 纯净、把过程写 stderr 或文件”的手段。Rich 的表格化输出不适合 rscmd 的机读优先定位。
- **结论**：**P1 增加 `--verbose`（只写 stderr，不改变 stdout/JSON/quiet）**；`--log-file` 列为 P2，等 CI/代理场景有明确需求再加。

### 2.7 并发与超时

- **fcmd**：`core/task.py` 定义 `RunConfig`（并发策略、线程数、dry-run、verbose、only/tags、超时、重试）与 `RetryPolicy`；`core/dag.py` 提供拓扑分层与依赖推断；`core/execution/executors.py` 实现 dependency/sequential/thread/async 四种执行器。
- **rscmd**：`runtime/concurrency.rs` 提供 `CancellationToken` 与 `bounded_parallel_map`（保序、可提前退出）；`runtime/proc.rs` 提供带超时、流式读取、输出截断的子进程执行；CLI 有 `--workers/--limit` 并写入 `ToolCtx`，但当前工具未消费；`ErrorKind::Interrupted` 存在，但没有信号接线。
- **差距**：缺全局 `--timeout` 的实际消费与 Ctrl+C → 130 的闭环；不存在跨工具任务图，也没有重试策略。任务图对当前 3 个独立工具是过度设计。
- **结论**：**P1 补 `--timeout` 与中断语义**（复用 `proc`/`net` 的超时能力，信号处理映射到 `Interrupted`）；**不移植 DAG/执行器/RetryPolicy**，需要重试时只在网络层做局部重试。

### 2.8 文件与网络基础设施

- **fcmd**：`infras/fs.py`（搜索、路径规范化、相对路径、时间戳、日期前缀）与 `infras/net.py`（GET/POST/HEAD、URL 检查、下载保存、链接提取、IP/子网校验、镜像测速）。
- **rscmd**：`runtime/fs.rs` 提供原子写（`.rscmd-tmp-*` + rename）、临时目录、路径规范化、文本判定、UTF-8 读取、递归遍历；`runtime/net.rs` 提供 `download`/`download_resume`/SHA-256 校验，支持 `file://`/`http(s)://`、blocking client 与超时；`runtime/text.rs`、`runtime/data.rs` 提供文本与 JSON 基础能力。
- **差距**：rscmd 在原子写、断点续传、校验和方面已优于 fcmd；缺网络重试/退避、代理配置约定、下载缓存目录；fs 与工具的接线面还很小。
- **结论**：**P2 按需补网络重试与缓存**（与 2.5 的目录约定绑定）；fs 不新增抽象。

### 2.9 测试与门禁

- **fcmd**：Makefile 目标 `lint`（ruff check/format）、`typecheck`（pyrefly）、`test`（`-m "not slow"`，xdist 8 worker）、`cov`（阈值 95）、`check-fast`/`check`、`prek`、`tox`、`bench-filescan --check`、`gitkeep-check`/`adr-check`。
- **rscmd**：xtask 统一入口，覆盖 fmt/clippy/registry/arch/docs/schema/deny/machete + Rust 测试 + Python 测试 + 覆盖率 + 启动 bench（阈值 10ms×2）+ wheel-smoke + release-preflight。CI 现状见 §1.5：**分支错位、不跑覆盖率与 Python 测试**；`release-preflight` 也不含覆盖率。
- **差距**：门禁种类不输 fcmd，输在“是否真的在 CI 上执行”；覆盖率因既有 `net.rs` 欠账长期红；Python 侧无类型检查；慢用例无 `slow` 标记约定（当前用例均在 0.5s 内）。
- **结论**：**P0 修 CI 分支/任务与覆盖率**（只补测试，不排除文件、不降阈值、不加 pragma）；P2 再评估 Python 类型检查与慢用例标记。

### 2.10 打包发布

- **fcmd**：hatchling + `uv build`（wheel + sdist）；`twine`/`uv publish` 发布；`bump-my-version` 打 tag 并同步锁文件；Makefile 提供多远程 push。
- **rscmd**：maturin + pyo3 abi3（`cp310-abi3`），`python-source=python`，单一 console script；版本来自 Cargo；`release.yml` 在 `v*` tag 上构建三平台二进制并上传 artifact，`publish` job 在 ubuntu 上跑 `release-preflight`、`maturin build`、twine 上传 PyPI、创建 GitHub Release。
- **差距**：(a) **wheel 只在 ubuntu 构建**，产物是 manylinux，Windows/macOS 用户无法从 PyPI 安装；(b) 无 tag ↔ Cargo 版本 ↔ wheel 版本一致性校验；(c) 无 CHANGELOG；(d) 无 sdist；(e) 开发态的 `_native.pyd` 陈旧问题会静默回退 CLI 行为。
- **结论**：**P0 补三平台 wheel 矩阵与版本一致性**；**P0 补 Python 产物新鲜度检查**；P1 补 CHANGELOG；sdist 视发布策略决定。

## 3. 优先级、工作量与依赖

| ID | 事项 | 优先级 | 粗估 | 依赖 | 验收口径 |
|---|---|---|---|---|---|
| P0-1 | CI 门禁对齐：触发分支改为实际分支（或统一分支名）；CI 增加 `check-cov` 与 Python 测试；`release-preflight` 纳入覆盖率 | P0 | 0.5 人日 | 先确认分支策略（master/main）与远端状态 | push/PR 在目标分支上真实触发；job 列表含 cov 与 py；`release-preflight` 本地可复现 |
| P0-2 | 覆盖率转绿：用本地 `TcpListener` 覆盖 `net.rs` 的下载成功/断点/校验失败/超时分支 | P0 | 1–1.5 人日 | 无（可与 P0-1 并行） | `cargo xtask check-cov` 退出 0，TOTAL ≥95%，`net.rs` 不低于仓库平均；不排除文件、不降阈值 |
| P0-3 | PyPI wheel 跨平台：release 增加 linux/windows/macos 构建矩阵（abi3） | P0 | 0.5–1 人日 | P0-1 | 三平台各产 wheel 并上传；三平台 `pip install` 后 `rscmd version`/`run` 可用 |
| P0-4 | 发布版本一致性：preflight 校验 tag=`vX.Y.Z` 与 Cargo workspace 版本、wheel 版本一致 | P0 | 0.25 人日 | P0-3 | 不一致时 preflight 非零退出；一致时通过 |
| P0-5 | Python 产物新鲜度：新增 xtask 检查（`_native.pyd` 落后于 Rust 源码即失败并提示 `maturin develop`），并把开发流程写入现有文档 | P0 | 0.25–0.5 人日 | 无（事实见 §1.4） | 人为改一个 `.rs` 后检查失败；重建后通过；不影响纯 Rust 构建 |
| P1-1 | schema 输出包装为 `rscmd-schema/1`，含 tools/builtins/global；重生成 `docs/schemas.md` | P1 | 0.5 人日 | P1-2（先稳定错误/参数契约） | `rscmd schema --json` 有版本字段；契约测试固定字段；`docs-check` 通过 |
| P1-2 | 具体错误码 → kind 的中央映射与测试（对齐 fcmd `ERROR_CODE_KINDS`） | P1 | 0.5 人日 | 无 | 表驱动测试覆盖全部登记码；未登记码回退规则固定 |
| P1-3 | `--verbose`：仅 stderr，stdout/JSON/quiet 语义不变 | P1 | 0.5–1 人日 | 无 | 三种输出模式的快照/契约测试通过；`--json` 下 stderr 不污染 stdout |
| P1-4 | 全局 `--timeout` + Ctrl+C → 130：写入 `ToolCtx` 并被 `proc`/`net` 消费；信号映射 `Interrupted` | P1 | 1–1.5 人日 | P1-3（共用 dispatch/ctx 改动） | 超时用例返回 3/1 中约定码；中断返回 130；JSON 错误信封含 `kind/exit_code` |
| P1-5 | CHANGELOG 与版本流程：按版本分段；preflight 校验段落存在 | P1 | 0.5 人日 | P0-4 | 发布前置检查拒绝缺段版本 |
| P2-1 | shell completion（clap_complete，bash/zsh/fish/pwsh） | P2 | 0.5 人日 | 无 | 生成脚本可在对应 shell 补全子命令 |
| P2-2 | MCP 薄封装：把 `schema` 元数据映射为 MCP tools | P2 | 2–3 人日 | P1-1 | 最小 MCP server 能列出并调用 `case`/`wc` |
| P2-3 | 网络重试/退避 + 下载缓存目录 | P2 | 1 人日 | §2.5 目录 ADR | 失败重试次数与退避可配置；缓存命中有测试 |
| P2-4 | Python 侧类型检查与三平台 wheel 冒烟 | P2 | 1 人日 | P0-3 | CI 中 mypy/pyright 通过；三平台导入冒烟通过 |
| P2-5 | 配置/状态目录 resolver（`RSCMD_HOME`/APPDATA，cache/state/logs） | P2 | 0.5–1 人日 | 首个持久化需求 + ADR | 目录优先级测试；写入失败有明确错误码 |
| P2-6 | 信封 `subcommand` 字段 | P2 | 0.25 人日 | 出现真实子命令 | 信封版本号与字段契约测试同步更新 |

### 依赖顺序

```
P0-1 ─┬─> P0-3 ─> P0-4 ─> P1-5
      └─> P0-2（可并行）
P0-5（独立）
P1-2 ─> P1-1
P1-3 ─> P1-4
P2-1 / P2-2 / P2-3 / P2-4 / P2-5 / P2-6：按需求触发，互不阻塞
```

## 4. 不照搬清单

1. **动态工具发现 + `_manifest_data` 指纹快路径 + 懒加载**：rscmd 是静态注册表的单二进制，工具数量个位数；清单会引入第二份事实源与过期回退逻辑，收益为负。
2. **每个工具一个 console script + Typer/argparse 签名投影**：fcmd 的形态服务于 Python 包的多入口分发；rscmd 的单入口 `rscmd` 是面向模型调用的有意设计，clap + schemars 已覆盖参数与 schema。
3. **Rich 表格/Unicode 检测/彩色排版作为默认输出**：rscmd 以 stdout 契约优先，人读只对 `doctor`/`version` 做最小渲染；引入 Rich 类输出会让机读与快照测试变脆。
4. **DAG/任务执行器/RetryPolicy 全家桶**：当前工具之间没有依赖图；已有 `CancellationToken` + `bounded_parallel_map` + `proc` 超时，按需组合即可，移植任务图属于过度设计。
5. **MCP adapter（600+ 行，含资源与提示词体系）**：等确有客户端需求时，用 schema 元数据做薄封装；不复制 fcmd 的资源/提示词组织方式。
6. **集中配置中心/XDG 体系**：fcmd 自身也没有；在首个持久化/缓存需求出现前设计只会空转，先写 ADR 再落地。
7. **tox / pre-commit(prek) / 多远程 push 流程**：属于发布习惯与仓库策略，不是底层能力；rscmd 用 xtask 单入口已覆盖等价校验。
8. **Python 侧重复维护的元数据与参数清单**：schemars + `schema-check` 已是单一事实源，双写会重演 fcmd 的清单漂移问题。

## 5. 风险与开放问题

1. **分支策略未定**：CI 触发 `main`，本地为 `master`，且 `origin/master` 上游引用不存在（upstream gone）。P0-1 开工前必须先确认目标分支与远端，否则改了 CI 也不会触发。
2. **覆盖率修复边界**：只允许补测试（本地 HTTP mock），不允许排除文件、降阈值或加 pragma；若 `net.rs` 存在确实不可达的平台分支，需在报告里逐行说明并由用户拍板，不能静默处理。
3. **PyPI 发布是否必要**：若 rscmd 仅内部使用，P0-3/P0-4 可降为 P1；若对外发布，三平台 wheel 是硬需求。
4. **开发态 Python 绑定**：`uv tool` 与 `.venv` 都通过 `rscmd.pth` 复用仓库内 `_native.pyd`，任何 Rust 改动后都需 `maturin develop`；P0-5 是防止该问题再次静默发生的兜底。
5. **状态目录的触发条件**：只有当出现“下载缓存/可恢复任务/用户级配置”时，P2-5 才应启动；否则保持无状态。
