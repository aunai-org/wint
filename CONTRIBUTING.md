# Contributing

Thanks for wanting to help. Bug reports, questions, examples from your own field and code are all welcome.

## Ask or report first

- Found a bug or a wrong result? Open an issue with the plan, a few rows of data and what you expected. Small examples are best.
- Want a new feature? Open an issue and describe the job you are trying to do. That is easier to discuss than a finished pull request.
- Security problem? Please do not open a public issue; see [SECURITY.md](SECURITY.md).

## Working on the code

You need a recent stable Rust. The WebAssembly build also needs the setup in [docs/RELEASING.md](docs/RELEASING.md) (only if you touch it).

Before you open a pull request, run what CI runs:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo test --all-features
```

Run clippy and the tests both with and without `--all-features`: some code only compiles under a feature, and CI checks both.

A few things that are specific to this project:

- **Results are data.** The engine returns numbers, tags and names, never display text. Wording, rounding and units belong in the `present` module or in the application. `tests/layering.rs` enforces this, so a `format!` in `src/engine.rs` will fail it.
- **Result shapes are pinned.** `tests/golden/` holds the exact JSON of a result and a plan. If you add a field on purpose, review the diff and regenerate with `WINT_UPDATE_GOLDEN=1 cargo test --all-features --test golden`. Removing or renaming a field changes `schema_version`; see the Stability section of the [specification](SPEC.md).
- **Add a test with behaviour changes**, and update the specification or docs if the behaviour is described there. Examples in `docs/` and the README are checked by tests where they print numbers, so keep them true.
- **Missing readings fail hard limits.** That is deliberate. Changes that would let a missing value pass need a very good reason.

Keep pull requests small and focused, and explain the why in the description. Commit messages in plain English are fine.

## Licence

By contributing you agree that your contribution is released under the [MIT licence](LICENSE), like the rest of the project.

## Conduct

Be kind. See the [code of conduct](CODE_OF_CONDUCT.md).
