# Filen Menubar

A lightweight, native menubar/system tray application for [Filen.io](https://filen.io) cloud sync on macOS and Linux.

## Features

- **Menubar-only interface** - No windows, just a clean system tray icon
- **Real-time sync status** - Shows current sync state with live file count updates
- **Live menu updates** - Menu items update in-place without closing the menu
- **Cross-platform** - macOS and Linux support
- **Native KDE support** - Uses StatusNotifierItem (SNI) via ksni for first-class KDE integration
- **Auto-sync** - Optionally start syncing on launch
- **Logout confirmation** - Prevents accidental logout with a confirmation dialog

## Requirements

### Filen CLI

This app wraps the [Filen CLI](https://github.com/FilenCloudDienste/filen-cli). Install it first:

```bash
npm install -g @filen/cli
```

Verify installation:

```bash
filen --version
```

**Important:** You must login to the Filen CLI before using this app:

```bash
filen login
```

### Build Dependencies

- [Rust](https://rustup.rs/) (latest stable)
- [Node.js](https://nodejs.org/) (v18+)
- [Tauri CLI](https://tauri.app/)

**macOS:**
```bash
xcode-select --install
```

**Linux (Debian/Ubuntu):**
```bash
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

**Linux (Fedora):**
```bash
sudo dnf install webkit2gtk4.1-devel openssl-devel curl wget file \
  libxdo-devel libappindicator-gtk3-devel librsvg2-devel
```

**Linux (Arch):**
```bash
sudo pacman -S webkit2gtk-4.1 base-devel curl wget file openssl libxdo \
  libappindicator-gtk3 librsvg
```

## Installation

```bash
# Clone the repository
git clone https://github.com/yourusername/filen-menubar.git
cd filen-menubar

# Install dependencies
npm install

# Install Tauri CLI
cargo install tauri-cli --version "^2" --locked

# Run in development mode
npm run tauri dev

# Build for production
npm run tauri build
```

## Configuration

Configuration is stored in a JSON file:

- **macOS:** `~/Library/Application Support/io.filen.menubar/config.json`
- **Linux:** `~/.config/filen-menubar/config.json`

### Options

```json
{
  "local_path": "~/Filen",
  "remote_path": "/",
  "sync_mode": "twoWay",
  "auto_start": true
}
```

| Option | Description | Default |
|--------|-------------|---------|
| `local_path` | Local folder to sync | `~/Filen` |
| `remote_path` | Remote Filen path | `/` |
| `sync_mode` | Sync direction: `twoWay`, `localToCloud`, `cloudToLocal` | `twoWay` |
| `auto_start` | Start syncing on app launch | `true` |

## Usage

1. **First:** Login to the Filen CLI: `filen login`
2. **Launch:** Start the menubar app
3. **Click "Login..."** in the tray menu to start syncing (uses CLI's stored session)

### Menu Options

```
Status: Synced
Up to date              ← Shows "X files remaining..." when syncing
─────────────
Open Sync Folder        → Opens your local sync folder in Finder
Sync Now                → Trigger a manual one-shot sync
─────────────
Logout                  → Stop sync and clear session (with confirmation)
─────────────
Settings...             → Opens config file in TextEdit (macOS)
Quit                    → Stop syncing and exit
```

### Sync States

| State | Description |
|-------|-------------|
| **Not Logged In** | No CLI session found. Run `filen login` first. |
| **Synced** | All files are up to date |
| **Syncing...** | Files are being transferred (shows count) |
| **Paused** | Sync is paused |
| **Sync Error** | An error occurred during sync |

### Tooltip

Hover over the tray icon to see real-time status:
- `Filen - Synced` when idle
- `Filen - Syncing 5 files` when transferring

The tooltip updates in real-time, even while the menu is open.

## Architecture

```
┌─────────────────────────────────────────────────┐
│                  Filen Menubar                  │
├─────────────────────────────────────────────────┤
│  ┌─────────────────┐  ┌─────────────────────┐   │
│  │   macOS Tray    │  │    Linux Tray       │   │
│  │  (Tauri Icon)   │  │      (ksni)         │   │
│  └────────┬────────┘  └──────────┬──────────┘   │
│           │                      │              │
│           └──────────┬───────────┘              │
│                      │                          │
│  ┌───────────────────▼──────────────────────┐   │
│  │              App State                    │   │
│  │  (sync status, login state, pending cnt) │   │
│  └───────────────────┬──────────────────────┘   │
│                      │                          │
│  ┌───────────────────▼──────────────────────┐   │
│  │            CLI Manager                    │   │
│  │  (subprocess, JSON event parsing)        │   │
│  └───────────────────┬──────────────────────┘   │
│                      │                          │
└──────────────────────┼──────────────────────────┘
                       │
              ┌────────▼────────┐
              │   Filen CLI     │
              │  (@filen/cli)   │
              │  --verbose mode │
              └─────────────────┘
```

### CLI Event Parsing

The app runs the Filen CLI with `--verbose` flag to get JSON event output. Key events:

| Event | Description |
|-------|-------------|
| `cycleProcessingTasksStarted` | Sync cycle starting, set state to Syncing |
| `deltasCount` | Number of files to sync |
| `transfer` + `success` | A file completed, decrement pending count |
| `cycleSuccess` | Sync cycle completed, set state to Synced |
| `cycleError` | Sync cycle failed, set state to Error |

## Tech Stack

- **[Tauri v2](https://tauri.app/)** - Cross-platform app framework
- **[Rust](https://www.rust-lang.org/)** - Backend logic
- **[ksni](https://crates.io/crates/ksni)** - Linux StatusNotifierItem (KDE support)
- **[tokio](https://tokio.rs/)** - Async runtime for subprocess management
- **[tauri-plugin-dialog](https://crates.io/crates/tauri-plugin-dialog)** - Native dialogs

## Platform Notes

### macOS

- The app hides from the Dock (menubar-only)
- Uses Tauri's native TrayIcon with in-place menu updates
- Template icon support for automatic dark/light mode
- Settings open in TextEdit for easy editing

### Linux (KDE)

- Uses **ksni** for native StatusNotifierItem support
- First-class KDE Plasma integration
- No libappindicator fallback issues

### Linux (GNOME)

- Requires [AppIndicator extension](https://extensions.gnome.org/extension/615/appindicator-support/)
- Works via ksni's SNI protocol

## Known Limitations

- **CLI Dependency:** The app requires the Filen CLI to be installed and logged in separately
- **CLI Sunset:** Filen is rewriting their CLI in Rust (expected Q4 2025/Q1 2026). This app will need updates when released.
- **Login UI:** Currently requires running `filen login` in terminal. A proper login dialog is planned.
- **Storage Display:** Not available (CLI v0.0.39 doesn't expose storage quota)

## Development

```bash
# Run with debug logging
RUST_LOG=debug npm run tauri dev

# Run with info logging (less verbose)
RUST_LOG=info npm run tauri dev

# Check for issues
cargo clippy --manifest-path src-tauri/Cargo.toml

# Format code
cargo fmt --manifest-path src-tauri/Cargo.toml
```

### Project Structure

```
src-tauri/
├── src/
│   ├── lib.rs          # Main app setup, action handlers, status loop
│   ├── cli.rs          # CLI subprocess management, JSON event parsing
│   ├── config.rs       # Configuration loading/saving
│   ├── credentials.rs  # CLI session detection
│   ├── state.rs        # Shared app state (sync status, pending count)
│   └── tray/
│       ├── mod.rs      # TrayInterface trait
│       ├── macos.rs    # macOS tray implementation (Tauri TrayIcon)
│       └── linux.rs    # Linux tray implementation (ksni)
├── Cargo.toml
└── tauri.conf.json
```

## License

MIT

## Acknowledgments

- [Filen.io](https://filen.io) for their cloud storage service
- [Tauri](https://tauri.app) for the excellent cross-platform framework
- [ksni](https://github.com/ptsochantaris/ksni) for Linux tray support
