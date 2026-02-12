# Repository Guidelines

## Project Structure & Module Organization
- `src/main.rs` initializes settings/logging and delegates to the library entrypoint.
- `src/lib.rs` contains command orchestration and shared application flow.
- `src/cli.rs` defines the Clap interface; `src/db.rs` handles SQLite persistence; `src/store.rs` owns in-memory cache/search/recent history; `src/interactive.rs` powers live search UI; `src/settings.rs` loads `kvstore.toml`.
- Runtime artifacts (`data.db`, `logs/kvstore.log`, `logs/recent.log`) are local outputs and should not be committed.

## Build, Test, and Development Commands
- `cargo build`: compile the crate in debug mode.
- `cargo run -- <args>`: run locally (example: `cargo run -- search api --tags`).
- `cargo test`: run unit tests (currently focused in `src/store.rs`).
- `cargo fmt`: apply standard Rust formatting.
- `cargo clippy --all-targets --all-features -D warnings`: fail on lint warnings before review.
- `cargo install --path .`: install `kvstore` to `~/.cargo/bin` for local smoke testing.

## Coding Style & Naming Conventions
- Target Rust 2021 style and let `rustfmt` control layout (default 4-space indentation).
- Use `snake_case` for modules/functions/variables, `PascalCase` for types/enums/traits, and `SCREAMING_SNAKE_CASE` for constants.
- Prefer focused functions and explicit error propagation with `Result` and `thiserror` (`KvError` pattern).
- Keep CLI-facing messages clear and stable; document user-visible behavior changes in PRs.

## Testing Guidelines
- Add tests close to implementation with `#[cfg(test)] mod tests`.
- Use behavior-oriented names such as `record_access_persists_recent_history`.
- For filesystem behavior, use `tempfile::tempdir()` and avoid touching real `data.db` or `logs/`.
- Before submitting changes, run `cargo test` and `cargo clippy --all-targets --all-features -D warnings`.

## Commit & Pull Request Guidelines
- Match existing commit history style: short, imperative subjects (for example, `Change commands`).
- Keep commits scoped to one logical change.
- PRs should include: objective, notable design choices, commands executed to validate, and sample CLI output when command behavior changes.
- Link related issues/tasks and call out configuration or migration impacts explicitly.
