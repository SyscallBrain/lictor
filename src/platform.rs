//! Optional Linux sidebar and desktop notification adapters.

use crate::herdr::Context;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{process::Command, time::timeout};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Visibility {
    Visible,
    Hidden,
    Unknown,
}

#[derive(Clone, Debug)]
enum Route {
    Tmux {
        pane: String,
        socket: Option<String>,
    },
    X11 {
        window: String,
    },
}

#[derive(Clone, Debug)]
struct Client {
    pid: u32,
    start_ticks: u64,
    session: String,
    tty: PathBuf,
    route: Route,
    initial: Visibility,
    toggled: bool,
}

/// Restores only sidebar clients to which this invocation delivered a toggle.
pub struct SidebarGuard {
    context: Context,
    clients: Vec<Client>,
    keys: Vec<String>,
    action: String,
    keep: bool,
    restored: bool,
    verbose: bool,
}

impl SidebarGuard {
    pub async fn apply(
        context: &Context,
        action: &str,
        keep: bool,
        verbose: bool,
    ) -> Result<Self, String> {
        if !matches!(action, "show" | "hide" | "toggle") {
            return Err("sidebar action must be show, hide, or toggle".into());
        }
        let keys = sidebar_keys(&config_path())?;
        let session = resolve_session(context).await?;
        let mut clients = discover_clients(&session).await;
        if clients.is_empty() {
            return Err(format!(
                "no supported sidebar client for session {session}; requires tmux or X11 with xdotool"
            ));
        }
        let width = pane_width(context).await;
        let mut successes = 0;
        for client in &mut clients {
            client.initial = visibility(&client.tty, width).await;
            if client.initial == Visibility::Unknown && action != "toggle" {
                eprintln!(
                    "lictor: sidebar state is unknown for client {}; applying one toggle",
                    client.pid
                );
            }
            if should_apply(action, client.initial) {
                match press(&client.route, &keys).await {
                    Ok(()) => {
                        client.toggled = true;
                        successes += 1;
                        if verbose {
                            eprintln!("lictor: toggled sidebar for client {}", client.pid);
                        }
                    }
                    Err(error) => {
                        eprintln!("lictor: sidebar client {}: {error}", client.pid);
                    }
                }
            } else {
                // A client already at the requested state is a valid result.
                successes += 1;
            }
        }
        if successes == 0 {
            return Err("sidebar key delivery failed for every supported client".into());
        }
        Ok(Self {
            context: context.fresh(),
            clients,
            keys,
            action: action.into(),
            keep,
            restored: false,
            verbose,
        })
    }

    /// Complete restoration before reporting that automatic rotation has stopped.
    pub async fn restore(&mut self) -> Vec<String> {
        if self.restored || self.keep {
            self.restored = true;
            return Vec::new();
        }
        self.restored = true;
        let mut warnings = Vec::new();
        let width = pane_width(&self.context).await;
        for client in &self.clients {
            if !client.toggled || !client_still_matches(client) {
                continue;
            }
            let current = visibility(&client.tty, width).await;
            if should_restore(&self.action, client.initial, current) {
                if let Err(error) = press(&client.route, &self.keys).await {
                    warnings.push(format!(
                        "sidebar restore for client {}: {error}",
                        client.pid
                    ));
                } else if self.verbose {
                    eprintln!("lictor: restored sidebar for client {}", client.pid);
                }
            } else if current == Visibility::Unknown || client.initial == Visibility::Unknown {
                warnings.push(format!(
                    "cannot determine original sidebar state for client {}",
                    client.pid
                ));
            }
        }
        warnings
    }
}

