# Sessions and sockets

All identifiers belong to one Herdr server. The same `w1:p1` can exist in two named sessions and represent different terminals.

Select a named session:

```bash
herdr session list
lictor --session work -A -l
lictor --session work -S w,a -s 5
```

Or select an explicit API socket:

```bash
lictor --socket-path /path/to/herdr.sock -W -l
```

`--session` and `--socket-path` are mutually exclusive. Named-session selection discovers an existing server; it must not start or replace one silently.

Without either option, Lictor uses the caller's `HERDR_SOCKET_PATH` when present, then Herdr's configuration-based socket location. Herdr configuration follows `HERDR_CONFIG_PATH`, otherwise the XDG configuration directory or `~/.config/herdr/config.toml`.

Relative selection context follows:

1. An explicit `--target` workspace or tab.
2. The caller's `HERDR_TAB_ID` when its environment identifies the selected server.
3. The selected server's focused workspace/tab.

This context is captured before rotation. Switching focus during rotation does not re-resolve initial numeric selectors.

Herdr has separate API and client sockets. Lictor sends NDJSON API requests to the API socket; observer subprocesses use Herdr's matching client socket resolution. An explicit API path must therefore point to the correct server installation and its accompanying client socket.

The initial release targets local Linux sessions. SSH machine routing and Windows named pipes are outside the initial platform contract.
