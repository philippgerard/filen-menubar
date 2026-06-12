# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Workflow

- **Work directly on `main` branch** - no feature branches needed for this project
- Commit and push changes directly to main

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

# Production build with signing and notarization (macOS)
source .env && APPLE_SIGNING_IDENTITY="Developer ID Application: Philipp Gérard (7WS855G6D3)" npm run tauri build

# Rust linting
cargo clippy --manifest-path src-tauri/Cargo.toml

# Rust formatting
cargo fmt --manifest-path src-tauri/Cargo.toml

# Rust tests (unit + integration)
cargo test --manifest-path src-tauri/Cargo.toml
```

## Architecture Overview

This is a **Tauri v2 menubar-only application** that wraps the Filen CLI to provide system tray sync status. There is no frontend UI - the app runs entirely as a system tray icon with a context menu.

### Module Structure (src-tauri/src/)

```
src-tauri/src/
├── lib.rs          # App entry point, status loop, tray action dispatch
├── actions.rs      # Tray action handlers (command pattern)
├── cli/
│   ├── mod.rs      # CliManager - subprocess lifecycle
│   ├── events.rs   # CLI JSON event parsing
│   ├── framer.rs   # Multi-line JSON framing of CLI stdout
│   ├── network.rs  # Network error detection
│   ├── discovery.rs # CLI binary discovery
│   └── process.rs  # ProcessRunner trait (for testability)
├── config.rs       # Configuration with SyncMode/LogLevel enums
├── credentials.rs  # CLI session detection
├── error.rs        # Unified error types
├── logging.rs      # File logging setup
├── state.rs        # AppState with watch channel for reactive updates
└── tray/
    ├── mod.rs      # TrayInterface trait, shared helpers
    ├── macos.rs    # Tauri TrayIcon implementation
    └── linux.rs    # ksni StatusNotifierItem implementation
```

### Core Components

- **lib.rs**: Main app entry point. Dispatches tray actions to `actions.rs`, runs the reactive status update loop, manages auto-start logic, and hides from the macOS dock.
- **actions.rs**: Individual handler functions for each tray menu action (command pattern). Uses `ActionContext` to group dependencies.
- **cli/**: CLI subprocess management
  - **mod.rs**: `CliManager` - Spawns and monitors `filen --verbose sync`
  - **events.rs**: Parses JSON events (`cycleSuccess`, `deltasCount`, `transfer`, etc.)
  - **framer.rs**: Accumulates the CLI's pretty-printed multi-line JSON into complete objects (string-literal-aware brace tracking, capped buffer)
  - **network.rs**: Detects network errors for offline state
  - **process.rs**: `ProcessRunner` trait for dependency injection (enables mocking)
- **state.rs**: `AppState` with `tokio::sync::watch` channel for reactive UI updates. Includes state machine validation for `SyncState` transitions.
- **config.rs**: Configuration with type-safe `SyncMode` and `LogLevel` enums.
- **error.rs**: Unified `AppError`, `CliError`, `ConfigError`, `CredentialError` types.
- **tray/**: Platform-specific implementations behind `TrayInterface` trait

### Event Flow

```
User clicks menu     CLI emits JSON      State changes
      │                    │                  │
      ▼                    ▼                  ▼
  TrayAction ──────► CliManager ──────► AppState
      │              (parse events)     (watch channel)
      ▼                                       │
  actions.rs                                  ▼
  (handlers)                            status_update_loop
                                        (reactive + timer)
                                              │
                                              ▼
                                        TrayInterface
                                        (update icon/menu)
```

1. User clicks menu item → `TrayAction` sent via `mpsc::unbounded_channel`
2. `handle_tray_action` dispatches to handler in `actions.rs`
3. For sync: `CliManager::start_sync` spawns `filen --verbose sync`
4. CLI stdout parsed for JSON events → `AppState` updated
5. Watch channel notifies `status_update_loop` → tray updated immediately
6. Animation timer (500ms) updates icon frames independently

### Key Design Decisions

- **No windows**: Uses `set_activation_policy(Accessory)` on macOS to hide from dock
- **CLI dependency**: Requires `@filen/cli` installed globally and authenticated via `filen` interactive session
- **Platform tray abstraction**: `TrayInterface` trait allows macOS (Tauri TrayIcon) and Linux (ksni) implementations
- **Platform parity**: macOS and Linux must have identical menu labels, functionality, and behavior. When updating one platform's tray menu, always update the other to match.

### Linux ksni Caveats

The Linux tray uses the `ksni` crate for D-Bus StatusNotifierItem support. Important notes:

- **ksni's `Handle::update()` is async**: You MUST await the update call for D-Bus signals to be emitted. The `TrayInterface` trait methods are sync, so updates are spawned on Tauri's async runtime via `trigger_update()`.
- **State is stored externally**: The `FilenTray` struct holds an `Arc<RwLock<LinuxTrayState>>` that's shared with `LinuxTray`. The `Tray::menu()` method reads from this shared state to build the menu.
- **Empty closure is intentional**: `handle.update(|_| {}).await` uses an empty closure because we update the shared state before calling update. The closure receives `&mut FilenTray`, but our state is in the external `Arc<RwLock>`.

## Versioning

Keep versions in sync across all three files:
- `package.json` - npm version
- `src-tauri/tauri.conf.json` - Tauri app version
- `src-tauri/Cargo.toml` - Rust crate version

The version is displayed in the About dialog via `env!("CARGO_PKG_VERSION")`.

## macOS Notarization

To build a signed and notarized release, the following environment variables are required:

- `APPLE_SIGNING_IDENTITY` - The signing certificate name (set inline in the build command)
- `APPLE_ID` - Apple Developer account email
- `APPLE_PASSWORD` - App-specific password (generate at appleid.apple.com)
- `APPLE_TEAM_ID` - Apple Developer Team ID

The credentials (except signing identity) are stored in `.env` in the project root. This file is gitignored.

Example `.env`:
```
APPLE_ID=your@email.com
APPLE_PASSWORD=xxxx-xxxx-xxxx-xxxx
APPLE_TEAM_ID=XXXXXXXXXX
```

## Configuration

Config stored at:
- **macOS**: `~/Library/Application Support/io.filen.menubar/config.json`
- **Linux**: `~/.config/filen-menubar/config.json`
