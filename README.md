# Filen Menubar

A lightweight, native menubar/system tray application for [Filen.io](https://filen.io) cloud sync on macOS and Linux.

<img width="224" height="315" alt="image" src="https://github.com/user-attachments/assets/297db40d-2ac9-467a-9fe3-c537f2090ac2" />

## Features

- **Tray-first interface** - Stays out of the Dock/taskbar; the only app window is the optional sign-in flow
- **In-app login** - Sign in, complete two-factor authentication, and persist the Filen CLI session without opening a terminal
- **CLI-owned credentials** - Credentials are sent to the installed CLI through a pseudo-terminal, never through process arguments, environment variables, or application logs
- **Real-time sync status** - Shows current sync state with live file count updates
- **Live menu updates** - Menu items update in-place without closing the menu
- **Native platform styling** - A macOS-style login window on macOS and a KDE/Plasma-style window on Linux
- **Cross-platform tray support** - Tauri's native tray on macOS and StatusNotifierItem (SNI) via `ksni` on Linux
- **Auto-sync** - Optionally start syncing on launch
- **Logout confirmation** - Prevents accidental logout with a confirmation dialog
- **Update check** - Manually check GitHub for a newer release from the tray menu

## Requirements

### Filen CLI

This app wraps the classic [Filen CLI](https://github.com/FilenCloudDienste/filen-cli)
and is currently tested with v0.0.36. Install it first:

```bash
npm install -g @filen/cli
```

Verify installation:

```bash
filen --version
```

Filen Menubar can guide you through the Filen CLI login from its tray menu.
If you prefer not to enter credentials in the app window, run `filen` or
`filen-cli` in a terminal and answer `y` when asked to keep the session. Filen
Menubar detects and uses that same saved CLI session.

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
- **Linux (Arch):** [`filen-menubar-bin`](https://aur.archlinux.org/packages/filen-menubar-bin) on the AUR, or the `.pkg.tar.zst` attached to each release

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

Install from the AUR with your usual helper:

```bash
paru -S filen-menubar-bin filen-cli-bin   # or: yay -S ...
```

`filen-cli-bin` is the sync backend. It is a standalone binary, so unlike the
npm route it pulls in no Node.js — you can skip `npm install -g @filen/cli`
entirely on Arch.

This is the recommended route: pacman owns the files, so `pacman -Qo` works and
updates arrive with your normal `paru -Sua`. The package repacks the release
binary rather than compiling, so it installs in seconds.

Without an AUR helper, grab the `.pkg.tar.zst` from the
[latest release](https://github.com/philippgerard/filen-menubar/releases/latest):

```bash
sudo pacman -U filen-menubar-bin-*-x86_64.pkg.tar.zst
```

Autostart is not configured by either route. To enable it:

```bash
cp /usr/share/applications/filen-menubar.desktop ~/.config/autostart/
```

#### Building from source

Use the install script for a complete from-source installation with autostart:

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

> **Note:** `/usr/local/bin` normally precedes `/usr/bin` on `PATH`, so a
> script-installed binary shadows the packaged one. Pick one route or the
> other — if you switch to the AUR package, remove the old copy first with
> `./scripts/install-linux.sh uninstall`.

Or build manually:

```bash
# Install build dependencies
sudo pacman -S webkit2gtk-4.1 base-devel curl wget file openssl libxdo \
  libappindicator-gtk3 librsvg nodejs npm rust

# Clone and build
git clone https://github.com/philippgerard/filen-menubar.git
cd filen-menubar
npm ci
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
npm ci

# Install Tauri CLI
cargo install tauri-cli --version "^2" --locked

# Run in development mode
npm run tauri dev

# Build for production
npm run tauri build
```

## Configuration

Configuration is stored in a JSON file:

- **macOS:** `~/Library/Application Support/filen-menubar/config.json`
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
  "logLevel": "info",
  "ignore": ["node_modules", "*.tmp"],
  "excludeDotFiles": false
}
```

| Option | Description | Default |
|--------|-------------|---------|
| `localPath` | Local folder to sync | `~/Filen` |
| `remotePath` | Remote Filen path | `/` |
| `syncMode` | Sync direction: `twoWay`, `localToCloud`, `cloudToLocal`, `localBackup`, or `cloudBackup` | `twoWay` |
| `autoStart` | Start syncing on app launch | `true` |
| `locale` | UI language (`en`, `de`). If omitted, uses system locale | System locale |
| `loggingEnabled` | Enable file logging for debugging | `false` |
| `logLevel` | Log verbosity: `trace`, `debug`, `info`, `warn`, `error` | `info` |
| `ignore` | File or directory patterns excluded from sync | `[]` |
| `excludeDotFiles` | Exclude files and directories whose names start with `.` | `false` |

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

1. **Launch:** Start the menubar app
2. **Click "Login..."** in the tray menu
3. **Sign in:** Enter your Filen credentials and, when required, your two-factor code
4. **Sync:** The login window closes and syncing starts automatically

The login window is optional. To authenticate manually instead, close it and
run `filen` or `filen-cli` in a terminal. Answer `y` when the CLI asks whether
to keep you logged in, then relaunch Filen Menubar or choose **Login...** again.

### Login Security

The sign-in screen is a local page bundled with Filen Menubar, not a hosted
Filen website. The app starts the installed Filen CLI in a pseudo-terminal and
responds to its normal interactive prompts:

- The CLI receives email, password, and two-factor codes through its standard
  input rather than command-line arguments or environment variables.
- Rust-side secret buffers are zeroed after use, and credentials are never
  written to application logs.
- Filen Menubar answers `y` to the CLI's **Keep me logged in?** prompt. The CLI
  encrypts the saved session and protects its encryption key with macOS
  Keychain or the Linux Secret Service.
- If secure system storage is unavailable, the app refuses the CLI's plaintext-storage fallback.

For two-factor-enabled accounts, Filen may send one failed-login alert followed
by the successful-login alert. The classic CLI first submits the password to
learn that a two-factor code is required, then submits the complete login. The
same behavior occurs during a manual interactive CLI login.

### Menu Options

```
Status: Synced
Up to date              ← Shows "X files remaining..." when syncing
─────────────
Open Local Folder       → Opens your local sync folder in Finder/file manager
Open Web UI             → Opens Filen web interface in browser
─────────────
Pause Syncing           → Pause/resume syncing (label toggles with state)
Logout                  → Stop sync and clear session (with confirmation)
─────────────
Settings...             → Opens config file in editor
Show Logs...            → Opens log folder (for debugging)
About Filen Menubar     → Shows version and app info
Check for Updates...    → Checks GitHub for a newer release, opens download page
─────────────
Quit                    → Stop syncing and exit
```

### Sync States

| State | Description |
|-------|-------------|
| **Not Logged In** | No CLI session found. Choose **Login...** from the tray menu. |
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
│  │       Optional Login Window              │   │
│  │  local UI → PTY → interactive CLI auth  │   │
│  └───────────────────┬──────────────────────┘   │
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
- **[portable-pty](https://crates.io/crates/portable-pty)** - Interactive CLI login without exposing secrets as arguments
- **[zeroize](https://crates.io/crates/zeroize)** - Clears credentials from memory after use
- **[tauri-plugin-dialog](https://crates.io/crates/tauri-plugin-dialog)** - Native dialogs

## Platform Notes

### macOS

- The app hides from the Dock (menubar-only)
- Uses Tauri's native TrayIcon with in-place menu updates
- Template icon support for automatic dark/light mode
- The optional login window follows macOS typography, spacing, controls, and window sizing
- Settings open in TextEdit for easy editing

### Linux (KDE)

- Uses **ksni** for native StatusNotifierItem support
- First-class KDE Plasma integration
- No libappindicator fallback issues
- The optional login window follows KDE/Plasma control and color conventions

### Linux (GNOME)

- Requires [AppIndicator extension](https://extensions.gnome.org/extension/615/appindicator-support/)
- Works via ksni's SNI protocol

## Known Limitations

- **CLI Dependency:** The Filen CLI must be installed; Filen Menubar can handle login after installation.
- **CLI Migration:** The app currently targets the classic Filen CLI v0.0.36. The Rust rewrite is not yet supported and will require sync integration changes.
- **Storage Display:** The current CLI integration does not expose account storage quota.

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
# Install the pinned JavaScript dependencies
npm ci

# Run with info logging
RUST_LOG=info npm run tauri dev

# Formatting, Clippy, and all Rust tests
npm run check

# Build an unsigned local production bundle
npm run tauri build
```

### Release Pipeline

Application versions must match in `package.json`, `package-lock.json`,
`src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, and
`src-tauri/tauri.conf.json`.

After creating a reviewed `v*` tag, dispatch the **Release** workflow from
`main` and enter its numeric version. The workflow rejects tags that are not
already contained in the selected `main` commit. It then:

1. Runs the Rust test suite on macOS and Linux.
2. Builds a signed and notarized Apple Silicon DMG.
3. Builds Debian, RPM, and Arch Linux packages.
4. Creates a draft GitHub release containing all artifacts.
5. Publishes `filen-menubar-bin` to the AUR after the draft release is reviewed and published.

### Troubleshooting

**Locale changes not reflected?**

The `rust_i18n` crate compiles translations at build time. If you modify locale files (`src-tauri/locales/*.yml`), you need a clean rebuild:

```bash
./scripts/rebuild.sh
./scripts/dev.sh
```

### Project Structure

```text
src/
├── login.html         # Optional local sign-in window
├── login.css          # macOS and KDE/Plasma presentation
└── login.js           # Login UI state and Tauri commands

src-tauri/
├── src/
│   ├── lib.rs          # Main app setup, action dispatch, status loop
│   ├── actions.rs      # Tray action handlers (command pattern)
│   ├── cli/            # CLI subprocess management, JSON event parsing
│   ├── config.rs       # Configuration loading/saving
│   ├── credentials.rs  # CLI session detection
│   ├── login.rs        # PTY-driven interactive login and lifecycle handling
│   ├── state.rs        # Shared app state (sync status, pending count)
│   ├── update.rs       # GitHub release update check
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
