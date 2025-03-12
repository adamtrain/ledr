# CLAUDE.md for ledr

## Build & Test Commands
```
cargo build --release  # Build in release mode
cargo test             # Run all tests
cargo test -- --test-threads=1   # Run tests sequentially
cargo test test_integration_standard   # Run specific test
cargo clippy           # Run linter
cargo fmt              # Format code
make                   # Format, build release, and generate docs
make test              # Format and run all tests
```

## Code Style Guidelines
- **Formatting**: 80 column width, hard tabs (4 spaces), trailing commas in match blocks
- **Naming**: snake_case for functions/variables, PascalCase for types/enums
- **Modules**: Organization via mod.rs files with pub mod exports
- **Imports**: Group by source (std lib, crate-local, external)
- **Error Handling**: Use anyhow crate, Result<T, Error> returns, bail! for early returns
- **Testing**: Integration tests in tests/ dir, unit tests in #[cfg(test)] modules
- **Safety**: Unsafe code is forbidden (as defined in Cargo.toml)
- **License**: All .rs files must have GPL headers (check with make check-gpl)