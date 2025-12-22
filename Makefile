VERSION=0.1
PREFIX?=/usr/local
BINDIR?=$(PREFIX)/bin
MANDIR?=$(PREFIX)/share/man

.PHONY: all build fmt test clean doc install uninstall release

all: fmt build doc

build:
	cargo build --release

fmt:
	cargo fmt

test: fmt
	cargo test -- --test-threads=1

clean: fmt
	cargo clean

doc:
	mkdir -p target
	scdoc < doc/ledr.1.scd > target/ledr.1
	scdoc < doc/ledr.5.scd > target/ledr.5

install:
	mkdir -p /usr/local/bin
	mkdir -p /usr/local/share/man/man1
	mkdir -p /usr/local/share/man/man5
	install -m 755 target/release/ledr $(DESTDIR)$(BINDIR)/ledr
	install -m 644 target/ledr.1 $(DESTDIR)$(MANDIR)/man1/ledr.1
	install -m 644 target/ledr.5 $(DESTDIR)$(MANDIR)/man5/ledr.5

uninstall:
	rm -f $(DESTDIR)$(BINDIR)/ledr
	rm -f $(DESTDIR)$(MANDIR)/man1/ledr.1

# Reports .rs files that do not have a GPLv3 header
check-gpl:
	@violations=$$(find . -name '*.rs' -exec sh -c 'head -n 1 "{}" | grep -q "©" || echo "{}"' \;); \
	if [ -n "$$violations" ]; then \
		echo "$$violations"; \
		exit 1; \
	fi
	@echo "All good"

# Create a GitHub release with cross-compiled binaries
# Usage: make release TAG=v0.7.22
# Prerequisites:
#   - gh CLI (authenticated)
#   - cargo-zigbuild (cargo install cargo-zigbuild)
#   - zig (brew install zig)
#   - rustup target add aarch64-apple-darwin x86_64-unknown-linux-gnu
release:
ifndef TAG
	$(error TAG is required. Usage: make release TAG=v0.7.22)
endif
	@if ! echo "$(TAG)" | grep -qE '^v[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?(\+[a-zA-Z0-9.]+)?$$'; then \
		echo "Error: TAG must be lowercase 'v' followed by valid semver (e.g., v1.2.3)"; \
		exit 1; \
	fi
	@echo "Checking git state..."
	@git fetch origin main
	@if [ "$$(git rev-parse --abbrev-ref HEAD)" != "main" ]; then \
		echo "Error: Must be on main branch"; \
		exit 1; \
	fi
	@if [ -n "$$(git status --porcelain)" ]; then \
		echo "Error: Working directory has uncommitted changes"; \
		exit 1; \
	fi
	@if [ "$$(git rev-parse HEAD)" != "$$(git rev-parse origin/main)" ]; then \
		echo "Error: Local main is not in sync with origin/main"; \
		exit 1; \
	fi
	@echo "Running tests..."
	$(MAKE) test
	@echo "Building documentation..."
	$(MAKE) doc
	@echo "Building macOS arm64 binary..."
	cargo zigbuild --release --target aarch64-apple-darwin
	@echo "Building Linux amd64 binary..."
	cargo zigbuild --release --target x86_64-unknown-linux-gnu
	@echo "Packaging artifacts..."
	mkdir -p target/release-pkg
	tar -czvf target/release-pkg/ledr-$(TAG)-darwin-arm64.tar.gz \
		-C target/aarch64-apple-darwin/release ledr \
		-C $(CURDIR)/target ledr.1 ledr.5
	tar -czvf target/release-pkg/ledr-$(TAG)-linux-amd64.tar.gz \
		-C target/x86_64-unknown-linux-gnu/release ledr \
		-C $(CURDIR)/target ledr.1 ledr.5
	@echo "Creating GitHub release..."
	git tag $(TAG)
	git push origin $(TAG)
	gh release create $(TAG) --generate-notes \
		target/release-pkg/ledr-$(TAG)-darwin-arm64.tar.gz \
		target/release-pkg/ledr-$(TAG)-linux-amd64.tar.gz
	@echo "Release $(TAG) complete!"
