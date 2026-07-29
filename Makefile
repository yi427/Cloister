.PHONY: format format-check verify install-hooks

format:
	cargo fmt --all

format-check:
	cargo fmt --all --check

verify: format-check
	cargo check --all-targets
	cargo test --all-targets
	cargo clippy --all-targets -- -D warnings

install-hooks:
	git config --local core.hooksPath .githooks
	@echo "Git hooks enabled from .githooks"
