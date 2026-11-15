# rscmd 文档索引（自动生成，勿手改）

本文件由 `cargo xtask gen-docs` 生成。

## 文件列表

- [tools.md](./tools.md) — 全部工具元数据表格
- [global-args.md](./global-args.md) — CLI 全局参数说明
- [schemas.md](./schemas.md) — 各工具参数 Schema 摘要

## 快速校验

```bash
cargo xtask check-fast        # fmt + clippy + registry + arch + docs + schema + py-fresh + deny + machete
cargo xtask bench             # 启动时间基准回归
cargo xtask release-preflight # 发布前全量预检
```

## Python 绑定

`python/rscmd/_native.*` 是 `maturin develop` 的产物，console script 与 `python -m rscmd`
都直接复用它。Rust 侧改动后必须重建，否则入口会静默跑旧 CLI：

```bash
maturin develop              # 或 .venv/Scripts/maturin.exe develop
uv tool install --editable .
```

`cargo xtask py-fresh-check` 会在 `_native.*` 旧于 Rust 源码时失败并提示重建；本地未构建时跳过。

## 当前状态

- 工具数量：**3**
- Schema 文件：`schema/` 下 5 个 `.json`
