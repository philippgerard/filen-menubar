# Rebuilding the bundled Filen backend

This archive is the corresponding source for one platform build of the
`v0.0.39-menubar.2` backend distributed with Filen Menubar. It contains:

- the patched Filen CLI source and its frozen Bun lockfile;
- the Filen Sync TypeScript source with the state-v3 change applied, plus the
  equivalent patch used for the published package's generated JavaScript;
- the exact Filen SDK source used by the runtime, patched so an unobserved
  transient realtime-socket error follows the reconnect path instead of
  terminating Node;
- the source-built native keyring wrapper, its Cargo lockfile, and a complete
  `cargo vendor` tree for every locked crate;
- the source trees for every npm package present in the esbuild runtime graph;
- the official Node.js `v24.18.1` source archive, its license/notices, the
  runtime SBOM, and the patches and scripts used to produce the distribution.

The preferred build hosts are macOS 13.5 or newer on Apple silicon and Linux
x86_64 with glibc 2.34 or newer. Install Rust stable, npm, official Node.js
`v24.18.1`, and Bun `1.3.14` revision `1.3.14+0d9b296af`. Linux also needs a C
toolchain, `pkg-config`, and D-Bus development headers.

Run:

```sh
./rebuild-source.sh
```

The script rebuilds the state-v3 TypeScript output and installs that rebuilt
output into the CLI dependency tree, runs the focused memory/logging tests,
recreates the CJS bundle, builds the native
keyring addon offline from the vendored Cargo graph, and launches the rebuilt
helper with the official Node runtime. Node's WebAssembly-backed HTTP and
WebSocket parser requires normal JIT operation; the distributed macOS helper
therefore has the narrow `allow-jit` hardened-runtime entitlement while the
Rust host has none. The runtime package source trees, including the exact SDK
source, are provided for inspection and modification; the frozen lockfile
remains the authoritative dependency-resolution input.
