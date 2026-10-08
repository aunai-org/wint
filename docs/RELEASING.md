# Releasing

How to cut a release of wint and its demo. There are two ways: push a tag and let GitHub Actions publish (below), or do every step by hand ([Manual release](#manual-release)). The manual steps are also the fallback when the workflow fails.

Two things get published from `wint`: the Rust crate `wint` (crates.io) and the WebAssembly package `wint-core` (npm). The demo is released by updating its bundled engine and tagging.

## Automatic release

Pushing a tag like `v0.1.1` runs `.github/workflows/release.yml`. It checks that the tag matches the version in `Cargo.toml` and that the commit is on `master`, runs the full checks and dry-runs both packages. Then it **waits for your approval**, publishes the crate to crates.io and the WebAssembly package to npm, and creates the GitHub Release with generated notes. CI itself already runs on every push and pull request.

One-time setup:

1. **crates.io token:** Account Settings, API Tokens, New Token. Give it the `publish-update` scope and limit it to the crate `wint`.
2. **npm token:** Access Tokens, Generate New Token, Granular. Limit it to the package `wint-core` with read and write access. If your npm account requires two-factor authentication to publish, the token must be allowed to bypass it, or CI cannot publish.
3. **GitHub environment:** in the `wint` repository, Settings, Environments, New environment, named `release`. Add yourself as a **required reviewer**, and add the two tokens as environment secrets named `CARGO_REGISTRY_TOKEN` and `NPM_TOKEN`. Only the publish job can read them, and only after you approve.
4. Optional: Settings, Branches, protect `master` so a pull request needs green CI.

To release:

1. Prepare as in step 1 of the manual release: set the version in `Cargo.toml`, merge to `master`, wait for green CI.
2. `git tag -a v0.1.1 -m "wint 0.1.1" && git push origin v0.1.1`.
3. Open the run under Actions. When `verify` is green, approve the `publish` job.
4. Release the demo as in step 5 of the manual release. Its own `release.yml` runs the tests on a tag and creates a GitHub Release with the single-file page attached; the live site is deployed by the Pages workflow when `main` changes.

If `publish` fails after the crate went out but before npm did, do not re-run the whole job: the crate version can never be published twice, so it would fail again at the first step. Publish the npm package by hand (`scripts/build-wasm.sh`, then `npm publish` in `pkg/`) and create the GitHub Release yourself.

## Manual release

### Before you start (once)

- Accounts: `cargo login` (crates.io token) and `npm login` (npm, with two-factor authentication).
- WASM tools: `rustup target add wasm32-unknown-unknown` and `cargo install wasm-bindgen-cli --version 0.2.129 --locked`.
- Names are free: `wint` on crates.io and `wint-core` on npm (both returned 404 on 2026-10-04). Names are only yours once published, so check again before the first publish.

### 1. Prepare `wint`

1. Start from an up-to-date trunk: `git checkout master && git pull`.
2. Set the version in `Cargo.toml` (`version = "0.1.0"`), run `cargo build` so `Cargo.lock` follows, and commit.
3. Update `ROADMAP.md` if the release changes what is done and what is next.
4. Run the same checks as CI, including the run without features:

   ```sh
   cargo fmt --check
   cargo clippy --all-targets -- -D warnings
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test
   cargo test --all-features
   scripts/build-wasm.sh && node scripts/wasm-smoke.mjs
   ```

5. Push and wait for CI to be green on the commit you are about to release.

### 2. Check what will be published

```sh
cargo publish --dry-run          # packages and builds the crate, uploads nothing
cargo package --list             # the files that go into the crate
(cd pkg && npm pack --dry-run)   # the npm package contents
```

On the last run the crate was 55 files (about 109 KB compressed) and the npm package 7 files (about 157 KB). A big jump means something unwanted got in.

### 3. Publish (not undoable)

Published versions can be yanked but never deleted or replaced. Be sure before you run these.

```sh
cargo publish                    # the crate; install with: cargo install wint --features net
cd pkg && npm publish            # the npm package wint-core (pkg/ is built by scripts/build-wasm.sh)
```

### 4. Tag and announce

```sh
git tag -a v0.1.0 -m "wint 0.1.0"
git push origin v0.1.0
```

Then create a GitHub Release from the tag (Releases, Draft a new release) and write the notes by hand: what it does, what changed, known limits. There is no changelog file yet; the release notes are the changelog.

### 5. Release the demo

1. In `wint-demo`, with the `wint` checkout at the tagged commit: `scripts/update-wasm.sh ../wint`. This rebuilds the engine and updates `vendor/` and `vendor/VERSION`.
2. Run the tests: `npm install`, `npm test` (with `npm run serve` running) and `npm run test:single`.
3. Commit, merge to `main`. The Pages workflow deploys the site on every push to `main`.
4. Tag the demo `v0.1.0` the same way as above.
5. Optional: rebuild the single-file page (`npm run build:single`) and republish the Claude artifact.

### 6. Check it worked

- crates.io and docs.rs show the new version (docs.rs builds the API docs from the code with all features, about ten minutes after publishing).
- npm shows `wint-core`; in an empty folder, `npm install wint-core` works.
- In an empty folder, `cargo install wint --features net` builds, and `wint --list-presets` runs.
- The demo is live and its footer shows the engine version.

## Versions

wint is 0.x: the Rust API can still change between minor versions. The JSON result shape has its own `schema_version` (see the Stability section of the [specification](../SPEC.md)); a change to it is noted in the release notes. The demo uses the same version number as the engine it bundles.
