# Architecture and development

Lictor has one Rust package and executable. Domain targets/queues are shared by CLI, managed jobs, the controller, and split viewers.

The integration has two surfaces:

1. A native NDJSON client over the Herdr API Unix socket for topology, focusing, and creating owned layouts.
2. An argv subprocess adapter for `herdr terminal session observe`, which converts the matching Herdr binary client protocol into read-only NDJSON ANSI frames.

Each observed source terminal has persistent virtual-terminal state. Full frames establish a baseline; subsequent ANSI deltas are applied in order. Full-tab mirrors compose those terminal screens according to authoritative layout data. Drawing can coalesce rendered states; dropping an intermediate ANSI delta corrupts the baseline.

Focus rotation and split lanes share selection/status/liveness rules. Jobs own created resources and private control state. Linux job controls use identity-checked signals with acknowledgement; split viewers use private files for lane commands and acknowledgements. A stopped split job retains its last observation views as tracked view-only resources until closed.

[How Lictor works](how-it-works.md) walks through a run end to end. In the source tree:

| File | Responsibility |
| --- | --- |
| `src/cli.rs` | Arguments, grouped help, and validation. |
| `src/domain.rs` | Targets and snapshots parsed from Herdr. |
| `src/herdr.rs` | Socket discovery, the NDJSON client, and the observer subprocess. |
| `src/rotation.rs` | Selector resolution and the focus rotation loop. |
| `src/runtime.rs` | Job registry, signals, pause/resume, lane control files, and owned-view filtering. |
| `src/mirror.rs` | Observer frame decoding into virtual terminals. |
| `src/main.rs` | Entry point, split jobs, and the viewer. |
| `src/tui.rs` | The controller. |
| `src/platform.rs` | Notifications and sidebar control. |

Read `IMPLEMENTATION_PLAN.md` in the repository for the agreed behavior and implementation milestones. `CONTRIBUTING.md` describes local development. The application source is organized around CLI, domain, integration, rotation, UI, mirror, and runtime/platform responsibilities.

## Checks

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

Fast tests use synthetic topology/stream fixtures and fake sockets. Controller state tests check stable selection identities and queue validation; PTY checks exercise terminal setup/teardown, modal key flows, and a detached finite job.

Real protocol tests run in a dedicated named Herdr session with synthetic processes. They must never target the owner's default session. An integration proof should verify source PTY dimensions, focus, identities, observer full/delta handling, fresh argv-backed layouts, and owned cleanup.

## Live acceptance scripts

Use an explicitly owned named session beginning with `lictor-proof-`. It must
contain two synthetic source panes printing `SOURCE-A` and `SOURCE-B` repeatedly,
with changing output. Record the source tab/pane IDs from its snapshot. If nested
sessions require `experimental.allow_nested`, put that setting in a temporary
`HERDR_CONFIG_PATH` while creating the test session.

```bash
export LICTOR_PROOF_SESSION=lictor-proof-your-suffix
export LICTOR_PROOF_SOURCE_TAB=RETURNED_TAB_ID
export LICTOR_PROOF_SOURCE_PANE=RETURNED_PANE_ID
export LICTOR_BIN="$PWD/target/release/lictor"
tests/herdr_protocol_proof.sh
python3 tests/split_acceptance.py target/release/lictor
```

The first script verifies fresh argv-backed layout creation, independent observer
streams, geometry/focus preservation, and full-tab composition. The second covers
split creation in the current workspace with both lanes advancing, workspace
active-tab preservation, the one-advance placeholder, pause/resume,
stop with continued live output, and close-on-exit. Their cleanup closes only
returned owned view/job IDs. Stop/delete your owned test session after the run to
remove its remaining synthetic sources and layout proof panes.

## Attribution and compatibility

The Bash reference is pinned to gist revision `4cb57553f6ed8996f0b22416333ed2383e0160fe`. Initial observation research used Herdr `v0.9.3`, protocol 22. The book should describe actual tested behavior rather than assuming future Herdr versions keep internal binary compatibility.

Lictor's observation adapter delegates that binary compatibility to the installed Herdr CLI. It does not embed Herdr source code or claim ownership of source terminals. All Lictor project code/docs use the MIT License, copyright 2026 Tiago Correia.
