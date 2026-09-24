# CLAUDE.md for ledr

## Build & Test Commands
```
cargo build --release  # Build in release mode
cargo test             # Run all tests (unit, integration, CLI snapshots)
cargo test --test cli  # End-to-end CLI tests only
cargo clippy --all-targets -- -D warnings   # Lint, as CI does
cargo fmt              # Format code
make                   # Format, build release, and generate man pages (needs scdoc)
make test              # Format and run all tests
make lint              # Clippy and rustfmt checks
make check-gpl         # Every .rs file must start with the GPL header
make screenshots       # Regenerate docs/*.svg for the README (needs uv)
```

Fancy layouts are snapshot-tested in `tests/cli.rs` against `tests/snapshots/`. After an
intended layout change, run `LEDR_UPDATE_SNAPSHOTS=1 cargo test --test cli` and review the diff.
Plain output is tested by the fixtures in `tests/test_data/`, run by `tests/integration_test.rs`;
plain output is a stable format that scripts may depend on, so change it only deliberately.

## Architecture
- `syntax/lexer.rs` is the single source of truth for ledger syntax. The builder, the formatter
  (`tidy.rs`) and `input/` all work from its `Line`s.
- `parsing/loader.rs` reads files and includes into lexed lines; `parsing/parser.rs` builds the
  `Ledger` from them; `parsing::load_ledger` runs the whole pipeline.
- Report models live in `reports/` along with their plain rendering; fancy rendering is in
  `render/`, built from the primitives in `ui/`. `commands.rs` ties loading and rendering
  together for each command, and `main.rs`/`cli.rs` are a thin shell around it.
- Errors that concern ledger text are `diagnostics::Diagnostic`s with a `Span`, so they can be
  shown with their source lines. Warnings are collected on `Ledger::warnings`, never printed.
- `util/quant.rs` is exact rational arithmetic. Never convert money to floating point except for
  proportions in visual output (`Quant::to_f64`). It panics with `OVERFLOW_MESSAGE` rather than
  wrap; the binary turns that into a readable error.

## Code Style Guidelines
- **Formatting**: 80 column width, hard tabs (4 spaces), trailing commas in match blocks
- **Naming**: snake_case for functions/variables, PascalCase for types/enums
- **Modules**: Organization via mod.rs files with pub mod exports
- **Imports**: Group by source (std lib, crate-local, external)
- **Error Handling**: Use anyhow crate, Result<T, Error> returns, bail! for early returns; use
  `Diagnostic` (with `.help(...)`) for anything a user could fix in their ledger
- **Testing**: Integration tests in tests/ dir, unit tests in #[cfg(test)] modules
- **Safety**: Unsafe code is forbidden (as defined in Cargo.toml)
- **License**: All .rs files must have GPL headers (check with make check-gpl)
