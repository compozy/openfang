SHELL := /bin/bash

.PHONY: help fmt fmt-check lint lint-clippy lint-fix check build test coverage clean verify

.DEFAULT_GOAL := help

##@ Formatting

fmt: ## Format Rust code
	cargo fmt --all -- --color always

fmt-check: ## Check Rust formatting
	cargo fmt --all --check -- --color always

##@ Linting

lint: fmt-check lint-clippy ## Run all lints (fmt check + clippy)

lint-clippy: ## Run clippy lints
	cargo clippy --all-targets --all-features -- -D warnings

lint-fix: ## Auto-fix clippy warnings where possible
	cargo clippy --fix --allow-dirty --allow-staged --all-targets --all-features -- -D warnings

##@ Building

check: ## Type-check without producing binaries
	cargo check --all-targets --all-features

build: ## Build in release mode
	cargo build --release

##@ Testing

test: ## Run all tests
	cargo test --all-features

coverage: ## Print code coverage summary
	cargo llvm-cov --all-features --summary-only

##@ Verification

verify: fmt lint test ## Run fmt + lint + test (full verification)

##@ Cleanup

clean: ## Clean build artifacts
	cargo clean

##@ Help

help: ## Show this help
	@awk 'BEGIN {FS = ":.*##"; printf "\nUsage:\n  make \033[36m<target>\033[0m\n"} /^[a-zA-Z_-]+:.*?##/ { printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2 } /^##@/ { printf "\n\033[1m%s\033[0m\n", substr($$0, 5) } ' $(MAKEFILE_LIST)
