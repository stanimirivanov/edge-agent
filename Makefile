PYTHON ?= python
LOCAL_COMPOSE = docker compose --env-file deploy/local/.env.example -f deploy/local/compose.yaml
SBOM_OUTPUT ?= artifacts/sbom

.PHONY: check fmt help image-smoke local-down local-status local-up sbom-images sbom-rust supply-chain test verify

help:
	@echo EdgeAgent engineering command surface
	@echo   make fmt     Check repository and Rust formatting
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
	$(PYTHON) scripts/verify_repository.py --format-check
	cargo fmt --all --check

check:
	$(PYTHON) scripts/verify_repository.py
	$(PYTHON) scripts/verify_architecture.py
	$(PYTHON) scripts/verify_images.py
	$(PYTHON) scripts/verify_local_stack.py
	$(PYTHON) scripts/verify_supply_chain.py
	$(PYTHON) scripts/verify_release.py
	cargo metadata --locked --offline --format-version 1 --no-deps
	cargo check --locked --workspace --all-targets
	cargo clippy --locked --workspace --all-targets -- -D warnings

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

sbom-rust:
	$(PYTHON) scripts/generate_sboms.py rust --output $(SBOM_OUTPUT)

sbom-images:
	$(PYTHON) scripts/generate_sboms.py images --output $(SBOM_OUTPUT)

supply-chain:
	cargo deny --locked check
	$(MAKE) sbom-rust SBOM_OUTPUT=$(SBOM_OUTPUT) PYTHON=$(PYTHON)
	$(MAKE) sbom-images SBOM_OUTPUT=$(SBOM_OUTPUT) PYTHON=$(PYTHON)
	$(PYTHON) scripts/verify_supply_chain.py --artifacts $(SBOM_OUTPUT)

test:
	$(PYTHON) -m unittest discover -s tests -p "test_*.py"
	cargo test --locked --workspace --all-targets

verify: fmt check test
