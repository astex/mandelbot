# Mandelbot

A fractal agent tree for agentic development.

Mandelbot is a terminal emulator that organizes agents into a hierarchical tree. Each node is a sandboxed virtual terminal with its own context, working directory, and prompting — scoped to a layer of work (system, project, change, sub-task). Agents communicate up and down the tree.

## Installing

### macOS (Homebrew)

```sh
brew tap astex/mandelbot
brew install --cask mandelbot
```

### Linux / macOS (shell script)

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/astex/mandelbot/releases/latest/download/mandelbot-installer.sh | sh
```

Linux users need `libfontconfig1` and `libfreetype6` installed.

## Building from source

Requires [rustup](https://rustup.rs/). The pinned toolchain will be installed automatically.

```sh
cargo build
```

## Status

Early exploration / hobby project.
