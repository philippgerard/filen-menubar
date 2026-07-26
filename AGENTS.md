# Filen Menubar agent guide

This is a Tauri v2 menubar/system-tray application for macOS and Linux. It
wraps the Filen CLI and has no window-based frontend; almost all application
behavior lives in Rust under `src-tauri/`.

## Workflow

- Run repository shell commands and make edits only from a Git worktree. The
  project hook enforces this for Codex.
- Keep changes focused and preserve unrelated user work.
- Add or update focused tests when behavior changes.
- Do not commit, push, tag, publish, deploy, sign, or notarize unless the user
  explicitly requests that action.
- Do not load `.env` or expose release credentials during ordinary development
  and verification.
- When a check cannot run because a platform, system dependency, Filen login, or
  external tool is unavailable, report the exact limitation instead of claiming
  success.

## Setup and commands

```bash
# Deterministic dependency installation
npm ci

# Development with hot reload
npm run tauri dev

# Development with debug logging
RUST_LOG=debug npm run tauri dev

# Formatting check, Clippy, and Rust tests
npm run check

# Individual checks
npm run check:fmt
npm run check:clippy
npm test

# Unsigned production build
npm run tauri build
```

Use the individual check closest to the change while iterating. Before
declaring a Rust behavior change complete, run `npm run check`. A documentation-
only change does not require a build; inspect the diff instead. Run a production
build only when the task needs bundle-level verification.

## Architecture

Important Rust modules under `src-tauri/src/`:

- `lib.rs`: app entry point, tray action dispatch, reactive status loop, and
  auto-start behavior.
- `actions.rs`: tray action handlers using `ActionContext`.
- `cli/`: Filen subprocess discovery, lifecycle, output framing, event parsing,
  network-error classification, and the injectable `ProcessRunner`.
- `config.rs`: persisted application configuration and sync-pair generation.
- `credentials.rs`: Filen CLI session detection and removal.
- `state.rs`: `AppState`, state-transition validation, and watch-channel
  notifications.
- `update.rs`: user-triggered GitHub release checks.
- `tray/macos.rs` and `tray/linux.rs`: platform tray implementations behind
  `TrayInterface`.

The event flow is:

```text
TrayAction -> action handler -> CliManager -> AppState -> status loop -> tray
```

## Invariants

- Preserve macOS/Linux parity. Menu labels, enabled states, actions, and behavior
  must remain equivalent across both tray implementations unless a genuine
  platform limitation is documented.
- Update `src-tauri/locales/en.yml` and `src-tauri/locales/de.yml` together when
  user-facing copy changes.
- Keep Filen subprocess behavior testable through `ProcessRunner`; tests should
  not require a real Filen login or mutate a user's sync directory.
- State changes should flow through `AppState` and notify subscribers. Preserve
  the state machine's validated transitions unless the task explicitly changes
  that contract.
- The update check remains user-triggered. Do not introduce background polling
  or automatic installation without an explicit product decision.

### Linux `ksni` constraints

- `ksni::Handle::update()` is asynchronous and must be awaited for D-Bus signals
  to be emitted. Synchronous `TrayInterface` methods schedule it through Tauri's
  async runtime.
- `LinuxTray` deliberately stores menu state in an external
  `Arc<RwLock<LinuxTrayState>>`.
- `handle.update(|_| {}).await` deliberately uses an empty closure because the
  shared state is updated before the signal is triggered.

## Versioning and packaging

Keep the application version synchronized in:

- `package.json`
- `src-tauri/tauri.conf.json`
- `src-tauri/Cargo.toml`
- `package-lock.json`, refreshed with `npm install`
- `src-tauri/Cargo.lock`, refreshed with
  `cargo update -p filen-menubar --manifest-path src-tauri/Cargo.toml`

`packaging/arch/PKGBUILD.in` uses `@PKGVER@`; do not hard-code the application
version there. The Arch package repacks the release `.deb`, and
`packaging/arch/render.sh` is the source of truth for rendering its build
directory.

## Code review rules

- Prioritize correctness, subprocess lifecycle safety, state-transition
  regressions, platform parity, packaging/release correctness, and missing
  focused tests.
- Treat formatting-only observations as non-actionable when the formatter
  already handles them.
- Call out validation that is platform-specific or could not be run locally.
