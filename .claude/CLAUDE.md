# Rust conventions

- Use Rust edition 2024. Run `cargo fix --edition` before changing older editions in `Cargo.toml`.
- Apply `rustfmt.toml` before formatting: 131 columns for lines and width heuristics, with compressed function parameters.
- Group imports into as few single-line `use` statements as fit within 131 columns. After formatting, combine same-crate leftovers with another fitting statement.
- Run `kitty-review` before committing each changeset.
- Add tests only for high-value behavior and failure paths. Use existing checks for routine dependency and formatting changes.
- Read `.github/workflows/ci.yml` and `flake.nix` for the build and validation commands.
