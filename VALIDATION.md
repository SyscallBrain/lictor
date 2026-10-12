# Local validation

Validated on 2026-10-11, with split-workspace regression checks on 2026-10-12.
The initial implementation is licensed under MIT,
Copyright (c) 2026 Tiago Correia.

## Environment

- Linux x86_64; local release build linked against GNU libc 2.43.
- Rust/Cargo 1.99.0, pinned by `rust-toolchain.toml`.
- Herdr 0.9.3, socket API protocol 22.
- mdBook 0.5.4.

The local archive is a build for this host. The prepared release workflow builds
its Linux x86_64 artifact on Ubuntu 24.04. Other operating systems and Herdr
versions have not been validated.

## Automated checks

| Check | Result |
| --- | --- |
| `cargo fmt --check` | Passed. |
| `cargo clippy --all-targets --locked -- -D warnings` | Passed. |
| `cargo test --locked` | 23 unit tests passed, including all 16 split-kind combinations through the controller key flow. |
| `cargo build --release --locked` | Passed. |
| `tests/cli_acceptance.py` against release executable | 35/35 passed. |
| `tests/controller_acceptance.py` against release executable | PTY key modes, interval editing, visible split configuration, pane swapping, select-all, split launch request, settings, detached finite job, and terminal restoration passed. |
| `tests/lifecycle_acceptance.py` against release executable | Pause/resume/stop acknowledgement, registry cleanup, dynamic status, executable identity, and reused-handle safety passed. |
| `mdbook build` and `docs/check_links.py` | Passed; checked local links/anchors across 20 generated pages. |
| `actionlint` | All three prepared workflows passed. |

Fake-socket/PTY tests use isolated private runtime directories. They do not
connect to an existing user session. The test scripts use Python's standard
library and require no Python packages.

## Live Herdr evidence

The owned disposable session `lictor-proof-20261011` contained synthetic source
panes producing `SOURCE-A` and `SOURCE-B`. The protocol proof verified fresh
argv-backed pane creation, two concurrent observers with initial full frames and
subsequent ANSI deltas, and source geometry preservation. The Rust viewer
composed both source panes into a full-tab view.

The coordinator independently tested the release executable with
`tests/split_acceptance.py`. Workspace observation preserved the original active
source tab after viewer creation acquired focus. The strict `-n 1` limit kept the
second lane as a waiting placeholder. Split pause/resume and stop froze the
selected target while retained observation continued to show changing output.
Close-on-exit removed the owned tab. Original source pane geometry and terminal
identities remained unchanged.

A short-limit startup race discovered during final review was corrected: both
viewers acknowledge their initial command, including disabled placeholders,
before their control files can be removed. The release build and live split
acceptance were rerun after the correction.

The disposable proof session was stopped/deleted and its owned records cleaned
up after validation. No default-session server, source pane, or agent was used
for these experiments.

On 2026-10-12, the owned session `lictor-proof-split-90251bf4` reproduced split
creation in the first source workspace despite another workspace being current.
The corrected release executable created its observation tab in the current
workspace. Running `-W -A -s 5 -S w,a` with a finite `-n 6` limit verified both
lanes advancing every five seconds across two workspaces. Synthetic source
processes reported agent identities for this test; no actual agent was started
or sent input. Full-tab views preserved both source panes. The extended live
split acceptance also passed current-workspace creation, both lane advances,
pause/resume, retained live output, close-on-exit, and original geometry and
terminal identities. This disposable session was stopped/deleted after testing.

The split-controller follow-up checked all 16 ordered kind combinations,
selection preservation, pane swapping, and conversion back to a focus queue.
Owned `lictor-proof-tui-*` sessions verified real split launches entirely through
PTY key input: `a,w` with all targets selected and `t,a` with individual targets
chosen through search. Both completed finite rotations, opened in the current
workspace, and displayed the expected source content, including complete
multi-pane tabs. The created views were closed and proof sessions stopped/deleted.

## Practical limits

- Mirrors use the installed Herdr observer CLI for its binary protocol adapter.
- Source terminal dimensions are preserved; smaller viewers crop or pad content.
- Inline Kitty graphics are not forwarded by this observer adapter.
- Sidebar delivery depends on supported local tmux/X11 routes; visibility and
  restoration use the original width heuristic. Unsupported helpers warn while
  core rotation continues. Its adapter/configuration logic was unit-tested;
  this validation did not drive the owner's live desktop/sidebar.
- GitHub workflows are prepared and linted locally. No public repository,
  deployment, Pages site, or GitHub Release was created in this task.

See the book's development chapter for live-test prerequisites and commands.
