# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build Commands

```bash
# Install dependencies
npm install

# Development (with hot reload)
npm run tauri dev

# Development with debug logging
RUST_LOG=debug npm run tauri dev

# Production build
npm run tauri build

# Rust linting
cargo clippy --manifest-path src-tauri/Cargo.toml

# Rust formatting
cargo fmt --manifest-path src-tauri/Cargo.toml
```

## Architecture Overview

This is a **Tauri v2 menubar-only application** that wraps the Filen CLI to provide system tray sync status. There is no frontend UI - the app runs entirely as a system tray icon with a context menu.

### Core Components (src-tauri/src/)

- **lib.rs**: Main app entry point. Handles tray action routing (`handle_tray_action`), spawns the status update loop, manages auto-start logic, and hides from the macOS dock.
- **cli.rs**: `CliManager` - Spawns and monitors the `filen` CLI subprocess. Parses JSON events from `--verbose` mode to track sync state (look for `cycleSuccess`, `cycleError`, `deltasCount`, `transfer` events).
- **state.rs**: `AppState` - Thread-safe shared state using `tokio::sync::RwLock`. Tracks `SyncState` enum and pending file count.
- **config.rs**: Configuration loading/saving from platform-specific JSON files.
- **credentials.rs**: Checks if Filen CLI session exists (doesn't store credentials itself).
- **tray/**: Platform-specific tray implementations
  - **macos.rs**: Uses Tauri's native `TrayIcon` with in-place menu updates
  - **linux.rs**: Uses `ksni` crate for KDE/freedesktop StatusNotifierItem

### Event Flow

1. User clicks menu item → `TrayAction` sent via `mpsc::unbounded_channel`
2. `handle_tray_action` in lib.rs routes to appropriate handler
3. For sync operations, `CliManager::start_sync` spawns `filen --verbose sync` subprocess
4. Stdout is parsed line-by-line for JSON events that update `AppState`
5. `status_update_loop` polls `AppState` every 2 seconds and updates tray via `TrayInterface` trait

### Key Design Decisions

- **No windows**: Uses `set_activation_policy(Accessory)` on macOS to hide from dock
- **CLI dependency**: Requires `@filen/cli` installed globally and authenticated via `filen` interactive session
- **Platform tray abstraction**: `TrayInterface` trait allows macOS (Tauri TrayIcon) and Linux (ksni) implementations

## Configuration

Config stored at:
- **macOS**: `~/Library/Application Support/io.filen.menubar/config.json`
- **Linux**: `~/.config/filen-menubar/config.json`
