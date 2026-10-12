# Lictor implementation plan

Status: initial Linux implementation completed and validated locally. See `VALIDATION.md` for evidence and supported limits.
Prepared on 2026-10-11 for `/home/tiago/Projects/lictor`.

## Objective and confirmed requirements

Build `lictor`, a Rust port of `herdr_rotate`, with a scriptable CLI, a Ratatui TUI, and a new two-pane observation mode. All code, comments, CLI messages, tests, and project documentation must be in English. Conversation with the owner is in European Portuguese.

The owner confirmed:

1. Linux is the initial supported platform. Keep platform dependencies isolated to make a later macOS port possible.
2. Preserve all script features, including sidebar control/restoration, desktop notifications, named sessions, explicit sockets, listing, dry-run, random order, status filtering, and stopping running rotations.
3. Show the actual content of existing terminals in split mode, preserving their processes and original locations.
4. A workspace or tab mirror shows the entire source tab, retaining its pane arrangement. A workspace mirror uses its active source tab by default.
5. `-S w,a` / `--split w,a` creates two actual Herdr panes, side by side: workspaces on the left and agents on the right. `--socket-path` remains available, but the old `-S` socket alias is deliberately reassigned.
6. Both split lanes advance every configured interval, each looping independently through its own selected list.
7. Stopping a split rotation leaves its observation tab open on the last selected targets. Their live output continues. Provide `--close-on-exit` to close Lictor-created views instead.
8. Running `lictor` without arguments opens the TUI. Arguments run the CLI directly; `--tui` opens the TUI with those arguments as initial settings. A non-TTY invocation without arguments must report how to use the CLI rather than hanging.
9. Permit mixed tabs, workspaces, and agents, including `-T -W -A`. This intentionally removes the Bash restriction on combining `-T` with the other groups.
10. Use Vim keybindings and the Tokyo Night theme. Normalize the internal theme identifier to `tokyo_night`; the owner's original spelling was `tokio_night`.
11. Prepare high-quality English documentation using mdBook and a GitHub Pages workflow for a future public release.
12. Any delegated agents must use **`gpt-6-luna` with reasoning effort `medium`**. Do not substitute another model. When spawning, use `fork_turns: "none"` with a self-contained task so the requested override applies.

The owner authorized implementation on 2026-10-11, assigning one `gpt-6-luna`/`medium` worker to application code and the coordinator to documentation, CI, review, and validation. Creating a public repository and publishing the book remain future tasks. Make routine implementation choices autonomously and bring substantive unanswered product decisions to the owner.

## Verified baseline and sources

The workspace is an empty Git repository on `main`, with no commits or remotes at the time of analysis. The locally installed Herdr CLI reports `0.9.3`; its bundled schema reports protocol `22`, schema version `1`. An installed CLI version is not proof of the running server version.

The Bash reference has 881 lines:

