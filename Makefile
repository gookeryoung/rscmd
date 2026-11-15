.PHONY: check-docs check-fast test flow py-check check gen-docs cov wheel-smoke test-tool cov-tool py-test-tool

check-docs:
	cargo run -q -p xtask -- docs-check

check-fast:
	cargo run -q -p xtask -- check-fast

test:
	cargo test --workspace

flow: check-fast test

py-check:
	python -m pytest tests/py -q

check:
	cargo run -q -p xtask -- check

gen-docs:
	cargo run -q -p xtask -- gen-docs

cov:
	cargo run -q -p xtask -- check-cov

wheel-smoke:
	cargo run -q -p xtask -- wheel-smoke

test-tool:
	cargo test -p rscmd-tool-$(TOOL)

cov-tool:
	cargo llvm-cov -p rscmd-tool-$(TOOL) --fail-under-lines 95

py-test-tool:
	python -m pytest tests/py -q -k $(TOOL)
