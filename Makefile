PYTHON ?= python

.PHONY: help fmt check test verify

help:
	@echo EdgeAgent engineering command surface
	@echo   make fmt     Check repository and Rust formatting
	@echo   make check   Validate repository, architecture, build, and lint contracts
	@echo   make test    Run Python harness and Rust workspace tests
	@echo   make verify  Run every required local check

fmt:
	$(PYTHON) scripts/verify_repository.py --format-check
	cargo fmt --all --check

check:
	$(PYTHON) scripts/verify_repository.py
	$(PYTHON) scripts/verify_architecture.py
	cargo check --workspace --all-targets
	cargo clippy --workspace --all-targets -- -D warnings

test:
	$(PYTHON) -m unittest discover -s tests -p "test_*.py"
	cargo test --workspace --all-targets

verify: fmt check test
