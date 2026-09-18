PYTHON ?= python

.PHONY: help fmt check test verify

help:
	@echo EdgeAgent engineering command surface
	@echo   make fmt     Check repository text formatting
	@echo   make check   Validate repository policy and documentation links
	@echo   make test    Run harness unit tests
	@echo   make verify  Run every required local check

fmt:
	$(PYTHON) scripts/verify_repository.py --format-check

check:
	$(PYTHON) scripts/verify_repository.py

test:
	$(PYTHON) -m unittest discover -s tests -p "test_*.py"

verify: fmt check test
