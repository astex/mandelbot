# Squeak

Terminal emulator and IDE-like environment for agentic development.

## Project Principles

- Minimal opinionated configuration — sensible defaults, few knobs
- Effortless theming — pick one and go
- Agentic development first

## Tech Stack

- Rust (edition 2024, toolchain pinned in rust-toolchain.toml)
- Qt UI via cxx-qt
- Built/managed with cargo

## Style

- Import order: stdlib, then external crates, then local modules — separated by blank lines

## Development

```sh
cargo build    # build
cargo run      # run
cargo test     # test
```