impl Drop for SidebarGuard {
    fn drop(&mut self) {
        if self.restored || self.keep {
            return;
        }
        // Explicit async restore is the normal path. On unwinding, undo delivered
        // toggles best-effort without waiting on Herdr or invoking a shell.
        for client in &self.clients {
            if client.toggled && client_still_matches(client) {
                let route = client.route.clone();
                let keys = self.keys.clone();
                // Preserve prefix/key ordering in a single bounded cleanup task.
                std::thread::spawn(move || {
                    for key in keys {
                        let (program, args) = key_command(&route, &key);
                        let Ok(mut child) = std::process::Command::new(program)
                            .args(args)
                            .stdin(Stdio::null())
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .spawn()
                        else {
                            break;
                        };
                        let deadline = std::time::Instant::now() + Duration::from_secs(2);
                        loop {
                            match child.try_wait() {
                                Ok(Some(_)) => break,
                                Ok(None) if std::time::Instant::now() < deadline => {
                                    std::thread::sleep(Duration::from_millis(10));
                                }
                                _ => {
                                    let _ = child.kill();
                                    let _ = child.wait();
                                    return;
                                }
                            }
                        }
                    }
                });
            }
        }
    }
}

pub async fn notify_stop(changes: usize, disabled: bool) {
    if disabled {
        return;
    }
    let body =
        format!("Rotation stopped after {changes} changes. Automatic focus changes are over.");
    let _ = run_output(
        "notify-send",
        &[
            "-a",
            "lictor",
            "-i",
            "media-playback-stop",
            "Lictor stopped",
            &body,
        ],
    )
    .await;
}

fn should_apply(action: &str, visibility: Visibility) -> bool {
    match action {
        "show" => visibility != Visibility::Visible,
        "hide" => visibility != Visibility::Hidden,
        _ => true,
    }
}

fn should_restore(action: &str, initial: Visibility, current: Visibility) -> bool {
    match (initial, current) {
        (Visibility::Unknown, _) | (_, Visibility::Unknown) => action == "toggle",
        _ => initial != current,
    }
}

fn config_path() -> PathBuf {
    if let Some(path) = std::env::var_os("HERDR_CONFIG_PATH") {
        return path.into();
    }
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        });
    root.join("herdr/config.toml")
}

fn config_key(value: &toml::Value, key: &str, fallback: &str) -> Result<String, String> {
    match value.get("keys").and_then(|keys| keys.get(key)) {
        None => Ok(fallback.into()),
        Some(toml::Value::String(key)) => Ok(key.clone()),
        Some(toml::Value::Array(keys)) => keys
            .first()
            .and_then(toml::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("Herdr keys.{key} has no usable first binding")),
        _ => Err(format!("Herdr keys.{key} must be a string or string list")),
    }
}

fn parse_keys(value: &toml::Value) -> Result<Vec<String>, String> {
    let prefix = config_key(value, "prefix", "ctrl+b")?;
    let toggle = config_key(value, "toggle_sidebar", "prefix+b")?;
    if toggle.is_empty() {
        return Err("Herdr toggle_sidebar binding is disabled".into());
    }
    if let Some(key) = toggle.strip_prefix("prefix+") {
        if prefix.is_empty() {
            return Err("Herdr prefix binding is disabled".into());
        }
        Ok(vec![prefix, key.into()])
    } else {
        Ok(vec![toggle])
    }
}

fn sidebar_keys(path: &Path) -> Result<Vec<String>, String> {
    let value = match fs::read_to_string(path) {
        Ok(text) => text
            .parse::<toml::Value>()
            .map_err(|error| format!("invalid Herdr key configuration: {error}"))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            toml::Value::Table(Default::default())
        }
        Err(error) => return Err(format!("cannot read Herdr key configuration: {error}")),
    };
    parse_keys(&value)
}

async fn resolve_session(context: &Context) -> Result<String, String> {
    if let Some(session) = &context.session {
        return Ok(session.clone());
    }
    let bytes = run_output("herdr", &["session", "list", "--json"]).await?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("cannot identify sidebar session: {error}"))?;
    value
        .get("sessions")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .find(|session| {
            session
                .get("socket_path")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|path| Path::new(path) == context.socket)
        })
        .and_then(|session| session.get("name"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "cannot map the selected socket to a sidebar client session".into())
}

