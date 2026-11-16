# 更新日志

本文件按版本分段（标题形如 `## [0.1.0] - 2026-11-16`）。
发布前 `cargo xtask version-check` 会校验 `[workspace.package].version` 存在对应段落。

## [0.1.0] - 2026-11-16

### 新增

- 错误码到错误大类的中央登记表 `ERROR_CODE_KINDS`；`RscmdError::coded` 依表推导 `kind` 与退出码，
  未登记码统一回退 `invalid_input`（退出码 3），避免同一错误码在不同调用点给出不同分类。
- `rscmd schema` 的输出改为 `rscmd-schema/1` 自描述对象：`version`、`tools`、`builtins`、`global`。
  （契约变更：此前 `data` 直接是工具元数据数组。）
