# Filen Menubar

A lightweight, native menubar/system tray application for [Filen.io](https://filen.io) cloud sync on macOS and Linux.

<img width="224" height="315" alt="image" src="https://github.com/user-attachments/assets/297db40d-2ac9-467a-9fe3-c537f2090ac2" />

## Features

- **Tray-first interface** - Stays out of the Dock/taskbar; the only app window is the optional sign-in flow
- **In-app login** - Sign in, complete two-factor authentication, and persist the Filen CLI session without opening a terminal
- **CLI-owned credentials** - Credentials are sent to the bundled CLI through a pseudo-terminal, never through process arguments, environment variables, or application logs
- **Real-time sync status** - Shows current sync state with live file count updates
- **Live menu updates** - Menu items update in-place without closing the menu
- **Recent activity** - Review the latest uploads, downloads, removals, and other file operations
- **Native platform styling** - A macOS-style login window on macOS and a KDE/Plasma-style window on Linux
- **Cross-platform tray support** - Tauri's native tray on macOS and StatusNotifierItem (SNI) via `ksni` on Linux
- **Auto-sync** - Optionally start syncing on launch
- **Bundled sync engine** - Ships a pinned classic CLI patched with the newer Filen sync engine's memory fixes
- **Logout confirmation** - Prevents accidental logout with a confirmation dialog
- **Update check** - Manually check GitHub for a newer release from the tray menu

## Requirements

### Bundled Filen CLI

No separate Filen CLI or Node.js runtime is required after installation. The
app bundles `v0.0.39-menubar.2`: a focused build of classic CLI v0.0.39 with
`@filen/sync` v0.3.7 and `@filen/sdk` v0.4.2. It runs on an app-owned copy of
the official Node.js v24.18.1 runtime. Neither the helper nor this Node.js
runtime is resolved from `PATH`: the runtime stays private to the application
bundle on macOS and under `/usr/lib/Filen Menubar/filen-cli/` in Linux packages.

The newer sync engine bounds large tree-building work, avoids deep-cloning
complete trees, and uses a more compact persisted state. The fork also stops
retaining an unused in-memory CLI log during continuous sync and omits ignored-
tree inventories that the menubar does not consume when no explicit CLI log
file is requested. A transient realtime-socket error now follows the SDK's
reconnect path instead of terminating the sync process. Unused drive, S3, and
WebDAV commands and their dependencies are excluded from the bundled build.
Its self-updater is disabled; app releases update the host, helper, and runtime
together.

The pinned inputs, patches, resolved dependency graph, license information, and
release-compliance tooling live under `third-party/filen-cli/`. Release assets
also include corresponding source, a software bill of materials (SBOM), and
third-party notices.

Existing classic-CLI sessions are reused, so upgrades do not require signing in
again. Do not run an older continuous-sync CLI against the same sync pair while
Filen Menubar is running. Its compact sync cache is isolated under `state/v3`,
leaving an older classic CLI's `state/v2` cache untouched for rollback. The
first patched run therefore performs a full tree scan.

### Build Dependencies

- [Rust](https://rustup.rs/) (latest stable)
- [Node.js](https://nodejs.org/) (v20+)
- [Bun](https://bun.sh/) v1.3.14 (exact version, build and test tool only)
- [Tauri CLI](https://tauri.app/)

**macOS:**
```bash
xcode-select --install
```

Release builds currently target Apple silicon and require macOS 13.5 or newer.

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

Linux release builds target x86_64, require glibc 2.34 or newer, and bundle the
Node.js runtime used by the sync backend.

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
sudo dpkg -i ./Filen.Menubar_*_amd64.deb

# Install any missing dependencies
sudo apt-get install -f
```

### Linux (Fedora/RHEL/openSUSE)

```bash
# Install the rpm package
sudo rpm -i ./Filen.Menubar-*.rpm

# Or with dnf
sudo dnf install ./Filen.Menubar-*.rpm
```

### Linux (Arch/CachyOS/Manjaro)

Install from the AUR with your usual helper:

```bash
paru -S filen-menubar-bin   # or: yay -S ...
```

The patched sync backend is included in `filen-menubar-bin`; there is no
separate CLI or system Node.js runtime dependency. The AUR package supports
x86_64 and carries the same app-owned runtime as the release `.deb`.

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
- Install missing distro build dependencies (Bun must already be installed at
  the exact version listed above)
- Build the application
- Install the host executable to `/usr/local/bin` and its private sync backend
  under `/usr/local/lib/Filen Menubar/`
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

# Install Bun v1.3.14, then clone and build
git clone https://github.com/philippgerard/filen-menubar.git
cd filen-menubar
npm ci
cargo install tauri-cli --version "^2" --locked
npm run tauri build

# Binary will be in src-tauri/target/release/filen-menubar
```

The build fails closed if Bun is missing or is not exactly v1.3.14.

> **Note:** AppImage is not supported because its sandboxing prevents reliable
> access to the bundled sync-backend resources.

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

### Recent Activity

The **"Recent Activity..."** window retains the newest 500 file operations
observed by this client, including uploads, downloads, directory changes,
removals, renames, and failed operations. Only paths relative to the configured
sync root are stored. Use **Clear** in the activity window to remove the history.

- **macOS:** `~/Library/Application Support/filen-menubar/activity-history.json`
- **Linux:** `~/.local/share/filen-menubar/activity-history.json`

## Usage

1. **Launch:** Start the menubar app
2. **Click "Login..."** in the tray menu
3. **Sign in:** Enter your Filen credentials and, when required, your two-factor code
4. **Sync:** The login window closes and syncing starts automatically

The app uses its bundled backend for authentication as well as syncing, so the
tray login is the supported sign-in path.

### Login Security

The sign-in screen is a local page bundled with Filen Menubar, not a hosted
Filen website. The app starts its bundled Filen CLI in a pseudo-terminal and
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
bundled login uses that same upstream authentication flow.

### Menu Options

```
Status: Synced
Up to date              ← Shows "X files remaining..." when syncing
─────────────
Open Local Folder       → Opens your local sync folder in Finder/file manager
Open Web UI             → Opens Filen web interface in browser
Recent Activity...      → Shows the latest file operations performed by this client
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
              │ Bundled CLI fork│
              │ @filen/sync .3.7│
              │  --verbose mode │
              └─────────────────┘
```

### CLI Event Parsing

The app runs the Filen CLI with `--verbose` flag to get JSON event output. Key events:

| Event | Description |
|-------|-------------|
| `cycleProcessingTasksStarted` | Sync cycle starting, set state to Syncing |
| `deltasCount` | Number of files to sync |
| task-level `transfer` + `success`/`error` | A file operation completed; update pending count and recent activity |
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
- The login and Recent Activity windows follow macOS typography, spacing, controls, and window sizing
- Settings open in TextEdit for easy editing

### Linux (KDE)

- Uses **ksni** for native StatusNotifierItem support
- First-class KDE Plasma integration
- No libappindicator fallback issues
- The login and Recent Activity windows follow KDE/Plasma control and color conventions

### Linux (GNOME)

- Requires [AppIndicator extension](https://extensions.gnome.org/extension/615/appindicator-support/)
- Works via ksni's SNI protocol

## Known Limitations

- **CLI lineage:** The bundled backend remains the classic TypeScript CLI, run
  by its app-owned Node.js runtime. The Rust rewrite is not a drop-in
  replacement for the current continuous-sync event integration.
- **Backend scope:** The focused helper includes the sync and interactive-login
  surfaces used by Filen Menubar. It does not bundle the classic CLI's
  filesystem utilities, drive, S3, or WebDAV servers.
- **Storage Display:** The current CLI integration does not expose account storage quota.

## Development

### Quick Start

```bash
# Install the pinned JavaScript dependencies, then start with debug logging
npm ci
RUST_LOG=debug npm run tauri dev
```

### Development Scripts

The `scripts/` directory contains helper scripts for common tasks:

| Script | Description |
|--------|-------------|
| `./scripts/build-filen-cli.sh` | Build and verify the pinned bundled backend |
| `./scripts/check-filen-cli-version.sh` | Verify the generated backend version and runtime |
| `./scripts/check-filen-cli-state-patch.sh` | Verify the isolated state-v3 patch |
| `./scripts/package-filen-cli-source.sh` | Create the backend corresponding-source archive |
| `./scripts/install-linux.sh` | Build and install from source on Linux |
| `./scripts/logs.sh` | Open log folder for current platform |

### Manual Commands

```bash
# Install the pinned JavaScript dependencies
npm ci

# Build the pinned patched CLI (also runs automatically before checks/builds)
npm run build:filen-cli

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

Create a reviewed `v*` tag and dispatch the **Release** workflow from `main`
with its numeric version. The workflow rejects a tag whose commit is not already
on `main` or whose five app version manifests disagree. It then:

1. Rebuilds and verifies the pinned CLI fork and runs the test suite on macOS
   and Linux.
2. Builds a signed and notarized Apple Silicon DMG.
3. Builds Debian, RPM, and Arch Linux packages.
4. Produces corresponding source, an SBOM, third-party notices, and checksums,
   then creates a draft GitHub release containing all artifacts.
5. Publishes `filen-menubar-bin` to the AUR after the draft release is reviewed and published.

### Troubleshooting

**Locale changes not reflected?**

The `rust_i18n` crate compiles translations at build time. If you modify locale files (`src-tauri/locales/*.yml`), you need a clean rebuild:

```bash
cargo clean --manifest-path src-tauri/Cargo.toml
RUST_LOG=debug npm run tauri dev
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

The Filen Menubar host application is MIT-licensed. The separately executed,
modified Filen CLI backend is AGPL-3.0-only. Its app-owned Node.js runtime is
MIT-licensed and carries Node.js's third-party notices. Complete notices and
corresponding source are included with release artifacts. Bun is used only as a
pinned build/test tool and is not distributed in the application bundles.

## Acknowledgments

- [Filen.io](https://filen.io) for their cloud storage service
- [Tauri](https://tauri.app) for the excellent cross-platform framework
- [ksni](https://github.com/ptsochantaris/ksni) for Linux tray support
