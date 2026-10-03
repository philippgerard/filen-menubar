# GTK3 compatibility backports

Tauri 2's Linux backend still requires the GTK3/GLib 0.18 API. These three
complete crates.io source archives retain their upstream package versions,
licenses, features and public APIs. The application selects the local sources
through `src-tauri/Cargo.toml`'s `[patch.crates-io]` entries. They contain narrow
source fixes, rather than claiming compatibility with a newer GLib generation.

| Crate | Version | Original archive SHA-256 |
| --- | --- | --- |
| glib | 0.18.5 | `233daaf6e83ae6a12a52055f568f9d7cf4671dabb78ff9560ab6da230ce00ee5` |
| glib-macros | 0.18.5 | `0bb0228f477c0900c880fd78c8759b95c7636dbd7842707f49e132378aa2acdc` |
| gtk3-macros | 0.18.2 | `52ff3c5b21f14f0736fed6dcfc0bfb4225ebf5725f3c0209edeec181e4d73e9d` |

The original archives are available at
`https://static.crates.io/crates/NAME/NAME-VERSION.crate`. Each archive was
verified against its crates.io checksum before extraction. Published source
metadata identifies gtk-rs-core commit
`42b9caf98e03ded086362d9653ca58fe94dc8658` for both GLib crates and gtk3-rs
commit `00133512bfc8bbb8e9be59b20e93406c6a3eeb21` for gtk3-macros; the latter's
published metadata also records a dirty tree. The archive checksum is the exact
baseline. `Cargo.toml.orig` is retained as upstream source metadata; Cargo uses
the normalized `Cargo.toml` files.

## Changes from those archives

- `glib`: backport
  [gtk-rs-core fix b5a4071e439bef2b5eea76c3aa25e5ae84839e34](https://github.com/gtk-rs/gtk-rs-core/commit/b5a4071e439bef2b5eea76c3aa25e5ae84839e34)
  for [RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html).
  `VariantStrIter::impl_get` passes a mutable out-pointer to the variadic C
  function. Its new optimized regression covers forward/backward iteration,
  `nth`, `nth_back`, `last`, Unicode, empty strings and exhaustion.
- `glib-macros`: backport
  [gtk-rs-core removal c74a40a16b23a1b9c8e67842d32d02bc187f551d](https://github.com/gtk-rs/gtk-rs-core/commit/c74a40a16b23a1b9c8e67842d32d02bc187f551d)
  of `proc-macro-error` using `syn::Error` and explicit result propagation.
  The enum, flags, object subclass and object interface paths are adapted to
  the 0.18 implementation, preserving its generated code and avoiding 0.19
  dynamic-type APIs. Compiler tests cover the changed invalid-input paths;
  the original integration tests cover valid expansion and runtime behavior.
- `gtk3-macros`: backport
  [gtk3-rs removal 45782251fa35ecf8bfe759ffceb3db32659a0c97](https://github.com/gtk-rs/gtk3-rs/commit/45782251fa35ecf8bfe759ffceb3db32659a0c97)
  of `proc-macro-error`. Compiler tests cover missing/malformed templates,
  unsupported input, duplicate child attributes and a valid widget template.
  `trybuild2` is added only as a test dependency.

`patches/*.patch` record every modification and regression-test addition
relative to the verified archives. `SOURCE-HASHES.json` records the exact local
crate files. The small workspace here is only for reproducible backport tests;
its lockfile is independent of the application's lockfile.

Removing `proc-macro-error` resolves RUSTSEC-2024-0370 without introducing its
also-unmaintained successor (`proc-macro-error2`, RUSTSEC-2026-0173). A
version-only scanner may still flag GLib 0.18.5 for RUSTSEC-2024-0429, while
scanners that omit local path packages may report no advisory. Neither result
proves the backport is present: verification must check the documented source
fix and regression. These changes do not claim that
the remaining GTK3 stack has regained upstream maintenance.

`npm run check:rust-audit` restores the three crates' original registry identities
in a disposable audit-only lockfile, preserving the application's actual
dependency graph. This keeps future registry advisories visible even though
Cargo builds local sources. The gate first verifies every source checksum and
the application's patch selections, then accepts only the exact GLib
0.18.5/RUSTSEC-2024-0429 unsoundness warning. All other vulnerabilities and
warnings, ignored advisories, malformed reports, and audit failures are fatal.
The application lockfile and package versions are never changed by the gate.

## Verification

Run from the repository root on a host with GLib/GIO development libraries:

```sh
cargo fmt --manifest-path third-party/rust/Cargo.toml --all -- --check
cargo test --manifest-path third-party/rust/Cargo.toml --locked -p glib --release --lib variant_iter::tests -- --test-threads=1
cargo test --manifest-path third-party/rust/Cargo.toml --locked -p glib-macros --tests -- --test-threads=1
```

With GTK3 development libraries available (normally the Linux build host):

```sh
cargo test --manifest-path third-party/rust/Cargo.toml --locked -p gtk3-macros --test diagnostics -- --test-threads=1
npm run test:rust-backports
npm run check
```

The compiler tests do not initialize GTK or require a graphical display or a
Filen login. Keep the optimized iterator test: debug-only execution does not
adequately exercise the original undefined behavior.
