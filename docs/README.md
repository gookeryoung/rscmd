# rscmd 文档索引（自动生成，勿手改）

本文件由 `cargo xtask gen-docs` 生成。

## 文件列表

- [commands.md](./commands.md) — 内建命令与工具命令一览（含短别名）
- [tools.md](./tools.md) — 全部工具元数据表格
- [global-args.md](./global-args.md) — CLI 全局参数说明
- [schemas.md](./schemas.md) — 各工具参数 Schema 摘要

## 快速校验

```bash
cargo xtask check-fast        # fmt + clippy + registry + arch + docs + schema + py-fresh + deny + machete
cargo xtask py-type-check     # mypy 检查 python/rscmd 与 tests/py（未装 mypy 时跳过）
cargo xtask wheel-smoke       # 构建 wheel、安装进 .venv 并冒烟
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

类型检查选 mypy（纯 Python 实现，`pip install mypy` 即可，CI 不需要额外运行时），
配置在 `pyproject.toml` 的 `[tool.mypy]`，原生边界由 `python/rscmd/_native.pyi` 声明。
CI 的 `python` job 跑 mypy 与 pytest，`wheel-smoke` job 在三平台构建 wheel 并冒烟。

## 当前状态

- 工具数量：**4**
- Schema 文件：`schema/` 下 6 个 `.json`
