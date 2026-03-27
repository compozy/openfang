SHELL := /bin/bash

.PHONY: help fmt fmt-check lint lint-clippy lint-fix lint-fast lint-package check check-fast check-full check-package build build-fast test test-fast test-package coverage clean verify doctor-compile

.DEFAULT_GOAL := help

##@ Formatting

fmt: ## Format Rust code
	cargo fmt --all -- --color always

fmt-check: ## Check Rust formatting
	cargo fmt --all --check -- --color always

##@ Linting

lint: fmt-check lint-clippy ## Run all lints (fmt check + clippy)

lint-clippy: ## Run clippy lints
	cargo clippy --workspace --all-targets --all-features -- -D warnings

lint-fast: ## Run clippy on the default development surface
	cargo clippy --all-targets -- -D warnings

lint-package: ## Run clippy for a single package (make lint-package PKG=openfang-kernel)
	@test -n "$(PKG)" || (echo "Usage: make lint-package PKG=<workspace-package>" >&2; exit 1)
	cargo clippy -p "$(PKG)" --all-targets -- -D warnings

lint-fix: ## Auto-fix clippy warnings where possible
	cargo clippy --workspace --fix --allow-dirty --allow-staged --all-targets --all-features -- -D warnings

##@ Building

check: check-fast ## Type-check the default development surface

check-fast: ## Type-check default-members without optional features
	cargo check --all-targets

check-full: ## Type-check the full workspace with every feature enabled
	cargo check --workspace --all-targets --all-features

check-package: ## Type-check a single package (make check-package PKG=openfang-kernel)
	@test -n "$(PKG)" || (echo "Usage: make check-package PKG=<workspace-package>" >&2; exit 1)
	cargo check -p "$(PKG)" --all-targets

build: ## Build in release mode
	cargo build --workspace --release

build-fast: ## Build the main CLI with the faster release profile
	cargo build --profile release-fast -p openfang-cli

##@ Testing

test: ## Run all tests
	cargo test --workspace --all-features

test-fast: ## Run unit and integration tests for the default development surface
	cargo test --lib --bins --tests --no-fail-fast

test-package: ## Run unit and integration tests for a single package (make test-package PKG=openfang-kernel)
	@test -n "$(PKG)" || (echo "Usage: make test-package PKG=<workspace-package>" >&2; exit 1)
	cargo test -p "$(PKG)" --lib --bins --tests --no-fail-fast

coverage: ## Print code coverage summary
	cargo llvm-cov --workspace --all-features --summary-only

##@ Verification

verify: fmt lint test ## Run fmt + lint + test (full verification)

##@ Cleanup

clean: ## Clean build artifacts
	cargo clean

doctor-compile: ## Diagnose Cargo compile-time blockers in the local environment
	./scripts/doctor-compile.sh

##@ Help

help: ## Show this help
	@awk 'BEGIN {FS = ":.*##"; printf "\nUsage:\n  make \033[36m<target>\033[0m\n"} /^[a-zA-Z_-]+:.*?##/ { printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2 } /^##@/ { printf "\n\033[1m%s\033[0m\n", substr($$0, 5) } ' $(MAKEFILE_LIST)
