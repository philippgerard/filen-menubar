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
filen
```

This opens an interactive session where you can login.

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

**Linux (Arch/CachyOS/Manjaro):**
```bash
sudo pacman -S webkit2gtk-4.1 libappindicator-gtk3 librsvg base-devel \
  curl wget file openssl libxdo
```

> **Note:** The `webkit2gtk-4.1` package is essential - it provides `javascriptcoregtk-4.1` required by Tauri.

## Installation

### Pre-built Binaries

Download the latest release from the [Releases](https://github.com/philippgerard/filen-menubar/releases) page:

- **macOS:** `.dmg` installer
- **Linux (Debian/Ubuntu):** `.deb` package
- **Linux (Fedora/RHEL):** `.rpm` package

### Linux (Debian/Ubuntu)

```bash
# Install the deb package
sudo dpkg -i filen-menubar_*.deb

# Install any missing dependencies
sudo apt-get install -f
```

### Linux (Fedora/RHEL/openSUSE)

```bash
# Install the rpm package
sudo rpm -i filen-menubar-*.rpm

# Or with dnf
sudo dnf install filen-menubar-*.rpm
```

### Linux (Arch/CachyOS/Manjaro)

Use the install script for a complete installation with autostart:

```bash
git clone https://github.com/philippgerard/filen-menubar.git
cd filen-menubar
./scripts/install-linux.sh install
```

This will:
- Install all build dependencies
- Build the application
- Install binary to `/usr/local/bin`
- Create desktop entry and icons
- Configure autostart on login

Or build manually:

```bash
# Install build dependencies
sudo pacman -S webkit2gtk-4.1 base-devel curl wget file openssl libxdo \
  libappindicator-gtk3 librsvg nodejs npm rust

# Clone and build
git clone https://github.com/philippgerard/filen-menubar.git
cd filen-menubar
npm install
cargo install tauri-cli --version "^2" --locked
npm run tauri build

# Binary will be in src-tauri/target/release/filen-menubar
```

> **Note:** AppImage is not supported due to sandboxing issues with accessing the Filen CLI.

#### Install Script Options

The install script supports several commands:

```bash
./scripts/install-linux.sh install    # Full install (build + setup + autostart)
./scripts/install-linux.sh build      # Build only
./scripts/install-linux.sh setup      # Install pre-built binary + autostart
./scripts/install-linux.sh autostart  # Setup autostart only
./scripts/install-linux.sh uninstall  # Remove everything
./scripts/install-linux.sh deps       # Install dependencies only
```

### Building from Source

```bash
# Clone the repository
git clone https://github.com/philippgerard/filen-menubar.git
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
  "localPath": "~/Filen",
  "remotePath": "/",
  "syncMode": "twoWay",
  "autoStart": true,
  "locale": "de",
  "loggingEnabled": true,
  "logLevel": "info"
}
```

| Option | Description | Default |
|--------|-------------|---------|
| `localPath` | Local folder to sync | `~/Filen` |
| `remotePath` | Remote Filen path | `/` |
| `syncMode` | Sync direction: `twoWay`, `localToCloud`, `cloudToLocal` | `twoWay` |
| `autoStart` | Start syncing on app launch | `true` |
| `locale` | UI language (`en`, `de`). If omitted, uses system locale | System locale |
| `loggingEnabled` | Enable file logging for debugging | `false` |
| `logLevel` | Log verbosity: `trace`, `debug`, `info`, `warn`, `error` | `info` |

### Logging

File logging is disabled by default. When enabled, logs are written to a single file that is overwritten on each app launch:

- **macOS:** `~/Library/Logs/io.filen.menubar/filen.log`
- **Linux:** `~/.local/share/filen-menubar/logs/filen.log`

To enable logging, add `"loggingEnabled": true` to your config file. You can also set the log level with `"logLevel": "debug"` for more verbose output.

Access logs via the **"Show Logs..."** menu item, or use the helper script:

```bash
./scripts/logs.sh
```

## Usage

1. **First:** Login to the Filen CLI by running `filen` and following the prompts
2. **Launch:** Start the menubar app
3. **Click "Login..."** in the tray menu to start syncing (uses CLI's stored session)

### Menu Options

```
Status: Synced
Up to date              ← Shows "X files remaining..." when syncing
─────────────
Open Local Folder       → Opens your local sync folder in Finder/file manager
Open Web UI             → Opens Filen web interface in browser
─────────────
Logout                  → Stop sync and clear session (with confirmation)
─────────────
Settings...             → Opens config file in editor
Show Logs...            → Opens log folder (for debugging)
─────────────
Quit                    → Stop syncing and exit
```

### Sync States

| State | Description |
|-------|-------------|
| **Not Logged In** | No CLI session found. Run `filen` to login first. |
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
- **Login UI:** Currently requires running `filen` in terminal to login. A proper login dialog is planned.
- **Storage Display:** Not available (CLI v0.0.39 doesn't expose storage quota)

## Development

### Quick Start

```bash
# Start development (with debug logging)
./scripts/dev.sh

# Or manually
RUST_LOG=debug npm run tauri dev
```

### Development Scripts

The `scripts/` directory contains helper scripts for common tasks:

| Script | Description |
|--------|-------------|
| `./scripts/dev.sh` | Start development server with debug logging |
| `./scripts/clean.sh` | Clean build cache and kill running instances |
| `./scripts/rebuild.sh` | Clean rebuild (use after locale/compile-time changes) |
| `./scripts/test.sh` | Run all tests |
| `./scripts/lint.sh` | Check formatting and run clippy |
| `./scripts/lint.sh --fix` | Auto-fix formatting issues |
| `./scripts/release.sh` | Build production release (runs tests first) |
| `./scripts/logs.sh` | Open log folder for current platform |

### Manual Commands

```bash
# Run with info logging (less verbose)
RUST_LOG=info npm run tauri dev

# Check for issues
cargo clippy --manifest-path src-tauri/Cargo.toml

# Format code
cargo fmt --manifest-path src-tauri/Cargo.toml
```

### Building a Notarized macOS Release

To build a signed and notarized macOS release locally:

1. **Prerequisites:**
   - Apple Developer ID Application certificate installed in Keychain
   - App-specific password from [appleid.apple.com](https://appleid.apple.com) → Sign-In and Security → App-Specific Passwords

2. **Create a `.env` file** (already gitignored):

   ```bash
   export APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAM_ID)"
   export APPLE_ID="your-apple-id@email.com"
   export APPLE_TEAM_ID="YOUR_TEAM_ID"
   export APPLE_PASSWORD="xxxx-xxxx-xxxx-xxxx"  # App-specific password
   ```

3. **Build:**

   ```bash
   source .env && npm run tauri build -- --target aarch64-apple-darwin
   ```

4. **Output:** The signed and notarized DMG will be at:
   ```
   src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/Filen Menubar_<version>_aarch64.dmg
   ```

5. **Verify notarization:**

   ```bash
   spctl -a -vvv "src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Filen Menubar.app"
   # Should show: source=Notarized Developer ID
   ```

### Troubleshooting

**Locale changes not reflected?**

The `rust_i18n` crate compiles translations at build time. If you modify locale files (`src-tauri/locales/*.yml`), you need a clean rebuild:

```bash
./scripts/rebuild.sh
./scripts/dev.sh
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
