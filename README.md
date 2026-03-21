# Mandelbot

A fractal agent tree for agentic development.

Mandelbot is a terminal emulator that organizes agents into a hierarchical tree. Each node is a sandboxed virtual terminal with its own context, working directory, and prompting — scoped to a layer of work (system, project, change, sub-task). Agents communicate up and down the tree.

## Building

Requires [rustup](https://rustup.rs/). The pinned toolchain will be installed automatically.

```sh
cargo build
```

## Status

Early exploration / hobby project.
