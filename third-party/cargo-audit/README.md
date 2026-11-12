# Pinned security auditor

`scripts/install-cargo-audit.mjs` builds cargo-audit 0.23.0 from its official
crates.io archive, verified against the checksum in `source.json`. The checked-in
`Cargo.lock` replaces the archive's lock before `cargo install --path --locked`.
It updates rustix to 1.1.6 for
[RUSTSEC-2026-0486](https://rustsec.org/advisories/RUSTSEC-2026-0486.html), which
still affects the upstream 0.23.0 lock. The source code is unchanged.
The precise Cargo update also normalized six Windows dependency references to
the existing windows-sys 0.52.0 entry; no other package version changed.

The tool cache is bound to the archive, lock, installer, host platform and
architecture. A receipt beside the binary records its hash, so incomplete cached
tools and binaries whose bytes changed are rebuilt even when they still report
the expected version. The receipt detects accidental changes only; anyone who
can rewrite both files bypasses it. Failed downloads, checksum mismatches,
failed builds and incorrect executable versions cannot publish a cached
installation.
The bundled-backend build fingerprint includes the installer, source pin and
reviewed lock so updates invalidate that build's cached audit as well.

Every `scripts/audit-keyring.sh` invocation first audits this tool lock with
`--deny warnings`, then applies the existing keyring policy. The application's
verified-backport and exact maintenance-notice policy remains unchanged.
The auditor is a build tool and is not distributed in Filen Menubar.

To update, verify the crates.io archive checksum, take its upstream lock, apply
only the required precise security updates, and review the lock diff. Run
`npm run check:rust-audit`; it runs the installer tests, installs the new tool
and audits the tool, keyring and application.
Keep `--locked`; do not replace the reviewed graph with install-time resolution.