async fn pane_width(context: &Context) -> Option<u64> {
    let reply = timeout(Duration::from_secs(2), context.snapshot())
        .await
        .ok()?
        .ok()?;
    let snapshot = reply.get("result")?.get("snapshot")?;
    let focused = snapshot.get("focused_tab_id")?.as_str()?;
    snapshot
        .get("layouts")?
        .as_array()?
        .iter()
        .find(|layout| layout.get("tab_id").and_then(serde_json::Value::as_str) == Some(focused))?
        .get("area")?
        .get("width")?
        .as_u64()
}

async fn visibility(tty: &Path, pane_width: Option<u64>) -> Visibility {
    let Some(width) = pane_width else {
        return Visibility::Unknown;
    };
    let Some(tty) = tty.to_str() else {
        return Visibility::Unknown;
    };
    let Ok(output) = run_output("stty", &["-F", tty, "size"]).await else {
        return Visibility::Unknown;
    };
    let columns = String::from_utf8_lossy(&output)
        .split_whitespace()
        .nth(1)
        .and_then(|columns| columns.parse::<u64>().ok());
    match columns {
        Some(columns) if columns >= width.saturating_add(9) => Visibility::Visible,
        Some(_) => Visibility::Hidden,
        None => Visibility::Unknown,
    }
}

fn process_args(pid: u32) -> Option<Vec<String>> {
    let bytes = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    Some(
        bytes
            .split(|byte| *byte == 0)
            .filter(|part| !part.is_empty())
            .map(|part| String::from_utf8_lossy(part).into_owned())
            .collect(),
    )
}

fn client_session(args: &[String]) -> Option<String> {
    let (program, arguments) = args.split_first()?;
    if Path::new(program).file_name()?.to_str()? != "herdr" {
        return None;
    }
    if arguments.len() == 3 && arguments[0] == "session" && arguments[1] == "attach" {
        return Some(arguments[2].clone());
    }
    let mut session = "default".to_owned();
    let mut i = 0;
    while i < arguments.len() {
        match arguments[i].as_str() {
            "--session" => {
                session = arguments.get(i + 1)?.clone();
                i += 2;
            }
            "--handoff" => i += 1,
            value if value.starts_with("--session=") => {
                session = value.strip_prefix("--session=")?.into();
                i += 1;
            }
            // Remote UI and API/server processes cannot be safely identified as
            // attached local clients by their command line alone.
            _ => return None,
        }
    }
    Some(session)
}

fn parent(pid: u32) -> Option<u32> {
    fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("PPid:")?.trim().parse().ok())
}

fn tmux_socket(pid: u32) -> Option<String> {
    let bytes = fs::read(format!("/proc/{pid}/environ")).ok()?;
    bytes.split(|byte| *byte == 0).find_map(|part| {
        let text = std::str::from_utf8(part).ok()?.strip_prefix("TMUX=")?;
        // Socket paths can contain commas; tmux appends two numeric fields.
        Some(text.rsplitn(3, ',').nth(2)?.into())
    })
}

