# Airlock Development Makefile
#
# Common commands:
#   make dev      - Start the desktop app in development mode (with hot reload)
#   make build    - Build everything for production
#   make test     - Run all tests
#   make check    - Run clippy and format checks

.PHONY: dev build bundle test check clean install-deps frontend-dev frontend-build frontend-build-force help generate-fixtures design-system-build release-build-macos

# Default target
help:
	@echo "Airlock Development Commands:"
	@echo ""
	@echo "  make dev           Start desktop app in dev mode (hot reload)"
	@echo "  make build         Build everything for production"
	@echo "  make bundle        Build with distributable bundles (DMG, app bundle)"
	@echo "  make test          Run all tests"
	@echo "  make check         Run clippy and format checks"
	@echo "  make clean         Clean build artifacts"
	@echo "  make install-deps  Install frontend dependencies"
	@echo ""
	@echo "  make release-build-macos  Build universal macOS release (local testing)"
	@echo ""
	@echo "  make frontend-dev  Start frontend dev server only"
	@echo "  make frontend-build Build frontend only"
	@echo "  make daemon        Run the daemon"
	@echo "  make cli           Run the CLI (with args: make cli ARGS='status')"
	@echo "  make generate-fixtures  Generate fixtures for frontend mock data"
	@echo ""

# Install frontend dependencies (clean install from lockfile)
install-deps:
	npm ci

# Marker file to track frontend build
FRONTEND_MARKER := crates/airlock-app/dist/.build-marker

# Build frontend assets (only if sources changed)
frontend-build: design-system-build
	@if [ ! -f $(FRONTEND_MARKER) ] || \
	   find crates/airlock-app/src -newer $(FRONTEND_MARKER) 2>/dev/null | grep -q .; then \
		echo "Frontend sources changed, rebuilding..."; \
		cd crates/airlock-app && npm run build && touch dist/.build-marker; \
	else \
		echo "Frontend up to date, skipping build"; \
	fi

# Force rebuild frontend (ignores cache)
frontend-build-force:
	cd crates/airlock-app && npm run build && touch dist/.build-marker

# Start frontend dev server
frontend-dev:
	cd crates/airlock-app && npm run dev

# Start desktop app in development mode (includes frontend hot reload)
# We manage Vite ourselves so the dev server is cleaned up when Tauri exits.
# `npm run dev` spawns a child node process (Vite), so we kill both the npm
# parent and its children to avoid orphaned processes on port 5173.
dev:
	@cd crates/airlock-app && npm run dev & VITE_PID=$$!; \
	trap "pkill -P $$VITE_PID 2>/dev/null; kill $$VITE_PID 2>/dev/null; exit" INT TERM EXIT; \
	sleep 2; \
	cd crates/airlock-app && cargo tauri dev

# Build everything for production
# Note: Tauri uses --features tauri/custom-protocol which causes feature conflicts
# with the regular workspace build. We use separate target dirs to avoid cache thrashing.
build: frontend-build
	cd crates/airlock-app && CARGO_TARGET_DIR=$(CURDIR)/target/tauri cargo tauri build --no-bundle
	cargo build --release --workspace --exclude airlock-app
	@# Copy desktop app to release directory so `airlock` can find it
	cp target/tauri/release/airlock-app target/release/

# Build with distributable bundles (DMG, app bundle)
bundle: frontend-build
	cd crates/airlock-app && CARGO_TARGET_DIR=$(CURDIR)/target/tauri cargo tauri build
	cargo build --release --workspace --exclude airlock-app
	@# Copy desktop app to release directory so `airlock` can find it
	cp target/tauri/release/airlock-app target/release/

# Build debug version (faster compilation)
build-debug: frontend-build
	cargo build --workspace --exclude airlock-app

# Run all tests
test:
	cargo test --workspace --exclude airlock-app

# Run clippy and format checks
check:
	cargo fmt --check
	cargo clippy --workspace --exclude airlock-app -- -D warnings
	npm run lint
	npm run format:check

# Format code
fmt:
	cargo fmt
	npm run format

# Clean all build artifacts
clean:
	cargo clean
	rm -rf target/tauri
	rm -rf crates/airlock-app/dist
	rm -rf crates/airlock-app/node_modules/.vite

# Run the daemon (debug build)
daemon:
	cargo run --bin airlockd

# Run the CLI (debug build)
cli:
	cargo run --bin airlock -- $(ARGS)

# Run the desktop app (requires frontend to be built)
app: frontend-build
	cargo run --bin airlock-app

# Build the design-system package
design-system-build:
	npm run build --workspace @airlock-hq/design-system

# Build universal macOS release locally (for testing the release process)
release-build-macos: frontend-build
	@echo "Building Tauri universal app..."
	cd crates/airlock-app && CARGO_TARGET_DIR=$(CURDIR)/target/tauri cargo tauri build --target universal-apple-darwin --no-bundle
	@echo "Building CLI and daemon (aarch64)..."
	cargo build --release --target aarch64-apple-darwin --workspace --exclude airlock-app
	@echo "Building CLI and daemon (x86_64)..."
	cargo build --release --target x86_64-apple-darwin --workspace --exclude airlock-app
	@echo "Creating universal binaries..."
	mkdir -p target/universal-apple-darwin/release
	lipo -create \
		target/aarch64-apple-darwin/release/airlock \
		target/x86_64-apple-darwin/release/airlock \
		-output target/universal-apple-darwin/release/airlock
	lipo -create \
		target/aarch64-apple-darwin/release/airlockd \
		target/x86_64-apple-darwin/release/airlockd \
		-output target/universal-apple-darwin/release/airlockd
	@echo "Embedding binaries into app bundle..."
	cp target/universal-apple-darwin/release/airlock \
		target/tauri/universal-apple-darwin/release/bundle/macos/Airlock.app/Contents/MacOS/
	cp target/universal-apple-darwin/release/airlockd \
		target/tauri/universal-apple-darwin/release/bundle/macos/Airlock.app/Contents/MacOS/
	@echo "Universal macOS build complete."
	@echo "App bundle: target/tauri/universal-apple-darwin/release/bundle/macos/Airlock.app"

# Generate fixtures for frontend mock data
generate-fixtures:
	cargo run --package airlock-fixtures --bin generate-fixtures
