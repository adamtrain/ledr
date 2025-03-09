VERSION=0.1
PREFIX?=/usr/local
BINDIR?=$(PREFIX)/bin
MANDIR?=$(PREFIX)/share/man

.PHONY: all build fmt test clean doc install uninstall

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