async fn discover_clients(session: &str) -> Vec<Client> {
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut clients = Vec::new();
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        if process_args(pid)
            .and_then(|args| client_session(&args))
            .as_deref()
            != Some(session)
        {
            continue;
        }
        let Ok(tty) = fs::read_link(format!("/proc/{pid}/fd/0")) else {
            continue;
        };
        if !tty.starts_with("/dev/pts") && !tty.starts_with("/dev/tty") {
            continue;
        }
        let Some(start_ticks) = process_start(pid) else {
            continue;
        };
        let socket = tmux_socket(pid);
        let mut ancestor = Some(pid);
        let mut route = None;
        for _ in 0..64 {
            let Some(process) = ancestor.filter(|pid| *pid > 1) else {
                break;
            };
            let comm = fs::read_to_string(format!("/proc/{process}/comm")).unwrap_or_default();
            if comm.trim().starts_with("tmux") {
                let mut arguments = Vec::<String>::new();
                if let Some(socket) = &socket {
                    arguments.extend(["-S".into(), socket.clone()]);
                }
                arguments.extend([
                    "list-panes".into(),
                    "-a".into(),
                    "-F".into(),
                    "#{pane_tty} #{pane_id}".into(),
                ]);
                let borrowed = arguments.iter().map(String::as_str).collect::<Vec<_>>();
                if let Ok(output) = run_output("tmux", &borrowed).await {
                    let text = String::from_utf8_lossy(&output);
                    if let Some(pane) = text.lines().find_map(|line| {
                        let mut fields = line.split_whitespace();
                        (fields.next()? == tty.to_str()?)
                            .then(|| fields.next().map(str::to_owned))
                            .flatten()
                    }) {
                        route = Some(Route::Tmux {
                            pane,
                            socket: socket.clone(),
                        });
                    }
                }
                break;
            }
            if std::env::var_os("DISPLAY").is_some()
                && let Ok(output) =
                    run_output("xdotool", &["search", "--pid", &process.to_string()]).await
            {
                let text = String::from_utf8_lossy(&output);
                let windows = text.split_whitespace().collect::<Vec<_>>();
                if windows.len() == 1 {
                    route = Some(Route::X11 {
                        window: windows[0].into(),
                    });
                    break;
                }
                if windows.len() > 1 {
                    break;
                }
            }
            ancestor = parent(process);
        }
        if let Some(route) = route {
            clients.push(Client {
                pid,
                start_ticks,
                session: session.into(),
                tty,
                route,
                initial: Visibility::Unknown,
                toggled: false,
            });
        }
    }
    clients
}

fn process_start(pid: u32) -> Option<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()
}

fn client_still_matches(client: &Client) -> bool {
    fs::read_link(format!("/proc/{}/fd/0", client.pid))
        .ok()
        .as_ref()
        == Some(&client.tty)
        && process_start(client.pid) == Some(client.start_ticks)
        && process_args(client.pid)
            .and_then(|args| client_session(&args))
            .as_deref()
            == Some(client.session.as_str())
}

fn translated_key(key: &str, x11: bool) -> String {
    let mut pieces = key.split('+').map(str::to_owned).collect::<Vec<_>>();
    let mut symbol = pieces.pop().unwrap_or_default();
    if key.ends_with("++") {
        symbol = "+".into();
        pieces.pop();
    }
    let lower = symbol.to_lowercase();
    symbol = match lower.as_str() {
        "enter" | "return" => {
            if x11 {
                "Return"
            } else {
                "Enter"
            }
        }
        "esc" | "escape" => "Escape",
        "space" => {
            if x11 {
                "space"
            } else {
                "Space"
            }
        }
        "backspace" => {
            if x11 {
                "BackSpace"
            } else {
                "BSpace"
            }
        }
        "tab" => "Tab",
        "left" => "Left",
        "right" => "Right",
        "up" => "Up",
        "down" => "Down",
        "backtick" | "`" => {
            if x11 {
                "grave"
            } else {
                "`"
            }
        }
        "minus" | "-" => {
            if x11 {
                "minus"
            } else {
                "-"
            }
        }
        "plus" | "+" => {
            if x11 {
                "plus"
            } else {
                "+"
            }
        }
        "comma" | "," => {
            if x11 {
                "comma"
            } else {
                ","
            }
        }
        "ampersand" | "&" => {
            if x11 {
                "ampersand"
            } else {
                "&"
            }
        }
        "/" if x11 => "slash",
        "?" if x11 => "question",
        "." if x11 => "period",
        ";" if x11 => "semicolon",
        "=" if x11 => "equal",
        "[" if x11 => "bracketleft",
        "]" if x11 => "bracketright",
        "\\" if x11 => "backslash",
        "'" if x11 => "apostrophe",
        "!" if x11 => "exclam",
        "#" if x11 => "numbersign",
        "$" if x11 => "dollar",
        _ => &symbol,
    }
    .into();
    if lower.starts_with('f') && lower[1..].parse::<u8>().is_ok() {
        symbol = lower.to_uppercase();
    }
    let mut out = String::new();
    for modifier in pieces {
        if x11 {
            out.push_str(match modifier.to_lowercase().as_str() {
                "cmd" | "super" => "super",
                "control" => "ctrl",
                _ => &modifier,
            });
            out.push('+');
        } else {
            out.push_str(match modifier.to_lowercase().as_str() {
                "ctrl" | "control" => "C-",
                "alt" => "M-",
                "shift" => "S-",
                _ => "",
            });
        }
    }
    out.push_str(&symbol);
    out
}

