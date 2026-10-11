PYTHON ?= python
LOCAL_COMPOSE = docker compose --env-file deploy/local/.env.example -f deploy/local/compose.yaml
SBOM_OUTPUT ?= artifacts/sbom

.PHONY: architecture check fmt help image-smoke local-down local-status local-up repository sbom-images sbom-rust supply-chain test verify

help:
	@echo EdgeAgent engineering command surface
	@echo   make fmt     Check repository and Rust formatting
	@echo   make repository    Enforce text, documentation, and governance contracts
	@echo   make architecture  Enforce manifest and capability dependency boundaries
	@echo   make check   Validate repository, architecture, build, and lint contracts
	@echo   make image-smoke  Build and run every OCI image using Docker
	@echo   make local-up     Start and verify local platform dependencies
	@echo   make local-status Show local platform container status
	@echo   make local-down   Stop local platform containers and preserve data
	@echo   make sbom-rust    Generate one CycloneDX dependency SBOM per Rust binary
	@echo   make sbom-images  Build images and generate their CycloneDX runtime SBOMs
	@echo   make supply-chain Enforce dependency policy and generate all SBOMs
	@echo   make test    Run Python harness and Rust workspace tests
	@echo   make verify  Run every required local check

fmt:
	cargo xtask verify fmt --python "$(PYTHON)"

repository:
	cargo xtask verify repository --python "$(PYTHON)"

architecture:
	cargo xtask verify architecture --python "$(PYTHON)"

check:
	cargo xtask verify check --python "$(PYTHON)"

image-smoke:
	cargo xtask verify image-smoke --python "$(PYTHON)"

local-up:
	$(PYTHON) scripts/verify_local_stack.py
	$(LOCAL_COMPOSE) config --quiet
	$(LOCAL_COMPOSE) up --detach
	$(PYTHON) scripts/verify_local_stack.py --running

local-status:
	$(LOCAL_COMPOSE) ps

local-down:
	$(LOCAL_COMPOSE) down

sbom-rust:
	cargo xtask verify --check sbom-rust --python "$(PYTHON)" --sbom-output "$(SBOM_OUTPUT)"

sbom-images:
	cargo xtask verify --check sbom-images --python "$(PYTHON)" --sbom-output "$(SBOM_OUTPUT)"

supply-chain:
	cargo xtask verify supply-chain --python "$(PYTHON)" --sbom-output "$(SBOM_OUTPUT)"

test:
	cargo xtask verify test --python "$(PYTHON)"

verify:
	cargo xtask verify --python "$(PYTHON)"
