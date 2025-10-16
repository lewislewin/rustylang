## Contributing

Thank you for considering a contribution!

### Prerequisites
- Rust toolchain (pinned via `rust-toolchain.toml`)
- `cargo fmt`, `cargo clippy`

### Workflow
1. Fork and create a feature branch.
2. Run checks locally:
   - `cargo fmt --all`
   - `cargo clippy --all-targets --all-features -- -D warnings`
   - `cargo test`
3. Open a PR against `main`. CI must pass.

### Commit style
- Prefer concise, imperative messages (e.g., "Add unit tests for json_utils").
- Reference issues when relevant.

### Code style
- Follow `rustfmt.toml` and address Clippy warnings.
- Prefer explicit error contexts via `anyhow::Context`.


