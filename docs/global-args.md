# CLI 全局参数（自动生成，勿手改）

本文件由 `cargo xtask gen-docs` 生成，Schema 来自 `GlobalArgs` 的 `JsonSchema derive`。

所有全局参数以 `_rscmd_` 内部 id 注册，避免与工具自身参数冲突。

```
完整 JSON Schema: schema/global.json
```

| 参数 | 类型 | 默认 | 说明 |
|---|---|---|---|
| `--confirm` | boolean | `—` | 确认执行有副作用的操作 |
| `--dry_run` | boolean | `—` | 仅预演，不产生副作用 |
| `--json` | boolean | `—` | 以 JSON 信封输出结果 |
| `--limit` | integer | `—` | 结果条数上限，0 表示不限制 |
| `--quiet` | boolean | `—` | 人读模式下不输出成功结果 |
| `--timeout` | integer | `—` | 单次调用的超时秒数，0 表示不限制；工具内的子进程与网络请求据此收敛 |
| `--verbose` | boolean | `—` | 在 stderr 输出工具名与耗时跟踪，不改变 stdout 契约 |
| `--workers` | integer | `—` | 并行度，0 表示自动 |
