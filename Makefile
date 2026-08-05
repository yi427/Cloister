IMAGE_TAG ?= cloister:dev
IMAGE_CONTEXT := .
IMAGE_FILE := images/rust-node/Containerfile
IMAGE_SCRIPTS := images/rust-node

.PHONY: format format-check verify image image-check install-hooks

format:
	cargo fmt --all

format-check:
	cargo fmt --all --check

verify: format-check image-check
	cargo check --all-targets
	cargo test --all-targets
	cargo clippy --all-targets -- -D warnings

image-check:
	sh -n $(IMAGE_SCRIPTS)/entrypoint.sh
	test -f skills/host-exec/SKILL.md

image:
	container build \
		--arch arm64 \
		--file $(IMAGE_FILE) \
		--progress plain \
		--tag $(IMAGE_TAG) \
		$(IMAGE_CONTEXT)

install-hooks:
	git config --local core.hooksPath .githooks
	@echo "Git hooks enabled from .githooks"
