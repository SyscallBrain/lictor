# How Lictor works

This chapter follows a run from the command line to the screen. You don't need it to use Lictor,
but it explains why Lictor behaves the way it does: why selections survive a reorder, why closing
the controller doesn't stop a rotation, and why a mirror can never type into your agents.

```text
 you ──► lictor (CLI or controller)
            │  1. snapshot + resolve targets
            ▼
         detached job ──────────── focus / layout.apply ──────────► Herdr socket API
            │  2. schedule                                              ▲
            │  3. lane commands (split mode)                            │ topology
            ▼                                                           │
   ┌──────────────── Lictor mirror tab (owned by the job) ─────────────┴──┐
   │  left viewer                          right viewer                    │
   │   └─ herdr terminal session observe    └─ herdr terminal session ...  │
   │        (read-only stream per source pane)                             │
   └───────────────────────────────────────────────────────────────────────┘
```

## 1. Talking to Herdr

Lictor speaks Herdr's newline-delimited JSON socket API directly; there is no `jq`, Python or
`socat` in between. It finds the socket from `--socket-path`, from `--session NAME`, from the
Herdr environment of the pane it runs in, or from the default session, in that order.

Everything starts with one `session.snapshot`: workspaces, tabs, panes, agents with their status,
and the layout of every tab. Lictor turns that into a list of **targets**, each with a kind
(workspace, tab, pane or agent) and Herdr's own ID, such as `w2`, `w2:t1` or `w2:p1`. Lictor treats
these IDs as opaque handles and never guesses them.

## 2. Resolving a selection

Flags and selectors become an ordered queue against that first snapshot:

- `-W`, `-A`, `-T` select whole groups, in the order they appear on the command line.
- `-p` entries are resolved in those groups: numbers, labels, `W:T.P` positions, agent kinds, or
  typed selectors such as `agent=claude`. See [target selectors](selectors.md).
- Relative positions are anchored to a **baseline** workspace and tab (where Lictor runs, or
  `--target`), captured once, so `1` keeps meaning the same pane while focus moves.

From then on a target is its ID. If you reorder the queue or the session changes, Lictor still
follows the same target. If the target disappears, it leaves the queue; Lictor never substitutes
whatever now sits at its old position. New workspaces or agents are not added automatically.

Validation happens before anything changes in Herdr. A contradiction such as `-T -S w,a` is
rejected with exit code 2 and a hint.

## 3. Jobs

A rotation runs as a **job**: a Lictor process that owns the schedule. The CLI runs the job in the
foreground; the controller starts a copy of the `lictor` executable in its own session, so neither
quitting the controller nor closing its terminal stops the rotation.

Each job writes a private record under `$XDG_RUNTIME_DIR/lictor-UID` with its process identity
(PID, start time and executable), its state, and any Herdr tab it created. Before acting on a
record, Lictor checks that identity, so a recycled PID is never signalled by mistake. This is what
`--jobs`, `--stop`, `--pause`, `--resume` and `--close-view` use. Pausing and resuming are signals
that the job acknowledges by updating its state; stopping lets the job clean up first (restore
the sidebar, release or keep its views) and then notify.

## 4. Focus rotation

At every interval the job:

1. takes a fresh snapshot and drops targets that no longer exist (ending with exit code 3 when
   none are left);
2. picks the next target in the queue, skipping agents whose live status doesn't match `--status`;
3. asks Herdr to focus it (`pane.focus`, `tab.focus` or `workspace.focus`).

Only successful focus requests count towards `-n`. When no agent matches the status filter, the
job waits instead of spinning. With `-r`, the first lap follows your order and later laps are
shuffled, keeping repeated entries.

## 5. Split observation

Split mode never moves focus between your sources. Instead, it builds a place to watch them.

**Creating the tab.** Both lanes are resolved first. Then a single `layout.apply` creates a new tab
in the baseline workspace: a left/right split whose two panes run `lictor __viewer …`, the hidden
viewer mode of the same executable. Lictor only ever creates a *fresh* tab; it never passes
existing tab or pane IDs, so it cannot replace a running process. The job records the tab, pane
and terminal IDs it got back, focuses the new tab, and waits for both viewers to report that they
are ready.

**Driving the lanes.** Each lane has a private control file. At every tick the job picks each
lane's next eligible target, writes it with a new generation number, and waits for the viewer to
acknowledge. A lane change counts towards `-n` only once the viewer has a usable picture. A lane
whose agents don't match `--status` keeps its last view while the other lane moves on.

**Observing.** For each source pane on display, the viewer runs
`herdr terminal session observe PANE` at the source pane's own size. Herdr sends a full frame and
then ANSI updates; the viewer replays them in order into a virtual terminal (one per pane, using
the `vt100` crate). Observation is read-only: Lictor doesn't attach, take control, send input or
resize anything.

**Drawing.** A lane shows a header with what it is showing, for example
`◧ WORKSPACE  api › tab 1` or `◨ AGENT  codex · web`. Below the header:

- An agent or pane fills the lane.
- A workspace or tab is rebuilt from Herdr's layout data: every pane in its relative position and
  size, with a thin border and its title. A zoomed source shows only its zoomed pane.
- When a source is taller than its space, the viewer keeps the **bottom** in view, where the newest
  output and agent prompts are. Wider sources are cropped on the right.

**Keeping up.** Every second the viewer compares the source layout with what it is drawing. If a
pane was split, closed or moved, or an observer ended, it reconnects and starts again from a fresh
full frame. A workspace lane follows that workspace's active tab, skipping Lictor's own tabs, so a
mirror never ends up mirroring itself. Lictor-owned tabs and panes are also hidden from every
target list.

**Stopping.** When the job stops or reaches `-n`, the lanes stop advancing but the tab stays open
and live, recorded as a *view-only* job, until you close it with `--close-view`, `X` in the
controller, or by closing the tab. With `--close-on-exit` the job closes the tab itself, after
checking that it still holds exactly the panes and terminals it created.

## 6. The controller

The controller is a [Ratatui](https://ratatui.rs/) application on top of the same pieces. It
refreshes the snapshot every two seconds and the job list twice a second, keeps your selections
by target ID, and builds the exact command line it would run. Pressing Enter validates that command
line with the same rules as the CLI, then starts it as a detached job. That's why anything you
build in the controller can also be written as a `lictor` command, and the other way round with
`--tui`.
