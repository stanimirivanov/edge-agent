PYTHON ?= python

.PHONY: help fmt check image-smoke test verify

help:
	@echo EdgeAgent engineering command surface
	@echo   make fmt     Check repository and Rust formatting
	@echo   make check   Validate repository, architecture, build, and lint contracts
	@echo   make image-smoke  Build and run every OCI image using Docker
	@echo   make test    Run Python harness and Rust workspace tests
	@echo   make verify  Run every required local check

fmt:
	$(PYTHON) scripts/verify_repository.py --format-check
	cargo fmt --all --check

check:
	$(PYTHON) scripts/verify_repository.py
	$(PYTHON) scripts/verify_architecture.py
	$(PYTHON) scripts/verify_images.py
	cargo check --workspace --all-targets
	cargo clippy --workspace --all-targets -- -D warnings

image-smoke:
	$(PYTHON) scripts/verify_images.py --build

test:
	$(PYTHON) -m unittest discover -s tests -p "test_*.py"
	cargo test --workspace --all-targets

verify: fmt check test
