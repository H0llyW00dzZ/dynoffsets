.PHONY: all header help \
        build build-release check \
        test test-default test-all test-no-default \
        fmt fmt-check \
        clippy clippy-fix \
        doc doc-open \
        coverage coverage-html \
        bench audit \
        verify publish-dry \
        clean clean-all

# Default target — print the help screen instead of silently building.
.DEFAULT_GOAL := help

# ---------------------------------------------------------------------------
# Banner
# ---------------------------------------------------------------------------
ifeq ($(OS),Windows_NT)
header:
	@echo.
	@echo      _                   __  __          _       
	@echo   __^| ^|_   _ _ __   ___ / _^|/ _^|___  ___^| ^|_ ___ 
	@echo  / _` ^| ^| ^| ^| '_ \ / _ \ ^|_^| ^|_/ __^|/ _ \ __/ __^|
	@echo ^| ^(_^| ^| ^|_^| ^| ^| ^| ^| ^(_^) ^|  _^|  _\__ \  __/ ^|_\__ \ 
	@echo  \__,_^|\__, ^|_^| ^|_^|\___/^|_^| ^|_^| ^|___/\___^|\__^|___/
	@echo        ^|___/                                     
	@echo.
	@echo   dynoffsets by H0llyW00dzZ ^(@github.com/H0llyW00dzZ^)
	@echo.
else
header:
	@printf '%s\n' ''
	@printf '%s\n' '     _                   __  __          _       '
	@printf '%s\n' '  __| |_   _ _ __   ___ / _|/ _|___  ___| |_ ___ '
	@printf '%s\n' ' / _` | | | | '"'"'_ \ / _ \ |_| |_/ __|/ _ \ __/ __|'
	@printf '%s\n' '| (_| | |_| | | | | (_) |  _|  _\__ \  __/ |_\__ \ '
	@printf '%s\n' ' \__,_|\__, |_| |_|\___/|_| |_| |___/\___|\__|___/'
	@printf '%s\n' '       |___/                                     '
	@printf '%s\n' ''
	@printf '%s\n' '  dynoffsets by H0llyW00dzZ (@github.com/H0llyW00dzZ) '
	@printf '%s\n' ''
endif

# ---------------------------------------------------------------------------
# Help — list available targets.
# ---------------------------------------------------------------------------
help: header
	@echo Usage: make ^<target^>
	@echo.
	@echo Build:
	@echo   build              Debug build
	@echo   build-release      Release build
	@echo   check              Quick type-check across all features and targets
	@echo.
	@echo Test:
	@echo   test               Run tests with all features
	@echo   test-default       Run tests with default features
	@echo   test-no-default    Run tests with --no-default-features
	@echo   test-all           Run all three test variants
	@echo.
	@echo Lint and format:
	@echo   fmt                Apply rustfmt to the workspace
	@echo   fmt-check          Verify rustfmt is clean (CI gate)
	@echo   clippy             Run clippy with -D warnings on all features and targets
	@echo   clippy-fix         Apply clippy auto-fixes
	@echo.
	@echo Docs and coverage:
	@echo   doc                Build rustdoc for the local crate
	@echo   doc-open           Build and open rustdoc
	@echo   coverage           Print line/region coverage summary
	@echo   coverage-html      Generate HTML coverage report
	@echo.
	@echo Benchmarks and audit:
	@echo   bench              Run criterion benchmarks
	@echo   audit              cargo-audit security check (requires cargo-audit)
	@echo.
	@echo Composite:
	@echo   verify             fmt-check + clippy + test-all  (full local CI gate)
	@echo   publish-dry        cargo publish --dry-run
	@echo   all                header + build + test
	@echo.
	@echo Cleanup:
	@echo   clean              cargo clean
	@echo   clean-all          cargo clean + remove benchmark artifacts

# ---------------------------------------------------------------------------
# Build
# ---------------------------------------------------------------------------
all: header build test

build: header
	@echo :: Building debug profile...
	cargo build --all-features
	@echo :: Done.

build-release: header
	@echo :: Building release profile...
	cargo build --release --all-features
	@echo :: Done.

check: header
	@echo :: Running cargo check...
	cargo check --all-features --all-targets
	@echo :: Done.

# ---------------------------------------------------------------------------
# Test
# ---------------------------------------------------------------------------
test: header
	@echo :: Running tests (all features)...
	cargo test --all-features
	@echo :: Done.

test-default: header
	@echo :: Running tests (default features)...
	cargo test
	@echo :: Done.

test-no-default: header
	@echo :: Running tests (--no-default-features)...
	cargo test --no-default-features
	@echo :: Done.

test-all: header test-default test-no-default test

# ---------------------------------------------------------------------------
# Format
# ---------------------------------------------------------------------------
fmt: header
	@echo :: Applying rustfmt...
	cargo fmt --all
	@echo :: Done.

fmt-check: header
	@echo :: Checking rustfmt...
	cargo fmt --all -- --check
	@echo :: Done.

# ---------------------------------------------------------------------------
# Clippy
# ---------------------------------------------------------------------------
clippy: header
	@echo :: Running clippy...
	cargo clippy --all-features --all-targets -- -D warnings
	@echo :: Done.

clippy-fix: header
	@echo :: Applying clippy fixes...
	cargo clippy --all-features --all-targets --fix --allow-dirty --allow-staged
	@echo :: Done.

# ---------------------------------------------------------------------------
# Docs
# ---------------------------------------------------------------------------
doc: header
	@echo :: Building rustdoc...
	cargo doc --no-deps --all-features
	@echo :: Done.

doc-open: header
	@echo :: Building and opening rustdoc...
	cargo doc --no-deps --all-features --open
	@echo :: Done.

# ---------------------------------------------------------------------------
# Coverage
# ---------------------------------------------------------------------------
coverage: header
	@echo :: Running cargo llvm-cov (summary)...
	cargo llvm-cov --all-features --workspace --summary-only
	@echo :: Done.

coverage-html: header
	@echo :: Generating HTML coverage report...
	cargo llvm-cov --all-features --workspace --html
	@echo :: Done. Open target/llvm-cov/html/index.html

# ---------------------------------------------------------------------------
# Benchmarks
# ---------------------------------------------------------------------------
bench: header
	@echo :: Running benchmarks...
	cargo bench --all-features
	@echo :: Done.

# ---------------------------------------------------------------------------
# Security audit
# ---------------------------------------------------------------------------
audit: header
	@echo :: Running cargo-audit...
	cargo audit
	@echo :: Done.

# ---------------------------------------------------------------------------
# Composite gates
# ---------------------------------------------------------------------------
# Full local CI gate. Run this before opening a PR.
verify: header fmt-check clippy test-all
	@echo :: Verify passed.

# Pre-publish smoke test — tells you exactly what crates.io will receive.
publish-dry: header verify
	@echo :: Running cargo publish --dry-run...
	cargo publish --dry-run --all-features
	@echo :: Done.

# ---------------------------------------------------------------------------
# Clean
# ---------------------------------------------------------------------------
clean: header
	@echo :: Cleaning cargo target...
	cargo clean
	@echo :: Done.

clean-all: header clean
	@echo :: Removing criterion artifacts...
ifeq ($(OS),Windows_NT)
	-@if exist target\criterion rmdir /s /q target\criterion
else
	-@rm -rf target/criterion
endif
	@echo :: Done.
