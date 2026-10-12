# Contributing

Use English for code, comments, CLI messages, tests, documentation, and pull requests. Preserve terminal processes and respect resource ownership when changing integration behavior.

Build and check from the repository root:

```bash
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo build --release --locked
python3 tests/cli_acceptance.py target/release/lictor
python3 tests/controller_acceptance.py target/release/lictor
python3 tests/lifecycle_acceptance.py target/release/lictor
mdbook build
python3 docs/check_links.py book
```

Commit `Cargo.lock` for reproducible application builds. Update help, book examples, and migration notes whenever public CLI behavior changes. Make substantive behavior tests use synthetic topology and fake streams rather than the contributor's private session.

Real Herdr experiments must run in a dedicated named test session. Use a temporary configuration if nested sessions require it. Keep explicit returned IDs and clean up only resources created by the experiment. Never stop another user's Herdr server or close/move source panes to make a test pass.

For mirrors, preserve ordered ANSI deltas and source PTY/input authority. Quiet streams are not dead streams. Validate reconnects, original process identities, full-tab layout changes, and exclusion of every Lictor-owned viewer surface.

For jobs, test cancellation during timers/socket requests and ensure no further rotation occurs after stop acknowledgement. Sidebar restoration and notifications must finish in the correct order. Retained views must stay tracked and have a clear cleanup route.

For TUI changes, cover key modes, selection identity during refresh, small terminals, and cleanup of raw mode/cursor/alternate screen. Verify usability with the actual terminal as well as the controller state tests.

## Automated reviews and dependency updates

Every pull request gets an AI review from [CodeRabbit](https://www.coderabbit.ai/), configured in `.coderabbit.yaml` with this project's rules. While the repository is small, reviews may need a manual start: comment `@coderabbitai review` on the pull request. Treat its findings like any reviewer's: fix what is right, and reply when you disagree. You can also ask it questions by mentioning `@coderabbitai`.

[Dependabot](https://docs.github.com/en/code-security/dependabot) (`.github/dependabot.yml`) opens weekly pull requests for Rust crates and the SHA-pinned GitHub Actions; minor and patch bumps are grouped into one pull request. Security updates for vulnerable crates arrive as soon as an advisory is published. Dependency pull requests must pass CI like any other; major version bumps may need code or documentation changes.

A pull request should explain the concrete problem, resulting behavior, relevant validation, and remaining limitations. Do not include private terminal transcripts or authentication data in fixtures or screenshots.

By contributing, you agree that your contribution is distributed under the project's MIT License.
