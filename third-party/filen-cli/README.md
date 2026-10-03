# Bundled Filen CLI backend

Filen Menubar bundles a separately executed, modified build of the classic
Filen CLI. The menubar application remains MIT-licensed; the helper and its
Filen dependencies are distributed under AGPL-3.0-only.

## Version and modifications

- Fork version: `v0.0.39-menubar.2`
- Upstream CLI: `v0.0.39`, commit
  `ca966d86d1fe3ed204088e448299d174288085f6`
- `@filen/sync`: `0.3.7`, with bounded tree-building, shallow snapshots, and
  compact state; state is isolated under `state/v3` for rollback safety
- `@filen/sdk`: `0.4.2`, matching the sync worker's SDK class identity, with
  unobserved transient realtime-socket errors left to its reconnect path
  instead of being raised as uncaught Node errors
- Continuous logging: output is not retained when no `--log-file` is
  configured; ignored-tree inventories are not serialized unless explicitly
  requested through file logging
- Surface: self-update, filesystem utility, mount, WebDAV, and S3 commands are
  excluded from the runtime bundle because Filen Menubar invokes only login,
  version probing, and continuous sync
- Runtime: official Node.js `v24.21.0` plus an esbuild CJS entrypoint. Bun
  `1.4.2+744846f84` is pinned as a build/test/package-lock tool only and is
  not distributed in the application
- Keyring: `@jupiterpi/node-keyring` is rebuilt from source commit
  `165e4334ff365792d9b1274761e8afeedcccaffe` with the checked-in Cargo lock.
  This avoids the published Linux binary's newer-glibc symbol requirement.

The production dependency graph is frozen in `bun.lock`. `braces@3.0.3` and
`node-forge@1.4.0` carry source backports for GHSA-vfj7-8cjw-p6xm and
GHSA-86w9-cpqp-85rv. The former applies the `lib/` diff from 3.0.3 to upstream
PR 72 at commit `28d440b5dd449dbf1fe6f3506cf94ecca4d02660`, bounding parser and
AST recursion to 100 levels and detecting parent cycles. Because that PR is
based on unreleased upstream `master`, the diff also carries upstream PR 49
(commas reset block invalidity, so sets starting with a range operator expand)
and PR 57 (unpaired quotes parse as literal characters instead of consuming the
remaining input). The `node-forge` backport applies the nested-child check
from upstream PR 1152 at commit `ceba34402e329f0365134f23fe19898756527d65`,
additionally requiring empty primitive NULL parameters. Both retain their real
upstream package versions. The checked-in patch files and
`security-backports.json` identify the exact source bytes and upstream
provenance; none of these upstream changes is in a released package.

The builder fails on high or critical production audit findings. The only
accepted findings are those two exact package/advisory pairs, after every
installed copy passes the pinned whole-package checksum and the malicious-input
and legitimate-behavior regressions. Raw Bun audit still reports their original
versions; the source-aware gate prints the verified-backport status explicitly.
Unexpected source, missing copies, malformed audit output, audit transport
failure, and any other high/critical advisory fail the build. Upgrade to released
upstream fixes and remove these patches when compatible releases become available.

The builder creates a CycloneDX runtime SBOM
and dependency notices from the actual esbuild input graph, and packages the
patched CLI and sync source, native-addon source, the complete vendored Cargo
graph, official Node source, locks, patches, and runtime package sources as a
corresponding-source artifact. CI extracts and rebuilds that archive on every
platform before a reviewed release can be drafted. Every runtime npm package
and Cargo crate must supply substantive license text through a standard
license file (including `LICENSE-*` variants), a README license section, or an
exact-hash pinned upstream supplement for a crate archive that omitted it;
declared package metadata alone is not accepted.
The SBOM and notices retain the package identities and record their source
backports and patched-package checksums. The corresponding-source rebuild runs
the same security regressions and production audit gate before bundling.

## Source and licenses

- Classic CLI: <https://github.com/FilenCloudDienste/filen-cli/tree/ca966d86d1fe3ed204088e448299d174288085f6>
- Sync engine: <https://github.com/FilenCloudDienste/filen-sync/tree/v0.3.7>
- SDK: <https://github.com/FilenCloudDienste/filen-sdk-ts/tree/v0.4.2>
- Native keyring: <https://github.com/JupiterPi/node-keyring/tree/165e4334ff365792d9b1274761e8afeedcccaffe>
- Node.js: <https://github.com/nodejs/node/tree/v24.21.0>

`AGPL-3.0.txt` contains the helper license. The generated application payload
also contains Node's complete license/notices, the actual runtime dependency
notices, and the runtime SBOM. `BUN-LICENSE.md` remains here for the build-tool
record; no Bun or JavaScriptCore binary is distributed.
