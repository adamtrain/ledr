# Ledr

#### Plain text accounting tool

Ledr is a [plain text accounting](https://plaintextaccounting.org) tool,
written in Rust, and designed for complex use cases. It includes a parser, an
importer, a reporting engine, and rock-solid mathematics.

Ledr is a work in progress, but it already has a robust syntax and is ready
for routine use. My primary focus of the project at the moment is to discover
and resolve bugs, make refinements, and write its documentation.

This document will be expanded as the project approaches general release.

## Getting Started

Check out the `tests` directory for a large number of examples of specific
entries and what they can contain. The tests cover substantially all the
syntax of the project. This is only a stopgap recommendation until better
documentation is written.

Compiling Ledr requires a working Rust toolchain. From there, it's as simple
as cloning the repository, running `cargo build --release`, and doing what you
will!

## Contributions

Ledr is a passion project. I use it every day for my own finances and
accounting, and I am currently putting substantial time into the project.

The most important type of contribution you can provide is an example of a
ledger that is being processed incorrectly or unexpectedly, is unintuitive to
you, or is unable to represent a legitimate financial situation that you or
someone is in.

Other than that, I welcome constructive contributions from others, but truly,
other open source projects than this could benefit much more from your time and
energy.

## Roadmap

The current targeted features are all implemented. The remainder of the time
between now and a soft open-source release will be spent on documentation
and refinement.

The completion of this documentation and refinement phase will signal the end
of the first phase of development. I am targeting Q1 2025 for general release.

## Copyright & License

Copyright © 2024-2025 Adam Train <adam@adamtrain.net>

Ledr is licensed under the GPLv3, and will always be free.
