# Migrating from Bash

Lictor starts from the behavior of [`herdr_rotate`](https://gist.github.com/SyscallBrain/e6534601ece6d956e922c589f517078c), preserving target selection, timing, ordering, dynamic agent status, sidebar controls, and the stop-shortcut workflow.

| Original behavior | Lictor change |
| --- | --- |
| `-S PATH` selects the socket. | Use `--socket-path PATH`; `-S` now means split lanes. |
| Bare invocation starts default pane rotation. | Bare `lictor` opens the TUI; pass CLI arguments for direct rotation. |
| `-T` cannot combine with workspaces/agents. | Mixed groups are supported, with typed selectors for ambiguous mixes. |
| Bash messages/comments are Portuguese. | Lictor messages and documentation are English. |
| Dry-run ignores random/status decisions. | Preview uses the planner against a stated snapshot. |
| JSON calls use `jq` plus Python or `socat`. | A native Rust socket client handles API calls. |
| Stop discovery uses PID/name markers. | Managed jobs use private ownership/control records. |
| Output strategy changes global focus. | Focus remains available; split mode observes sources without moving their focus. |

Existing command examples with `-p`, `-T`, `-W`, `-A`, `-s`, `-n`, `-r`, `--status`, `-l`, and `--stop` have corresponding CLI behavior. Update executable paths in shell aliases and Herdr bindings.

The legacy `-W -A` first-tab policy is retained for focus mode; add `-c` for active tabs. Split workspace viewers use eligible active source tabs by default.

Lictor intentionally preserves duplicate queue entries as extra dwell time and frozen initial selection membership. Closing a selected target removes it; creating another target at the same numeric position does not silently select it.
