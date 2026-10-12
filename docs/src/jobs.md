# Jobs and stopping

A managed job owns its rotation queue, timers, local control state, optional sidebar restoration state, and any observation views it creates. Controller lifetime is separate from job lifetime.

The TUI hands a started rotation to a detached job and stays open, listing it in its Jobs panel. This matters for focus rotation: the user may immediately be looking at a different pane. `lictor --jobs` lists every job with its state and mirror tab, so you can find its ID later.

```bash
lictor --stop
lictor --job JOB_ID --pause
lictor --job JOB_ID --resume
lictor --job JOB_ID --stop
lictor --tui --job JOB_ID
```

Reopening the controller pauses the selected job for inspection. Resume deliberately with Enter or the resume command.

`--stop` without a job ID addresses every active Lictor rotation for the current user, across Herdr sessions. It does not stop the Herdr server or source agents. No matching active rotation returns exit code 3.

Ctrl-C, SIGTERM, and SIGHUP stop scheduling. Cleanup and sidebar restoration finish before the stop acknowledgement/notification. Pending socket waits and timers must not cause another focus change after acknowledgement.

## Retained observation views

By default, a split stop ends rotation while preserving the last selected source views. Viewer processes remain tracked as view-only resources and continue to show source output. They are not reported as active rotations.

Close them through the TUI's close-view action or by closing the generated Herdr tab. Starting with `--close-on-exit` requests automatic cleanup on stop/completion. Cleanup closes only resources created by that job.

The private registry lives beneath `$XDG_RUNTIME_DIR/lictor-UID` when usable, otherwise a user-private temporary directory. Do not delete its files to stop a job; use controls so source observers and sidebar state can be released correctly.

## A Herdr stop shortcut

Use the installed executable's absolute path in your Herdr configuration:

```toml
[[keys.command]]
key = "prefix+shift+k"
type = "shell"
command = "/absolute/path/to/lictor --stop"
description = "Stop Lictor rotations"
```

Check Herdr's current configuration syntax before adding this binding. The global command stops all of your active Lictor rotations; include `--job` if you need a single job.
