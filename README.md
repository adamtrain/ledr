# Ledr

#### Plain text accounting tool

Ledr is a [plain text accounting](https://plaintextaccounting.org) tool,
written in Rust, and designed for complex use cases. It includes a parser,
a reporting engine, and rock-solid mathematics.

Ledr has a robust syntax and is ready for routine use.

## Documentation

Ledr includes comprehensive man pages:

- **ledr(1)** - Command reference with examples
- **ledr(5)** - Ledger file format specification
- **ledr(7)** - Advanced topics and data integrity guidance

After installation, run `man ledr` to get started. The `tests` directory also
contains examples covering substantially all the syntax.

## Getting Started

A pre-built binary for macOS (arm64) is available on the
[GitHub Releases](https://github.com/adamtrain/ledr/releases) page.

Alternatively, you can compile from source. This requires a working Rust
toolchain. Clone the repository, run `cargo build --release`, and the binary
will be at `target/release/ledr`.

## Contributions

Ledr is feature-complete. I use it every day for my own finances and
accounting. Future development will focus on bug fixes and additional reports
as they are requested or needed.

The most important type of contribution you can provide is an example of a
ledger that is being processed incorrectly or unexpectedly, is unintuitive to
you, or is unable to represent a legitimate financial situation that you or
someone is in.

## Copyright & License

Copyright © 2024-2026 Adam Train <adam@usdocument.org>

Ledr is licensed under the GPLv3, and will always be free.
