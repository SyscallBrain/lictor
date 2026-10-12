# The selection TUI

Run `lictor` without arguments, or add `--tui` to initialize the interface with CLI settings:

```bash
lictor --tui -W -A -s 5
```

Opening the interface does not start a rotation. Select targets, review the queue/settings, and start deliberately with Enter. A redirected, noninteractive invocation must use CLI arguments instead.

![The controller in split mode](images/tui-select.png)

The screen is a board of two panels that matches what will run. In **focus** mode, a target browser sits on the left and the rotation queue on the right. In **split** mode, the left and right panels are the two lanes of the observation tab, side by side as they will appear. Narrow terminals stack the panels. A Jobs panel appears below them while jobs exist.

![The controller in focus mode: targets on the left, the rotation queue on the right](images/tui-focus.png)

The header shows the mode, interval (`⏱`), iteration limit (`↻`), active filter and job count. The footer shows the current mode (NORMAL, VISUAL, SEARCH, EDIT) and only the keys that matter in the focused panel. Press `?` for every binding, grouped by purpose.

Rows show the target's name, its workspace and, where Herdr reports one, an agent status with icon and text. Selection follows the target ID when the list refreshes or changes order; a disappearing target is not replaced by whatever takes its row position. `◆` marks the currently focused Herdr target, and `#n` marks a target's position in its lane.

![The grouped keybinding help](images/tui-help.png)

| Key | Normal-mode action |
| --- | --- |
| `j`, `k` | Next/previous row. |
| `h`, `l` | Left/right panel (lane in split mode). |
| Tab, Shift-Tab | Cycle panels, including Jobs. |
| `gg`, `G` | First/last row. |
| `Ctrl-d`, `Ctrl-u` | Half-page movement. |
| Space | Toggle a target; in the focus queue, remove it. |
| `v` | Select a range; Space applies it. |
| `/` | Filter rows; Enter keeps the filter, Esc clears it. |
| `a`, `u` | Select visible rows / clear the panel's selection. |
| `m` | Switch between focus and split mode. |
| `c`, `C` | Change the panel's kind (workspaces, tabs, agents, panes). |
| `s` | Swap the split lanes. |
| `J`, `K` | Reorder the focus queue. |
| `t` | Set the rotation interval in seconds. |
| `i` | Edit settings: iterations, random order, statuses, sidebar, notifications. |
| Enter | Start the selection; in Jobs, resume the job. |
| `p` | Pause/resume the selected job. |
| `x`, `X` | Stop a job / close its retained observation tab. |
| `R` | Refresh live targets. |
| `?` | Show grouped keybinding help. |
| Esc | Leave visual/search mode, clear the filter, or dismiss a message. |
| `q` | Leave the controller; launched jobs continue. |
| `Ctrl-c` | Stop the selected job and leave the controller. |

Search and text fields use their own modes. For example, typing `q` into search does not quit.

Press `t`, replace the value (Ctrl-u clears it), and press Enter to save or Esc to cancel. Use positive seconds, including decimals such as `0.5` or `0,5`. Invalid values keep the editor open with an error and preserve the previous interval. This setting is equivalent to CLI `-s` and applies when you start a new rotation; it does not change an existing job's interval.

To create a split tab entirely from the TUI, equivalent to `lictor -W -A -s 5 -S w,a`:

1. Press `m` for split mode. The lanes start as workspaces (left) and agents (right).
2. Focus a lane with `h`/`l` and press `c` to change its kind if needed. Every combination and order is supported; `s` swaps the lanes. Changing a lane's kind clears its selection.
3. Press `a` in each lane to select all its targets, or Space for individual ones. Each lane needs at least one.
4. Press `t`, type `5`, and press Enter. Then press Enter again to start.

Or prefill everything from the command line: `lictor --tui -W -A -s 5 -S w,a`, then press Enter.

Switching from focus to split keeps existing selections of the matching kinds. Switching back combines both lanes into one queue, and the lane kinds return the next time you enter split mode.

![The controller after starting a split job, with the Jobs panel](images/tui-running.png)

The controller stays open after Enter. The new job appears in the Jobs panel, where `p`, `x` and `X` control it; a split job's observation tab opens and takes focus in Herdr. Focus rotation moves Herdr focus away from the controller's pane, so return to it to pause or stop the job, or use `lictor --stop`. When you quit, Lictor prints the IDs of jobs that keep running. Detached job output goes to a private log, shown if a job fails to start. Reopening a job's controller with `--tui --job ID` pauses its rotation while you inspect it, so it does not keep stealing focus.

The Tokyo Night theme uses blue for the focused panel and selection, purple for the mode, and status icons alongside status text, so nothing depends on color alone. Terminals narrower than 72 columns stack the panels; below 40 × 10 the controller asks you to resize.
