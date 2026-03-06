.DEFAULT_GOAL := help

# ---- Tool versions ----------------------------------------------------------
BACON_VERSION   := 3.22.0
NEXTEST_VERSION := 0.9.128

# Detect OS for the nextest binary download
UNAME := $(shell uname -s)
ifeq ($(UNAME), Darwin)
    NEXTEST_TARGET := mac
else
    NEXTEST_TARGET := linux
endif

# ---- Phony targets ----------------------------------------------------------
.PHONY: help install install-tools install-hooks

help:
	@echo ""
	@echo "Usage: make <target>"
	@echo ""
	@echo "  install        Install all tools and git hooks (run this after cloning)"
	@echo "  install-tools  Install bacon, cargo-nextest and verify protoc"
	@echo "  install-hooks  Install the pre-commit git hook"
	@echo ""

## install: install all tools and git hooks
install: install-tools install-hooks

## install-tools: install bacon, cargo-nextest and protoc
install-tools:
	@echo ">>> Installing bacon v$(BACON_VERSION)..."
	@if ! bacon --version 2>/dev/null | grep -q "$(BACON_VERSION)"; then \
		cargo install bacon --version $(BACON_VERSION) --locked; \
	else \
		echo "    bacon v$(BACON_VERSION) already installed, skipping."; \
	fi
	@echo ">>> Installing cargo-nextest v$(NEXTEST_VERSION)..."
	@if ! cargo nextest --version 2>/dev/null | grep -q "$(NEXTEST_VERSION)"; then \
		curl -LsSf https://get.nexte.st/$(NEXTEST_VERSION)/$(NEXTEST_TARGET) | tar zxf - -C ~/.cargo/bin; \
	else \
		echo "    cargo-nextest v$(NEXTEST_VERSION) already installed, skipping."; \
	fi
	@echo ">>> Checking protoc..."
	@if ! command -v protoc >/dev/null 2>&1; then \
		echo "    protoc not found. Install it with:"; \
		echo "      macOS:  brew install protobuf"; \
		echo "      Debian: apt-get install protobuf-compiler"; \
		echo "      or download from https://github.com/protocolbuffers/protobuf/releases"; \
		exit 1; \
	else \
		echo "    protoc $$(protoc --version) already installed, skipping."; \
	fi

## install-hooks: install the pre-commit git hook
install-hooks:
	@echo ">>> Installing git hooks..."
	@bash scripts/install-hooks.sh

