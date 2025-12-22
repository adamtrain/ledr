# Ledr

#### Plain text accounting tool

Ledr is a [plain text accounting](https://plaintextaccounting.org) tool,
written in Rust, and designed for complex use cases. It includes a parser, an
importer, a reporting engine, and rock-solid mathematics.

Ledr has a robust syntax and is ready for routine use.

## Getting Started

Check out the `tests` directory for a large number of examples of specific
entries and what they can contain. The tests cover substantially all the
syntax of the project. This is only a stopgap recommendation until better
documentation is written.

Compiling Ledr requires a working Rust toolchain. From there, it's as simple
as cloning the repository, running `cargo build --release`, and doing what you
will!

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
