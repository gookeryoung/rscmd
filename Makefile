# Makefile - rscmd project shortcuts
# 运行 `make help` 查看全部命令。
#
# 与 fcmd 对齐的目标命名（语义映射见 AGENTS.md）：
#   check-docs / check-fast / flow / check / cov / test / push
# Rust 等价物：ruff+pyrefly → cargo fmt+clippy；pytest → cargo test；
#              pytest --cov → cargo llvm-cov；bump-my-version → 手动改版本 + CHANGELOG。

TOOL ?=

.PHONY: help check-docs adr-check docs-check check-fast test flow py-check check \
        gen-docs gen-schema cov wheel-smoke bench version-check prek push \
        test-tool cov-tool py-test-tool

help: ## 显示命令列表
	@echo   check-docs      L0 文档门禁（adr-check + 生成物 docs-check）
	@echo   check-fast      L1 配置/内环门禁（fmt + clippy + 注册表/架构/文档/schema/deny/machete）
	@echo   flow            L2 代码门禁（check-fast + test）
	@echo   check           全量门禁（check-fast + test + pytest + mypy）
	@echo   test            代码测试（cargo test，不跑覆盖率）
	@echo   cov             覆盖率（llvm-cov，阈值 95%，排除 rscmd-py / xtask）
	@echo   py-check        Python 侧 pytest（需 .venv）
	@echo   adr-check       校验 ADR 命名/编号/索引/废弃状态
	@echo   docs-check      校验 docs/*.md 生成物是否最新
	@echo   gen-docs        重新生成 docs/*.md
	@echo   gen-schema      重新生成 schema/*.json
	@echo   wheel-smoke     构建 wheel、安装进 .venv 并冒烟
	@echo   bench           启动时间基准回归
	@echo   version-check   校验 tag / Cargo / wheel / CHANGELOG 版本一致
	@echo   prek            本地提交钩子全量跑一遍（需安装 prek）
	@echo   push            推送所有 remote 与 tags（不自动打 tag）
	@echo   test-tool       cargo test -p rscmd-tool-$$TOOL
	@echo   cov-tool        llvm-cov -p rscmd-tool-$$TOOL（阈值 95%）
	@echo   py-test-tool    pytest -k $$TOOL

check-docs: ## L0 文档门禁（adr-check + docs-check）
	cargo run -q -p xtask -- check-docs

adr-check: ## 校验 ADR 命名/编号/索引/废弃状态
	cargo run -q -p xtask -- adr-check

docs-check: ## 校验 docs/*.md 生成物是否最新
	cargo run -q -p xtask -- docs-check

check-fast: ## L1 配置/内环门禁
	cargo run -q -p xtask -- check-fast

test: ## 代码测试（不跑覆盖率）
	cargo test --workspace

flow: check-fast test ## L2 代码门禁（check-fast + test）

py-check: ## Python 侧 pytest（需 .venv）
	cargo run -q -p xtask -- py-check

check: ## 全量门禁（check-fast + test + pytest + mypy）
	cargo run -q -p xtask -- check

gen-docs: ## 重新生成 docs/*.md
	cargo run -q -p xtask -- gen-docs

gen-schema: ## 重新生成 schema/*.json
	cargo run -q -p xtask -- gen-schema

cov: ## 覆盖率（llvm-cov，阈值 95%）
	cargo run -q -p xtask -- check-cov

wheel-smoke: ## 构建 wheel、安装进 .venv 并冒烟
	cargo run -q -p xtask -- wheel-smoke

bench: ## 启动时间基准回归
	cargo run -q -p xtask -- bench

version-check: ## 校验 tag / Cargo / wheel / CHANGELOG 版本一致
	cargo run -q -p xtask -- version-check

prek: ## 本地提交钩子全量跑一遍（需安装 prek）
	prek run -a

push: ## 推送所有 remote 与 tags（不自动打 tag）
	cargo run -q -p xtask -- push

test-tool: ## cargo test -p rscmd-tool-$TOOL
	cargo test -p rscmd-tool-$(TOOL)

cov-tool: ## llvm-cov -p rscmd-tool-$TOOL（阈值 95%）
	cargo llvm-cov -p rscmd-tool-$(TOOL) --fail-under-lines 95

py-test-tool: ## pytest -k $TOOL
	python -m pytest tests/py -q -k $(TOOL)
