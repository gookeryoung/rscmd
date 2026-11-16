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

### 变更

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
