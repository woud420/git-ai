# Rust CI setup

After checkout, use this action to install Rust in the current job:

```yaml
- uses: ./.github/actions/setup-rust
  with:
    toolchain: msrv
    components: clippy
```

Every caller explicitly chooses `msrv` or `stable`. `msrv` reads the literal
`[package].rust-version` in the checked-out `Cargo.toml`; a two-component version
is normalized to patch zero. `stable` tracks the latest stable compiler for
forward-compatibility coverage and is not evidence of minimum-version support.
Keep both kinds of jobs when changing CI. The pinned installer action revision
lives here; it does not pin the compiler selected by `stable`.

`components` and `targets` pass through to the installer. Caching, job conditions,
and build/test commands remain with the caller. Docker release builds use
`bash .github/actions/setup-rust/resolve.sh msrv` before installing Rust inside
the container; installing a host toolchain would not configure that container.

The resolver needs only Bash and awk. Unsupported manifest forms fail closed;
the policy test cross-checks its result with Cargo's TOML metadata. Its isolated
tests use only the Python standard library and also run in the lint matrix:

```sh
python3 -m unittest discover -s .github/actions/setup-rust -p 'test_*.py'
```
