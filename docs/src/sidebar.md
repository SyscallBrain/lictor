# Sidebar and notifications

To hide the Herdr sidebar while rotating:

```bash
lictor -W -s 5 --sidebar hide
```

The actions are `hide`, `show`, and `toggle`. Lictor records the initial state where possible and restores it on completion or stop. `--sidebar-keep` leaves the requested state in place.

Herdr 0.9.3 has no public sidebar state/set API. The Linux adapter sends the configured Herdr prefix/toggle binding to supported clients through tmux or X11/xdotool. It reads Herdr's TOML key configuration rather than assuming everyone uses the default `ctrl+b`, then `b`.

Visibility is estimated from terminal width versus the pane layout. If that estimate is unavailable, exact restoration cannot be guaranteed. Multiple attached clients and concurrent manual sidebar changes can affect the estimate. An unsupported sidebar route reports a warning while core rotation remains usable.

These limitations concern sidebar control, not the ability to observe existing terminal content.

## Stop notifications

When a rotation is stopped, Lictor can send a desktop notification through `notify-send`. It sends the message after scheduling has stopped and sidebar restoration has finished, so the notification is a useful indication that automatic focus changes are over.

```bash
lictor -A -s 5 --no-notify
```

Missing notification helpers do not prevent rotation. Normal finite completion does not need the same stopped-by-signal notification.
