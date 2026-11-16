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
