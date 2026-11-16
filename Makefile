# Makefile - rscmd project shortcuts
# Run `make help` to list all targets.
#
# Target names mirror fcmd (semantic mapping lives in AGENTS.md):
#   check-docs / check-fast / flow / check / cov / test / push
# Rust equivalents: ruff+pyrefly -> cargo fmt+clippy; pytest -> cargo test;
#                  pytest --cov -> cargo llvm-cov; bump-my-version -> manual version + CHANGELOG.

TOOL ?=

.PHONY: help check-docs adr-check docs-check check-fast test flow py-check check \
        gen-docs gen-schema cov wheel-smoke bench version-check prek push \
        test-tool cov-tool py-test-tool

help: ## List available targets
	@echo   check-docs      L0 docs gate (adr-check + generated-artifact docs-check)
	@echo   check-fast      L1 config/inner-loop gate (fmt + clippy + registry/arch/docs/schema/deny/machete)
	@echo   flow            L2 code gate (check-fast + test)
	@echo   check           Full gate (check-fast + test + pytest + mypy)
	@echo   test            Code tests (cargo test, no coverage)
	@echo   cov             Coverage (llvm-cov, 95 percent line threshold, excludes rscmd-py / xtask)
	@echo   py-check        Python-side pytest (requires .venv)
	@echo   adr-check       Validate ADR naming/numbering/index/deprecation state
	@echo   docs-check      Validate that docs/*.md generated artifacts are up to date
	@echo   gen-docs        Regenerate docs/*.md
	@echo   gen-schema      Regenerate schema/*.json
	@echo   wheel-smoke     Build the wheel, install into .venv and smoke test
	@echo   bench           Startup-time benchmark regression
	@echo   version-check   Validate tag / Cargo / wheel / CHANGELOG version consistency
	@echo   prek            Run all local commit hooks once (requires prek)
	@echo   push            Push all remotes and tags (never creates tags)
	@echo   test-tool       cargo test -p rscmd-tool-$$TOOL
	@echo   cov-tool        llvm-cov -p rscmd-tool-$$TOOL (95 percent line threshold)
	@echo   py-test-tool    pytest -k $$TOOL

check-docs: ## L0 docs gate (adr-check + docs-check)
	cargo run -q -p xtask -- check-docs

adr-check: ## Validate ADR naming/numbering/index/deprecation state
	cargo run -q -p xtask -- adr-check

docs-check: ## Validate that docs/*.md generated artifacts are up to date
	cargo run -q -p xtask -- docs-check

check-fast: ## L1 config/inner-loop gate
	cargo run -q -p xtask -- check-fast

test: ## Code tests (no coverage)
	cargo test --workspace

flow: check-fast test ## L2 code gate (check-fast + test)

py-check: ## Python-side pytest (requires .venv)
	cargo run -q -p xtask -- py-check

check: ## Full gate (check-fast + test + pytest + mypy)
	cargo run -q -p xtask -- check

gen-docs: ## Regenerate docs/*.md
	cargo run -q -p xtask -- gen-docs

gen-schema: ## Regenerate schema/*.json
	cargo run -q -p xtask -- gen-schema

cov: ## Coverage (llvm-cov, 95% threshold)
	cargo run -q -p xtask -- check-cov

wheel-smoke: ## Build the wheel, install into .venv and smoke test
	cargo run -q -p xtask -- wheel-smoke

bench: ## Startup-time benchmark regression
	cargo run -q -p xtask -- bench

version-check: ## Validate tag / Cargo / wheel / CHANGELOG version consistency
	cargo run -q -p xtask -- version-check

prek: ## Run all local commit hooks once (requires prek)
	prek run -a

push: ## Push all remotes and tags (never creates tags)
	cargo run -q -p xtask -- push

test-tool: ## cargo test -p rscmd-tool-$TOOL
	cargo test -p rscmd-tool-$(TOOL)

cov-tool: ## llvm-cov -p rscmd-tool-$TOOL (95% threshold)
	cargo llvm-cov -p rscmd-tool-$(TOOL) --fail-under-lines 95

py-test-tool: ## pytest -k $TOOL
	python -m pytest tests/py -q -k $(TOOL)