- [Original gist](https://gist.github.com/SyscallBrain/e6534601ece6d956e922c589f517078c).
- [Pinned gist revision](https://gist.github.com/SyscallBrain/e6534601ece6d956e922c589f517078c/4cb57553f6ed8996f0b22416333ed2383e0160fe).
- Revision: `4cb57553f6ed8996f0b22416333ed2383e0160fe`.
- SHA-256 of the script file returned by the pinned gist API: `1cee3df1f9754da1d3871dfcf90766a5e705a1ec290c07389391927172eaf8c4`.

Herdr observation was audited against tag `v0.9.3`, resolving to commit `7b116c05bfda646af39d2524c54e70c751f57ee8`:

- [Terminal stream CLI](https://github.com/herdrdev/herdr/blob/v0.9.3/src/client/terminal_sessions.rs): observation request and NDJSON records.
- [Stream encoder](https://github.com/herdrdev/herdr/blob/v0.9.3/src/server/render_stream.rs): initial full frame and subsequent changes.
- [ANSI encoder](https://github.com/herdrdev/herdr/blob/v0.9.3/src/protocol/render_ansi.rs): full redraw and differential rendering.
- [Headless server](https://github.com/herdrdev/herdr/blob/v0.9.3/src/server/headless.rs): observation versus control and resizing.
- [Socket path resolution](https://github.com/herdrdev/herdr/blob/v0.9.3/src/server/socket_paths.rs): API socket versus client socket.
- [Herdr tests](https://github.com/herdrdev/herdr/blob/v0.9.3/src/server/headless/tests/mod.rs): multiple observers and preservation of source terminal dimensions.
- [Current socket API documentation](https://herdr.dev/docs/socket-api/).
- [Current CLI terminal observation documentation](https://herdr.dev/docs/cli-reference/#direct-terminal-attach).
- [Ratatui testing](https://ratatui.rs/recipes/testing/snapshots/) and [terminal restoration](https://ratatui.rs/recipes/apps/panic-hooks/).
- [mdBook continuous integration](https://rust-lang.github.io/mdBook/continuous-integration.html).

Prefer the pinned source and installed schema when current web documentation differs. Resolve compatible crate versions at implementation time and commit `Cargo.lock`.

## What the Bash script does

The script is an orchestrator over Herdr's newline-delimited JSON API, not a terminal emulator. Its initial `session.snapshot` resolves a baseline workspace/tab and selectors into an ordered list of typed IDs. Each visit loads fresh state, validates the next target, reads current agent status, focuses a valid eligible target, and waits.

| Area | Behavior to preserve or deliberately extend |
| --- | --- |
| Defaults | Pane rotation, 3 seconds, unlimited changes; first target is focused immediately. Bare invocation deliberately becomes the TUI. |
| Groups | Panes, tabs (`-T`, `-w`), workspaces (`-W`), agents (`-A`); combined workspace/agent order follows first occurrence in argv. Extend to mixed tab groups. |
| Context | `--target`, then caller tab when `HERDR_*` refers to the same server, otherwise server focus. Named session and socket path are mutually exclusive. |
| Selectors | One-based pane `N`, `T.P`, `W:T.P`; tab `N`, `W:N`; workspace number/name; agent ordinal/name/type; opaque Herdr IDs. Agent type expands to all matching initial agents. |
| Explicit IDs | Workspace/tab/pane IDs can resolve across legacy modes. In agent mode, a pane target must contain a recognized agent. IDs are opaque handles, not inferred counters. |
| Mixed legacy `-p` | In `-W -A`, workspaces are resolved before agents; collisions favor the workspace. Retain this compatibility and offer unambiguous typed selectors. |
| Workspace focus | `-W` alone uses the active tab. Legacy `-W -A` uses each workspace's first tab unless `-c` is present. Split workspace mirrors default to the active source tab instead. |
| Duplicates | Duplicate target entries provide extra dwell time. Do not deduplicate the rotation sequence. |
| Duration | Positive decimal seconds, dot or comma separator; `-n` is a nonnegative integer, zero means unlimited. Reject overflow, nonfinite, and unrepresentable durations. |
| Counting | `-n` counts successful focus requests, including a request to an already focused target; status skips, failures, and removals are not changes. |
| Status | `--status` implies agents; `idle,working,blocked,done,unknown`. Filter agents dynamically; other selected groups are unaffected. No eligible agent means wait rather than exit or busy-loop. |
| Random | Keep duplicates; shuffle at subsequent lap boundaries. The script's first lap is ordered. Make this explicit and test it with an injected RNG. |
| Topology changes | Remove disappeared targets, including all repeated occurrences of their ID; exit 3 when all are gone. The baseline selection does not automatically absorb newly created targets. |
| Moves | A cross-workspace pane move changes its public ID and counts as target removal in compatibility mode. Do not silently follow a replacement occupant. |
| Transport failures | Snapshot failure waits and retries. Focus failure is not counted; reconcile live state before continuing. |
| Listing | `-l`, with `-a` for session-wide scope; list mode exits without starting a rotation. |
| Dry-run | Read-only resolution and preview; default five steps when `-n` is zero. Improve the known Bash mismatch: preview the actual planner's ordering/filter decisions against one stated snapshot. |
| Output | Verbose changes with timestamps; quiet suppresses headings. English output is an intentional migration change. |
| Stop | `-k` / `--stop` stops all active Lictor rotations for the current user, across sessions. Report no running rotation as exit 3. |
| Signals | INT, TERM, HUP stop scheduling, restore sidebar, deregister, then notify. Cancellation must also interrupt a pending transport wait. |
| Sidebar | `toggle/show/hide`, `--sidebar-keep`, initial state/restoration, custom prefix/toggle bindings; external tmux or X11+xdotool support. |
| Exit codes | 0 success/clean stop, 1 general runtime failure, 2 arguments, 3 missing/no targets, 4 unavailable Herdr/socket. |

Known Bash quirks must be documented, not accidentally copied as architectural requirements: dry-run ignores random/status, signal delivery can wait for a five-second socket call, mixed selectors can collide, sidebar visibility is estimated, and `--stop` checks process names through `/proc`. The Rust port preserves the features while correcting these deficiencies as specified below.

## Integration approach and feasibility gate

Use two distinct output strategies sharing the same domain model and selection engine:

1. **Focus rotation:** JSON requests such as `pane.focus`, `tab.focus`, `workspace.focus`, preserving the script's semantics.
2. **Split observation:** create a tab and two Herdr panes containing Lictor viewer processes. Each viewer displays existing source terminals through read-only observers; rotation selects what is mirrored without focusing source targets.

For stable JSON operations, implement a native Rust Unix socket client. This removes `jq`, Python, and `socat` from runtime requirements. For terminal observation, initially invoke the installed **`herdr terminal session observe TARGET`** CLI as an argv subprocess. It bridges Herdr's internal binary client protocol to NDJSON, avoiding an independent binary-protocol implementation or a fork of Herdr.

The installed schema also supports `layout.apply` with a fresh `workspace_id`, a split root, and argv `command` arrays on pane nodes. Prefer this to launch the two viewers as a new tab in one operation. Always omit replacement `tab_id` and existing source `pane_id` values: supplying a replacement tab can close live original processes. Validate this fresh-tab creation path in milestone 0.

Verified properties and constraints:

- Observation records include `type: "terminal.frame"`, `seq`, `encoding: "ansi"`, `width`, `height`, `full`, and base64 `bytes`.
- After the first full redraw, bytes are ANSI updates. Maintain a persistent virtual terminal per stream; frames must be applied in order. Reset/rebootstrap after reconnect or dimension change as required by the actual frame encoding.
- Observe `--cols/--rows` sets an observer viewport. It does not resize the source PTY or transfer control authority. Source applications cannot reflow to the smaller mirror size.
- Multiple read-only observers can watch one terminal. Do not use `attach`, `control`, `--takeover`, input forwarding, or source focus as a fallback.
- There is no observer heartbeat. A quiet stream is healthy. Handle `terminal.closed`, clean EOF, child failure, and shutdown separately; a closed record is not guaranteed on EOF.
- The CLI does not forward Kitty graphics messages. V1 mirrors textual terminal content, colors, cursor effects, and terminal UI layouts; it does not promise inline graphics fidelity.
- Observation is per terminal, not per tab. Reconstruct a full tab from topology/layout metadata and one observer per visible source pane.
- Observation does not directly mark the source agent completion seen; focus commands do. Status filters in mirror mode must follow the authoritative server status without assuming that observation consumes `done`.
- Observer socket resolution differs from the JSON API socket. Pass a matching session or carefully selected `HERDR_SOCKET_PATH` to the subprocess, accounting for inherited context. Never send binary stream requests to the API socket.

**Milestone 0 must prove these claims in an isolated named Herdr session before implementing the full UI.** Check client and server compatibility independently. Confirm one and then multiple observers, initial/delta frames, source sizes, no input/focus changes, source status, explicit socket routing, and layout changes. Use synthetic shells/counters and a simple alternate-screen app, not the owner's active agents.

If a feature is unavailable, produce an actionable capability error. Do not upgrade/restart the user's active Herdr server, move source panes, rerun agents, or substitute screenshots/list dashboards for the requested live mirror. Stop dependent mirror work and consult the owner if the isolated proof fails.

## CLI contract

Keep the useful Bash flags and long names. Add descriptive aliases where helpful, such as `--targets` alongside `--panes`, since targets can be several kinds.

```bash
# Selection UI
lictor
lictor --tui -W -A -s 5

# Compatibility-oriented focus rotation
lictor -p 1,2 -s 3
lictor -T -p 1,2,4 -s 5
lictor -W -A -s 5
lictor -A -W -s 5 -c
lictor -A -p claude --status done,blocked
lictor -T -W -A -s 5
lictor -A -l -a
lictor -W -A -d -n 8

# Actual Herdr observation tab: left=workspaces, right=agents
lictor -W -A -s 5 -S w,a
lictor -S w,a -s 5 --left-targets '1,3' --right-targets 'claude,codex'
lictor -S t,a -s 5 --left-targets 'w1:t1,w2:t2' --right-targets 'reviewer'
lictor -S w,a --socket-path /path/to/herdr.sock
lictor -S w,a --close-on-exit -n 20

# Existing shortcut use case
lictor --stop
```

Specific rules:

- `--split` accepts exactly two ordered kinds: `w` workspaces, `t` tabs, `a` agents, `p` panes. Support either order; the physical split is a vertical divider, left/right. New repeated-kind lane combinations may use distinct selections.
- Split kinds imply required groups; explicit group flags can agree with them but must not request an undisplayed kind. Validate contradictions before any mutation.
- `--left-targets` and `--right-targets` are resolved in their own lane contexts. Explicit pane/tab/workspace IDs are validated against the lane's declared kind. Split `-p` is rejected with a clear instruction to use the lane flags; its mixed meaning would be ambiguous.
- Add typed selectors for new mixed CLI selection: `workspace=NAME_OR_ID_OR_NUMBER`, `tab=ID_OR_WORKSPACE:NUMBER`, `agent=NAME_OR_TYPE_OR_ORDINAL`, `pane=ID_OR_LEGACY_LOCATION`. The discriminator uses `=` so it does not collide with Herdr IDs. Do not redefine existing untyped strings silently.
- Group order in focus mode follows first occurrence of flags, including `--status` implying the agent group at its position. Keep old `-W -A -p` precedence; require typed selectors for new mixed combinations containing tabs.
- `-c` retains the legacy mixed workspace focus meaning. Its effect in new mixed focus groups containing workspaces is the same. Reject it when there is no workspace group or when it adds no valid meaning. Split workspace lanes already use active source tabs and need no `-c`.
- Scope default tab discovery to the baseline workspace, as before; `-a` retains its listing meaning. Cross-workspace tab selection is explicit or available through the TUI.
- In split mode, `-n` counts **successful lane advances**, summed across lanes. The initial presentation counts, and a repeated target counts as extra dwell. With two lanes, a tick can contribute two changes. An odd remaining limit updates lanes left-to-right until reached. State this difference from elapsed ticks in help/book.
- Precedence: defaults, explicit CLI settings, then interactive edits. No automatic startup merely because a TUI opens with flags.
- `--dry-run` must never create tabs/panes, register active jobs, inject sidebar keys, or start observers. Show selected IDs, output mode, lane arrangement, proposed commands/resources, and a bounded preview. Explain that future dynamic statuses cannot be predicted.
- Provide minimal job control (`--job ID`, `--pause`, `--resume`) and a TUI close-view action to support returning to a hidden controller and cleaning up retained views. Keep internal viewer/job subcommands hidden from normal help.

## TUI behavior

Use Ratatui with a Crossterm backend. One UI screen presents selectable target categories and a rotation queue/settings area. Display workspace/tab/pane ancestry, stable IDs, agent kind/name/status, and selected count. Distinguish the category widgets from Herdr tabs in labels.

| Key in normal mode | Action |
| --- | --- |
| `j`, `k` | Move through targets; arrows may be aliases. |
| `h`, `l` | Change category or active split lane. |
| `gg`, `G` | First/last row. |
| `Ctrl-d`, `Ctrl-u` | Half-page movement. |
| `Space` | Toggle highlighted target. |
| `v` | Enter visual range selection; Space applies the range. |
| `/` | Enter search mode; `n`/`N` next/previous result in normal mode. |
| `a`, `u` | Select all visible results / clear selection in the current category/lane. |
| `J`, `K` in queue | Move a queue entry down/up. |
| `i` | Edit interval/settings in input mode. |
| `s` | Configure output mode and split lane kinds. |
| `Enter` | Start the reviewed selection or resume a paused job. |
| `p` | Pause/resume a selected running job. |
| `?` | Help showing actual current-mode bindings. |
| `Esc` | Leave search/input/visual mode or cancel a local action. |
| `q` | Leave the controller; an already launched job continues. |
| `Ctrl-c` | Stop the selected active rotation, restore terminal state, and exit controller. |

Use explicit normal/search/input/visual modes: text entry must not trigger normal-mode actions. Empty selection cannot start. Selection identity uses typed IDs and remains stable while snapshots refresh; removed targets are indicated/pruned, and reordering does not substitute a different target at the old index. Default TUI selection is explicit; new arrivals are not auto-selected. Queue editing may insert a duplicate to provide extra dwell, preserving CLI semantics.

Theme: Tokyo Night background `#1a1b26`, surface `#24283b`, text `#c0caf5`, muted `#565f89`, blue `#7aa2f7`, purple `#bb9af7`, green `#9ece6a`, yellow `#e0af68`, red `#f7768e`. Status must have text as well as color. Show compact layouts on small terminals, readable truncation, selected markers, and local error messages.

**Focus and controller visibility:** focus rotation can hide the pane running the TUI. Do not keep an inaccessible foreground key loop as the only way to stop the rotation. Separate the controller from the managed job. On launch, restore the controller terminal and hand off to the job; report its ID and stop/reopen commands. Reopening the TUI for an existing job pauses that job before editing, preventing focus theft while selecting; resume requires the user's explicit start action. Use local IPC for pause/resume/status, not keystrokes into source agents.

## Split observation behavior

1. Resolve and validate every source target **before** creating the observation tab; capture the baseline workspace and each relevant source tab.
2. Create the new tab in the baseline workspace, with a clear `Lictor` label and an ownership record. Use fresh `layout.apply`: root direction `right`, ratio 0.5, and two pane nodes whose `command` arrays launch the exact current Lictor executable with internal viewer arguments. Do not set replacement `tab_id` or source `pane_id` fields. Obtain all owned IDs from the response, and gate viewer startup on the job handshake. If fresh argv-backed layout creation is unsupported, report the capability error and consult the owner about compatibility rather than switching to shell input. Never inject viewer commands into source terminals.
3. Keep the generated tab/panes out of every Lictor discovery/rotation path, including nested workspace mirrors. Exclusion uses owned IDs/metadata, not a name substring. If focusing the generated tab makes it that workspace's active tab, retain the last eligible source tab; never mirror the observation tab itself.
4. Each viewer receives a lane definition and only reads source terminal streams. Agent/pane targets render one terminal. Tab/workspace targets render every pane in the source tab using its layout/split tree and labels. Source IDs remain opaque.
5. For workspaces, track the most recently active eligible source tab. Initially snapshot this before view creation. Later manual activation of another eligible source tab updates it; activation of any Lictor-owned tab does not. Closing it selects the next eligible source tab deterministically or reports no valid source tab.
6. Preserve relative pane placement and split proportions using topology/layout snapshots, including zoom state as a source presentation fact. Reconcile new/closed/moved panes and changed layouts in selected tabs. This differs from selection membership: new panes within an already selected tab can be displayed, but new target tabs/workspaces/agents are not automatically added to the rotation queue.
7. Parse each ordered ANSI stream into a persistent virtual terminal and draw its cells into a clipped Ratatui region. Do not dump source ANSI bytes directly to the outer terminal. Evaluate `vt100` and `tui-term` compatibility; a small custom cell widget is acceptable if dependencies do not align.
8. When source content is wider/taller than its region, crop or pad without source resize or invented text reflow. Provide local viewport navigation and zoom; label cropping visibly. Retain the source layout's structure rather than pretending arbitrary tiny cells can show the complete original screen.
9. Give each lane its own sequence position, status eligibility, RNG state, and target lifetime. Use a shared monotonic clock; both eligible lanes advance at each interval, wrapping independently. If one has no matching-status agents, leave its last view visible with a waiting indicator while the other continues. If its targets vanish, show an empty lane while the other remains operational; if all lanes lose all targets, end rotation with code 3.
10. Bound queues and render rates. Never discard an ANSI delta and apply a later delta on the wrong baseline. Decode/apply in order or reconnect for a new full baseline; only coalesce completed screen states for drawing. Limit active observers to the currently presented tab panes and release previous observers on lane changes.
11. A target advance is successful when viewers have a usable initial frame or validated cached screen for the new presentation. Observer setup failure does not consume `-n`. If the same target repeats, preserve its stream and count another dwell entry.
12. On stop or finite completion, stop lane advancement and restore sidebar before notification. Retain viewer processes and read-only streams for the last targets; record them as view-only resources, not active rotations. Do not leave untracked orphan processes. Closing the retained tab or a TUI close-view action releases them.
13. With `--close-on-exit`, close only the tab/panes created by that job and release their viewers/observers. Failed creation must roll back only newly created resources. Treat manual tab/pane closure, child crash, parent loss, API disconnect, and source disappearance explicitly. Never close or move original source panes.

## Architecture and ownership boundaries

Use one Cargo package with a reusable library and one executable. Avoid a multi-crate workspace until a real boundary requires it.

| Module | Responsibility |
| --- | --- |
| `src/main.rs`, `src/cli.rs` | Public arguments, hidden process roles, startup mode, exit mapping; all CLI validation precedes mutations. |
| `src/domain/` | Typed target IDs, topology, selectors, ordered queues, lane/job configuration; no terminal or socket side effects. |
| `src/herdr/` | NDJSON API transport, typed wire models, session/socket resolution, capabilities, live state reconciliation, observation subprocess adapter. |
| `src/rotation/` | Clock-driven planner, dynamic status/liveness, duplicates/random/iterations; shared by CLI and TUI/job runner. |
| `src/runtime/` | Job ownership, local IPC, registry, signals, cancellation, child supervision, retained-view cleanup. |
| `src/mirror/` | Per-source VT state, stream decoding, full-tab composition, clipping/local viewport controls. |
| `src/tui/` | Selection state machine, keymaps, Tokyo Night theme, drawing, controller job editing. |
| `src/platform/` | Linux sidebar discovery/key injection, desktop notifications, secure runtime paths. |
| `tests/`, `tests/fixtures/` | Fake socket/stream fixtures, planner/parser tests, TUI and isolated integration tests. |
| `docs/`, `book.toml`, `.github/workflows/` | Implementation records, mdBook source, validation and future publishing. |

Suggested dependency families: `clap`, `serde`/`serde_json`, `tokio` with only needed IO/time/signal/process features, Ratatui/Crossterm, `thiserror`, `rand`, `toml`, `base64`, and a tested VT parser/render adapter. Use fake clocks and RNG injection. Avoid an unrelated network daemon or a Herdr source dependency; workers communicate through user-local Unix sockets.

JSON transport must handle partial/multiple lines, request IDs, bounded response sizes, read/connect deadlines, EOF, server error objects, malformed responses, unknown fields, and cancellation. Isolate event subscription IO from request/response IO. Snapshot polling at rotation steps is sufficient for compatibility; use event invalidation plus authoritative snapshots for UI/live layout freshness, with periodic polling fallback. Events are not a replayable replacement for snapshots.

## Linux lifecycle, sidebar, and notifications

- Store registry/config/IPC under `$XDG_RUNTIME_DIR/lictor` when suitable, else a user-specific private directory in the temp location. Directory mode 0700 and private files/sockets; reject unsafe ownership/symlinks. Avoid a world-writable predictable PID marker scheme.
- Record job ID, executable identity, process start identity, socket/session scope, owned Herdr IDs, state, and child relationships. Prefer authenticated user-local IPC for `--stop`; never signal an arbitrary reused PID solely by name.
- `--stop` addresses all active rotations for the user, preserving the old scope; `--job ID` narrows it. Retained view-only resources are visible for cleanup and are not reported as still rotating.
- Signal handling cancels timers and pending IO, prevents further focus/selection commands, restores resources once, and delivers completion/stop acknowledgements only after restoration. Do not issue a speculative second mutation after an uncertain API result; reconcile IDs/state first.
- Parse Herdr key configuration as TOML, including supported string/list bindings. Detect tmux client pane targets and X11 windows as in the reference, using argv execution. Keep key-name conversion and external senders independently testable.
- Prefer an authoritative sidebar capability if actually available. For Herdr 0.9.3, preserve the external key-injection approach with its limits: visibility is a width heuristic, multiple clients can race, and unknown initial state cannot guarantee exact restoration.
- On Wayland without tmux or X11 key injection, rotation still works and an explicit sidebar request produces a clear unsupported-capability warning. Do not change unrelated desktop settings or claim reliable sidebar support.
- Preserve desktop stop notifications via `notify-send` when available and `--no-notify`. Emit them only after rotation cessation and restoration. Missing optional helper is nonfatal. Herdr's own notification API can be an explicit additional adapter, not a silent replacement for desktop behavior.
- Core rotation/mirroring requires Rust binary plus running Herdr; observation currently also requires a compatible local Herdr CLI. `notify-send`, `tmux`, and `xdotool` are optional capabilities. Document this honestly rather than claiming the program has no runtime dependencies.

## Execution milestones and acceptance gates

### 0. Protocol and mirror proof

Owner: integration worker; only experimental adapter/test fixtures and a concise feasibility record.

Retrieve pinned reference sources. Use a small disposable Rust harness if needed before the production scaffold. Capture sanitized synthetic schema/snapshot/stream fixtures. Check running test server/CLI metadata and method support. Demonstrate fresh argv-backed layout creation and two side-by-side read-only viewers, including a composed tab with at least two source panes and a delta-based alternate-screen stream. Ensure focus, PTY sizes, source processes, and status are preserved. Prove explicit socket/session routing, quiet-stream survival, and viewer teardown. Record unsupported terminal graphics and small-viewport limits. This gate blocks the full mirror milestone if it fails.

### 1. Project scaffold and frozen interfaces

Owner: coordinator.

Create `Cargo.toml`, lockfile, toolchain/MSRV decision, library/binary skeleton, formatter/linter setup, and development README. Define domain types, API adapter traits, planner commands/events, lane/viewer messages, job states, and exit mapping before parallel file ownership begins. Add minimal CI formatting/build/tests and an mdBook skeleton. Write the exact CLI compatibility table and intentionally changed behavior from this plan into a migration page.

Acceptance: reproducible `cargo build --locked`, an executable `--help`, and agreed shared interfaces. No actual rotation required yet.

### 2. Native API adapter and headless feature parity

Owners: integration worker (`src/herdr/`) and rotation worker (`src/domain/`, `src/rotation/`, argument definitions under coordinator ownership).

Implement socket/context/session discovery, live topology, selector grammar, ordering, duplicates, listing, status filtering, random laps, dry-run, iteration counting, and focus actions. Expand mixed target groups deliberately. Implement stop/signals and English diagnostics through runtime interfaces. Validate against synthetic snapshots and fake servers first.

Acceptance: every supported Bash use case has a defined equivalent; legacy defaults with explicit arguments match the compatibility table; invalid invocations make no mutations; dry-run is bounded and side-effect free; cancellation works during timers and socket waits.

### 3. Complete lifecycle and platform adapters

Owner: runtime/platform worker (`src/runtime/`, `src/platform/`).

Implement private registry/IPC, foreground and detached jobs, controlled process supervision, pause/resume, reliable `--stop`, sidebar control/restoration, and desktop notifications. Include view-only resources in ownership design before split implementation. Distinguish clean stop from cleanup failure in diagnostics.

Acceptance: stale/reused PIDs are never signalled; cross-session stop scope is covered; restoring terminal/sidebar happens before stop acknowledgement/notification; optional helper failures are explicit and nonfatal where appropriate.

### 4. Ratatui selection/controller UI

Owner: TUI worker (`src/tui/`); consume frozen interfaces only.

Build category/lane selection, queue order/duplicates, search/input/visual modes, interval/status settings, mixed targets, job selection, split setup, Tokyo Night colors, and responsive layout. Implement launch/handoff and reopen/pause behavior. UI refresh must preserve selections by identity.

Acceptance: the user can select named tabs, workspaces, and agents without memorizing indices, launch either output mode, regain control, pause/change/resume, stop, and close retained views using keyboard-only flows. Terminal raw mode, cursor, and alternate screen are restored on error/panic/signals.

### 5. Production split observation

Owners: mirror worker (`src/mirror/`) and integration/runtime owners for their existing modules.

Turn the milestone-0 proof into supervised observation processes, ordered frame decoding, full-tab composition, source layout updates, local zoom/navigation, two independent lane queues on shared ticks, and ownership-aware tab creation/cleanup. Integrate exclusion of all managed surfaces and active-source-tab tracking before enabling workspace mirrors.

Acceptance: `lictor -W -A -s 5 -S w,a` creates an actual two-pane observation tab. The left pane cycles full workspace source tabs; the right cycles selected agents. Both update every five seconds, and original panes/agents continue at their original locations/sizes. Stops retain last live views; close-on-exit closes only created resources. No recursive mirroring or leaked observers.

### 6. English book and public-release preparation

Owner: documentation worker (`README.md`, `docs/`, `book.toml`, documentation workflow); coordinate shared workflow files with the coordinator.

Prepare chapters: introduction, requirements/install, quick start, focus rotation, TUI/keybindings, selection grammar, split examples, complete CLI reference, configuration/session/socket precedence, agent statuses, job lifecycle/stop/retained views, sidebar/platform limits, troubleshooting, migration from Bash, architecture, testing, and contributing.

Use synthetic examples for screenshots/recordings. Build with `mdbook build`; test code examples where applicable, check internal links, and ensure the book works under a project Pages subpath. Prepare Actions build/upload/deploy with least required permissions, deployment concurrency, and immutable pinned action/tool versions. Pull requests build the book; deployment is limited to the publishing branch/manual trigger. CI must not require the owner's token, private config, live session, or gist.

Prepare a release workflow for Linux x86_64 first, with locked builds, archive/checksum artifacts, installation instructions, and a documented Herdr compatibility matrix. Additional architectures depend on an actually verified build. Choose the project license with the owner before the public release; preserve attribution for the original script and any borrowed upstream code.

Acceptance: the book builds locally/in CI and covers every public flag and important behavior; publishing configuration is ready. Creating the GitHub repository, enabling Pages, tagging/releasing, and publishing are future owner-requested steps, not part of this planning task.

### 7. Integration review and delivery

Owner: coordinator; a `gpt-6-luna`/`medium` reviewer may review read-only.

Run the validation suite below, fix regressions, and verify the exact documented examples in an isolated session. Review cancellation, job/view ownership, recursion exclusions, source dimensions, CLI/TUI compatibility, and sidebar limitations. Report outcomes and material limitations in Portuguese, with artifact paths and repeatable instructions. Do not claim features passed based only on fake tests when real Herdr behavior remains unverified.

## Validation matrix

| Layer | Required meaningful checks |
| --- | --- |
| Parser/resolver | One-based selectors, names/types, opaque IDs, explicit context, duplicate entries, status placement, old mixed collisions, new typed mixes, socket/session exclusivity, decimal durations, contradictory split flags. |
| Planner | Ordered first lap/random later laps, frozen selection/live status, filtered wait, successful-only counts, repeated dwell, removals/all gone, independent split wrapping and total advance cap. Use virtual time. |
| JSON transport | Partial lines, multiple records, correlation IDs, unknown fields, errors, malformed/oversized replies, EOF/timeouts, shutdown during pending IO. |
| Stream/render | Full then delta frames, parser reset/reconnect, sequence validation, dimensions, ANSI colors/cursor/alternate screen, Unicode/wide cells, EOF without closed record, quiet output, safe bounded queues, cropped/full-tab layouts. |
| Runtime | Cancel during sleep/read/create; no post-stop selection; process identity/stale registry; stop across sessions; controller handoff/reopen; retained view ownership; child crash/manual generated tab closure; uncertain mutation reconciliation. |
| Sidebar/notifier | Mock key sender/client discovery, TOML key lists, tmux/X11 translations, show/hide/toggle, known/unknown restoration, keep/no-notify, helper unavailable, notification only after restoration. |
| TUI | Key mode transitions, search/visual/select-all, selected IDs after reorder/disappearance, mixed queue/lane setup, empty selection, small terminal, Tokyo Night styles, launch and regain-control flow. Use TestBackend plus a small PTY end-to-end set. |
| Isolated Herdr | Real focus and split behavior, multiple panes/tab mirror, agent-status fixtures, no source process duplication/move/input/resize, named/explicit socket routing, no recursion, managed cleanup/retained observation. |
| Project/book | `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --locked`, `cargo build --release --locked`, `mdbook build`, book links and Pages subpath. |

Only experiments in an owned named session may create or close test Herdr resources. Track IDs returned by the API, keep a bounded cleanup record, and stop only the test server created for that experiment. Do not run rotation against the owner's active session as a test.

## Delegation instructions for the executing agent

Keep the protocol proof and shared interfaces sequential. Afterward, parallel work is possible along the ownership boundaries above; do not create two owners for the same shared interface/workflow file. This environment has four concurrent agent slots including the coordinator.

Every worker task must name its files/responsibility, state that the worker is not alone in the codebase, forbid reverting others' edits, require adaptation to concurrent changes, and provide interfaces/acceptance tests relevant to its work. All spawned workers and reviewers must use `gpt-6-luna`, effort `medium`, with a self-contained prompt. Integration, runtime/platform, and TUI/mirror work should be staged as needed to fit the slots; documentation can follow frozen CLI behavior. The coordinator owns shared files and final review.

Suggested execution prompt:

> Implement Lictor in `/home/tiago/Projects/lictor` according to `IMPLEMENTATION_PLAN.md`. Read applicable local instructions first. Speak to the owner in European Portuguese; write all project artifacts in English. Begin with the isolated Herdr protocol/mirroring proof, then freeze domain/job/adapter interfaces. Preserve every listed Bash feature and apply the explicitly confirmed CLI/TUI/split changes. Delegate only to `gpt-6-luna` with `medium` reasoning and explicit file ownership; workers share the codebase and must not revert others. Ask the owner when evidence reveals a substantive unresolved product choice, but continue independent authorized work. Finish tested CLI, TUI, real two-pane mirrors, documentation and local release artifacts. Public GitHub creation/publication is a later explicit task. Never modify the owner's active source panes or restart its Herdr server for tests.

## Questions reserved for the public release

No unresolved product question currently blocks the planned implementation. The owner selected the MIT License with copyright in the name of Tiago Correia. Before public publication, confirm the repository location/name, first version number, and permission to enable Pages/publish a release. Recheck any capabilities that fail milestone 0 with the owner before committing to a substitute.
