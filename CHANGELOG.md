# 更新日志

本文件按版本分段（标题形如 `## [0.1.0] - 2026-11-16`）。
发布前 `cargo xtask version-check` 会校验 `[workspace.package].version` 存在对应段落。

## [0.1.0] - 2026-11-16

### 新增

- 错误码到错误大类的中央登记表 `ERROR_CODE_KINDS`；`RscmdError::coded` 依表推导 `kind` 与退出码，
  未登记码统一回退 `invalid_input`（退出码 3），避免同一错误码在不同调用点给出不同分类。
- 新增全局 `--verbose`/`-v`：在 stderr 输出 `[rscmd] tool=<工具名> elapsed=<毫秒>ms` 执行跟踪，
  stdout 的人类可读文本与 JSON 信封保持原样。
- `rscmd schema` 的输出改为 `rscmd-schema/1` 自描述对象：`version`、`tools`、`builtins`、`global`。
  （契约变更：此前 `data` 直接是工具元数据数组。）
- 新增全局 `--timeout <SECONDS>`（0 表示不限制）：解析后写入 `ToolCtx.timeout`，工具内的
  子进程（`rscmd_runtime::proc`）与网络请求（`rscmd_runtime::net`）通过 `ToolCtx::timeout_or`
  统一取值；未设置时沿用工具自身的默认超时。
- Ctrl+C 现在以退出码 130 结束：CLI 注册 SIGINT 处理，按当前输出模式渲染 `kind=interrupted`
  的错误信封（`--json` 下为写 stderr 的 `rscmd-error/1`）。
- `ERROR_CODE_KINDS` 登记 `TIMEOUT`（runtime，退出码 1）与 `INTERRUPTED`（interrupted，退出码 130）。
- 新增内建命令 `rscmd completion <SHELL>`：按完整命令树生成 bash / zsh / fish / powershell 补全脚本，
  脚本原文走结果 `text` 字段（`--json` 下为 `rscmd-result/1` 信封）。
- 新增工具 `hash`（crypto 域，别名 `hashtool`）：计算文本或文件的 SHA-256，返回 `hex`/`bytes`/`source`；
  `TEXT` 与 `--file` 二选一，同时给出或都不给按 `INVALID_ARGS`（退出码 3）拒绝。
- `ERROR_CODE_KINDS` 登记 `IO`（runtime，退出码 1），用于文件读取失败等运行期 IO 错误。
- `rscmd doctor` 新增两项检查：`Python 扩展`（`_native` 产物是否旧于最新 Rust 源码，安装态或未构建时跳过）
  与 `终端编码`（控制台代码页不是 UTF-8 时给出 `chcp 65001` 建议），检查项由 3 项增至 5 项；
  环境结论可通过 `DoctorProbe` 注入，便于单测覆盖各分支。
- 结果信封新增可选 `subcommand` 字段：工具带下级子命令时写入子命令名；普通工具不出现该键。
  属兼容扩展（`rscmd-result/1` 不变），无子命令时消费方看到的字段集合与之前完全一致。
- 新增 `rscmd mcp`：以 MCP stdio server 模式运行，支持 `initialize`、`ping`、`tools/list`、
  `tools/call`。仅暴露有 JSON 入口的非 GUI 工具，`inputSchema` 直接复用工具 schema；
  协议版本 2025-06-18。未引入第三方依赖；该模式只由 CLI 可执行文件提供。
- 网络层新增可配置重试策略 `RetryPolicy { retries, backoff }` 与 `download_with` /
  `download_resume_with`：默认 0 重试（保持原有行为），仅 5xx 与超时重试，退避按 2 的幂
  递增且单次封顶 30s；4xx、scheme、Range 与校验失败不重试。
- `DownloadError` 新增 `code()` 统一错误码（`NET_TIMEOUT` / `NET_HTTP_STATUS` /
  `NET_TRANSPORT` / `UNSUPPORTED_SCHEME` / `RANGE_NOT_SUPPORTED` /
  `RANGE_NOT_SATISFIABLE` / `CHECKSUM_MISMATCH` / `IO`），并登记进 `ERROR_CODE_KINDS`。
- `rscmd-runtime` 新增目录 resolver（`home::Dirs::resolve`）：按 `RSCMD_HOME` >
  Windows `LOCALAPPDATA` / macOS `~/Library/Application Support` / Linux
  `XDG_DATA_HOME` 或 `~/.local/share` 解析统一根目录，环境变量、平台与主目录均可注入；
  子目录 `cache/downloads`、`state`、`logs` 只拼接不创建，见 ADR 0001。
- `rscmd-runtime` 新增下载缓存（`cache::DownloadCache`）：URL 规范化 SHA-256 内容寻址、
  默认 TTL 7 天、256 MiB / 1024 条容量上限与写后淘汰；损坏、过期、孤立文件自动清理。
  `net::download_with_cache` 命中不重复下载，未启用或目录不可写时静默降级为直接下载。
  纯新增能力，现有工具行为与 JSON 契约不变。

### 变更

- 断点续传遇到 4xx/5xx 时按 HTTP 状态码报错（此前一律报「不支持 Range」）；
  2xx 但非 206 仍报 `RANGE_NOT_SUPPORTED`。
- **契约变更（breaking）**：`rscmd tools --json` 的 `data` 由裸数组改为 `rscmd-tools/1` 版本信封
  （`version` + `tools`），与 `rscmd schema` 的自描述风格一致。旧消费方需把 `data` 改为读取
  `data.tools`；单条元数据字段（`name`/`aliases`/`side_effects`/`python`/`gui`/`examples`）不变。
- `rscmd schema` 的每个工具条目固定输出 `aliases` 与 `side_effects`，并用契约测试锁定
  （字段此前已输出，本次只补契约与文档，属兼容扩展）。
- CLI 帮助与命令一览中的短别名改为 `doctor(d)` 形式：内建命令的名称/别名/说明集中登记在
  `registry::BUILTINS`，帮助由该表构造，`cargo xtask gen-docs` 新增 `docs/commands.md`。
- 注册表校验的保留名范围扩展到内建命令短别名（`r`/`i`/`v`/`d`）：工具名或别名与内建命令撞车时
  `cargo xtask registry-check` 直接失败，避免把重复的 clap 子命令/别名留到运行期才暴露。
- Python 侧加入类型检查：选 mypy（纯 Python 实现，CI 无需额外运行时），配置在 `pyproject.toml`
  的 `[tool.mypy]`（`strict`，覆盖 `python/rscmd` 与 `tests/py`）；新增 `python/rscmd/_native.pyi`
  声明原生边界签名，`_raise_native` 标注 `NoReturn`。`cargo xtask py-type-check` 已并入 `check`。
- CI 新增 `wheel-smoke` job（ubuntu/windows/macos）：用 `cargo xtask wheel-smoke` 构建 release
  wheel、装进 `.venv` 后验证导入、成功调用与错误映射；`python` job 同时执行 mypy。两项只做测试，
  不改变发布物与发布流程。
- `cargo xtask version-check` 现在校验 `CHANGELOG.md` 存在 `[workspace.package].version` 对应段落，
  缺段直接失败并提示补写，避免发布时漏写更新日志。
- 新增 `cargo xtask code-check`：扫描 `crates/**` 中 `RscmdError::coded("...")` 字面量，未在
  `ERROR_CODE_KINDS` 登记即失败；已并入 `check-fast`，防止漏登记导致 kind 回退成 `input`（退出码 2）。
