# Contributing

Thanks for your interest in improving `wg-tui`!

## Development

```sh
cargo build
WGTUI_DEMO=1 cargo run     # sample data, no root, no real configs
```

Before opening a PR, please run:

```sh
cargo fmt --all
cargo clippy --all-targets -- -D warnings
```

Both are enforced in CI.

## Guidelines

- Keep the privileged helper (`packaging/wg-helper`) as small and auditable as
  possible. New privileged operations should be a new explicit verb with strict
  input validation — never pass caller-controlled paths.
- Prefer pure-Rust dependencies; a key goal of this project is that it builds and
  runs with **no GUI/C library dependencies** and works over SSH.
- Keep the UI keyboard-driven and discoverable (update the `?` help and the
  footer hints when you add a key).

## Reporting bugs

Open an issue with your distro, terminal, `wg --version`, and steps to reproduce.
Never paste real private keys or configs into an issue.
