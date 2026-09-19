PYTHON ?= python
LOCAL_COMPOSE = docker compose --env-file deploy/local/.env.example -f deploy/local/compose.yaml

.PHONY: help fmt check image-smoke local-down local-status local-up test verify

help:
	@echo EdgeAgent engineering command surface
	@echo   make fmt     Check repository and Rust formatting
	@echo   make check   Validate repository, architecture, build, and lint contracts
	@echo   make image-smoke  Build and run every OCI image using Docker
	@echo   make local-up     Start and verify local platform dependencies
	@echo   make local-status Show local platform container status
	@echo   make local-down   Stop local platform containers and preserve data
	@echo   make test    Run Python harness and Rust workspace tests
	@echo   make verify  Run every required local check

fmt:
	$(PYTHON) scripts/verify_repository.py --format-check
	cargo fmt --all --check

check:
	$(PYTHON) scripts/verify_repository.py
	$(PYTHON) scripts/verify_architecture.py
	$(PYTHON) scripts/verify_images.py
	$(PYTHON) scripts/verify_local_stack.py
	cargo check --workspace --all-targets
	cargo clippy --workspace --all-targets -- -D warnings

image-smoke:
	$(PYTHON) scripts/verify_images.py --build

local-up:
	$(PYTHON) scripts/verify_local_stack.py
	$(LOCAL_COMPOSE) config --quiet
	$(LOCAL_COMPOSE) up --detach
	$(PYTHON) scripts/verify_local_stack.py --running

local-status:
	$(LOCAL_COMPOSE) ps

local-down:
	$(LOCAL_COMPOSE) down

test:
	$(PYTHON) -m unittest discover -s tests -p "test_*.py"
	cargo test --workspace --all-targets

verify: fmt check test