fn key_command(route: &Route, key: &str) -> (&'static str, Vec<String>) {
    match route {
        Route::X11 { window } => (
            "xdotool",
            vec![
                "key".into(),
                "--window".into(),
                window.clone(),
                "--clearmodifiers".into(),
                translated_key(key, true),
            ],
        ),
        Route::Tmux { pane, socket } => {
            let mut args = Vec::new();
            if let Some(socket) = socket {
                args.extend(["-S".into(), socket.clone()]);
            }
            args.extend([
                "send-keys".into(),
                "-t".into(),
                pane.clone(),
                translated_key(key, false),
            ]);
            ("tmux", args)
        }
    }
}

async fn press(route: &Route, keys: &[String]) -> Result<(), String> {
    for key in keys {
        let (program, args) = key_command(route, key);
        run_output(
            program,
            &args.iter().map(String::as_str).collect::<Vec<_>>(),
        )
        .await?;
    }
    Ok(())
}

async fn run_output(program: &str, arguments: &[&str]) -> Result<Vec<u8>, String> {
    let mut command = Command::new(program);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let output = timeout(Duration::from_secs(2), command.output())
        .await
        .map_err(|_| format!("{program} timed out"))?
        .map_err(|error| format!("{program}: {error}"))?;
    if !output.status.success() {
        return Err(format!("{program} key/helper operation failed"));
    }
    Ok(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_prefix_and_list_binding_use_first_key() {
        let value = "[keys]\nprefix = 'ctrl+a'\ntoggle_sidebar = ['prefix+shift+b', 'alt+b']"
            .parse()
            .unwrap();
        assert_eq!(parse_keys(&value).unwrap(), ["ctrl+a", "shift+b"]);
        assert_eq!(translated_key("shift+b", false), "S-b");
    }

    #[test]
    fn disabled_toggle_is_not_replaced_with_default() {
        let value = "[keys]\ntoggle_sidebar = ''".parse().unwrap();
        assert!(parse_keys(&value).is_err());
    }

    #[test]
    fn only_identified_local_ui_clients_receive_keys() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            client_session(&args(&["/bin/herdr", "--session", "work"])),
            Some("work".into())
        );
        assert_eq!(
            client_session(&args(&["herdr", "session", "attach", "work"])),
            Some("work".into())
        );
        assert!(client_session(&args(&["herdr", "--session", "work", "server"])).is_none());
        assert!(client_session(&args(&["herdr", "--remote", "work"])).is_none());
    }

    #[test]
    fn show_hide_and_uncertain_restore_follow_reference_semantics() {
        assert!(!should_apply("show", Visibility::Visible));
        assert!(!should_apply("hide", Visibility::Hidden));
        assert!(should_apply("show", Visibility::Unknown));
        assert!(should_restore(
            "hide",
            Visibility::Visible,
            Visibility::Hidden
        ));
        assert!(!should_restore(
            "show",
            Visibility::Unknown,
            Visibility::Visible
        ));
        assert!(should_restore(
            "toggle",
            Visibility::Unknown,
            Visibility::Unknown
        ));
    }

    #[test]
    fn translated_punctuation_and_chords_are_passed_as_arguments() {
        assert_eq!(translated_key("ctrl++", false), "C-+");
        assert_eq!(translated_key("ctrl++", true), "ctrl+plus");
        assert_eq!(translated_key("alt+enter", true), "alt+Return");
        let route = Route::Tmux {
            pane: "%4".into(),
            socket: Some("/tmp/socket path".into()),
        };
        let (_, args) = key_command(&route, "ctrl+a");
        assert_eq!(
            args,
            ["-S", "/tmp/socket path", "send-keys", "-t", "%4", "C-a"]
        );
    }
}
