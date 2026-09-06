MINIMUM_MAKE_VERSION := 4.4.1
MAKE_VERSION_COMPONENTS := $(subst ., ,$(MAKE_VERSION))
MAKE_VERSION_MAJOR := $(word 1,$(MAKE_VERSION_COMPONENTS))
MAKE_VERSION_MINOR := $(or $(word 2,$(MAKE_VERSION_COMPONENTS)),0)
MAKE_VERSION_PATCH := $(or $(word 3,$(MAKE_VERSION_COMPONENTS)),0)
MAKE_VERSION_SUPPORTED := $(or \
	$(intcmp $(MAKE_VERSION_MAJOR),4,,,yes), \
	$(and $(filter 4,$(MAKE_VERSION_MAJOR)),$(intcmp $(MAKE_VERSION_MINOR),4,,,yes)), \
	$(and $(filter 4,$(MAKE_VERSION_MAJOR)),$(filter 4,$(MAKE_VERSION_MINOR)),$(intcmp $(MAKE_VERSION_PATCH),1,,yes,yes)))

ifeq ($(MAKE_VERSION_SUPPORTED),)
$(error GNU Make $(MINIMUM_MAKE_VERSION) or newer is required; found $(MAKE_VERSION))
endif

TEST_FILTER ?=
CARGO_TEST_ARGS ?=
EXTRA_TEST_BINARY_ARGS ?=
NO_CAPTURE ?= false
COVERAGE_THRESHOLD ?= 50

ifneq ($(strip $(NUMBER_OF_PROCESSORS)),)
TEST_THREADS ?= $(shell powershell -NoProfile -NonInteractive -Command "[Math]::Min([Environment]::ProcessorCount, 12)")
else
TEST_THREADS ?= $(shell n=$$(if command -v nproc >/dev/null 2>&1; then nproc; elif command -v getconf >/dev/null 2>&1; then getconf _NPROCESSORS_ONLN; else sysctl -n hw.ncpu; fi); if [ "$$n" -gt 12 ]; then echo 12; else echo "$$n"; fi)
endif

TEST_BINARY_ARGS := $(strip $(if $(filter true,$(NO_CAPTURE)),--nocapture) $(EXTRA_TEST_BINARY_ARGS))
COVERAGE_IGNORE := tests/.*|benches/.*|examples/.*

export GIT_AI_TEST_SHARED_DAEMON_POOL_SIZE := $(TEST_THREADS)

.PHONY: install dev clean build test test-fuzz test-fuzz-all test-fuzz-heavy
.PHONY: test-fuzz-partial test-fuzz-destructive test-fuzz-squash
.PHONY: test-fuzz-combined test-fuzz-workflow test-fuzz-marathon
.PHONY: lint check-windows doc fmt format-check coverage coverage-html
.PHONY: coverage-lcov coverage-check test-bats check

install:
	rustup show active-toolchain
	cargo fetch

dev:
ifeq ($(OS),Windows_NT)
	powershell -NonInteractive -NoProfile -ExecutionPolicy Bypass -File scripts/dev.ps1
else
	./scripts/dev.sh
endif

clean:
	cargo clean

build:
	cargo build

test:
	cargo test $(CARGO_TEST_ARGS) $(TEST_FILTER) -- --test-threads $(TEST_THREADS) $(TEST_BINARY_ARGS)

test-fuzz:
	cargo test $(CARGO_TEST_ARGS) fuzz_standard_seed_ -- --test-threads $(TEST_THREADS) $(TEST_BINARY_ARGS)

test-fuzz-all:
	cargo test $(CARGO_TEST_ARGS) fuzz_ -- --test-threads $(TEST_THREADS) $(TEST_BINARY_ARGS)

test-fuzz-heavy:
	cargo test $(CARGO_TEST_ARGS) fuzz_ -- --test-threads $(TEST_THREADS) $(TEST_BINARY_ARGS) --nocapture

test-fuzz-partial:
	cargo test $(CARGO_TEST_ARGS) fuzz_partial_stage_ -- --test-threads $(TEST_THREADS) $(TEST_BINARY_ARGS)

test-fuzz-destructive:
	cargo test $(CARGO_TEST_ARGS) fuzz_destructive_ -- --test-threads $(TEST_THREADS) $(TEST_BINARY_ARGS)

test-fuzz-squash:
	cargo test $(CARGO_TEST_ARGS) fuzz_squash_ -- --test-threads $(TEST_THREADS) $(TEST_BINARY_ARGS)

test-fuzz-combined:
	cargo test $(CARGO_TEST_ARGS) fuzz_combined_ -- --test-threads $(TEST_THREADS) $(TEST_BINARY_ARGS)

test-fuzz-workflow:
	cargo test $(CARGO_TEST_ARGS) fuzz_workflow_ -- --test-threads $(TEST_THREADS) $(TEST_BINARY_ARGS)

test-fuzz-marathon:
	cargo test $(CARGO_TEST_ARGS) fuzz_marathon_ -- --test-threads $(TEST_THREADS) $(TEST_BINARY_ARGS) --ignored

lint:
	cargo clippy --all-targets -- -D warnings

check-windows:
	cargo check --all-targets --target x86_64-pc-windows-gnu

doc:
	RUSTDOCFLAGS="-D warnings" cargo doc --no-deps

fmt:
	cargo fmt

format-check:
	cargo fmt -- --check

coverage:
	cargo llvm-cov test --ignore-filename-regex='$(COVERAGE_IGNORE)'

coverage-html:
	cargo llvm-cov test --ignore-filename-regex='$(COVERAGE_IGNORE)' --html --open

coverage-lcov:
	cargo llvm-cov test --ignore-filename-regex='$(COVERAGE_IGNORE)' --lcov --output-path lcov.info

coverage-check:
	cargo llvm-cov test --ignore-filename-regex='$(COVERAGE_IGNORE)' --fail-under-lines $(COVERAGE_THRESHOLD)

test-bats:
	bats tests/e2e

check:
	$(MAKE) lint
	$(MAKE) format-check
	$(MAKE) test
