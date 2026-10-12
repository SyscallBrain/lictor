# Troubleshooting

## Herdr is unavailable

Inspect the selected session and socket rather than restarting a working server:

```bash
herdr session list
herdr status server
lictor --session work -W -l
```

A socket path can exist while its server is unavailable. Client and server versions can also differ after an update. Capability errors identify which required feature is missing. Replacing a running Herdr server stops its terminals, so upgrade/restart it deliberately through your own workflow.

## A target does not resolve

List the relevant group and use the returned ID:

```bash
lictor -A -l -a
lictor -T -l -a
```

Pane/tab indices are relative to the captured baseline context. Workspace labels and agent kinds can collide in legacy mixed selection; use typed selectors. Agent targets require recognized live agents, not just a pane whose title resembles one.

## A status-filtered lane does not advance

Check whether any selected agent currently has an allowed status. Waiting is expected when none match. Observing a completed agent does not directly mark it seen; focus and client viewing policies have different effects on status.

## The mirror looks cropped

The source PTY stays at its original size, so a smaller viewer cannot display every source cell at once. Viewers keep the bottom of a tall source in view (the newest output) and crop wide sources on the right. Make the Herdr window larger, hide its sidebar with `-b hide`, or mirror agents or panes instead of whole workspaces. Layout structure and process preservation are more important than forcing a source application to reflow.

If a mirror shows an older screen than its source, check its header for a `⚠` notice: the viewer reconnects automatically when a stream ends or the source layout changes.

Inline Kitty graphics are not forwarded by the initial Herdr observer adapter.

## Rotation hid the controller

Focus rotation moves Herdr away from the controller's pane; the controller keeps running there. Go back to it to pause (`p`) or stop (`x`) the job, run `lictor --stop` from another shell or your Herdr shortcut, or use `lictor --jobs` to find the job's ID and `lictor --tui --job ID` to reopen a controller on it, which pauses it while you look. The controller and rotation are separate, so leaving the UI does not strand the job.

## Nothing seems to happen after starting a split

The mirror opens in a new tab named `Lictor mirror` in the current workspace and Herdr switches to it. If it doesn't appear, check `lictor --jobs` and the controller's message line; a job that fails to start reports the last line of its log and the log's path.

## Sidebar control is unavailable

On Linux it needs a supported tmux or X11 route. Native Wayland window key injection is not provided by `xdotool`. Rotation and observation still work without sidebar control.

## A retained view is still running

Stopping rotation intentionally keeps its final live views open. Close the generated tab or use the TUI's close-view action. Start future rotations with `--close-on-exit` for automatic view cleanup.

## Reporting an issue

Include Lictor/Herdr versions, the command or TUI sequence, expected/actual behavior, and whether the issue reproduces in a synthetic named session. Remove private terminal output, repository names, prompts, environment secrets, and socket paths you do not want to share.
