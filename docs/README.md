# rscmd 文档索引（自动生成，勿手改）

本文件由 `cargo xtask gen-docs` 生成。

## 文件列表

- [tools.md](./tools.md) — 全部工具元数据表格
- [global-args.md](./global-args.md) — CLI 全局参数说明
- [schemas.md](./schemas.md) — 各工具参数 Schema 摘要

## 快速校验

```bash
cargo xtask check-fast   # fmt + clippy + registry + arch + docs + schema + deny + machete
cargo xtask bench        # 启动时间基准回归
cargo xtask release-preflight  # 发布前全量预检
```

## 当前状态

- 工具数量：**3**
- Schema 文件：`schema/` 下 5 个 `.json`
