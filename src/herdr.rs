use serde_json::{Value, json};
use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    time::timeout,
};
use tokio_util::sync::CancellationToken;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Error)]
pub enum Error {
    #[error("Herdr socket unavailable: {0}")]
    Io(#[from] std::io::Error),
    #[error("Herdr returned an error: {0}")]
    Api(String),
    #[error("invalid Herdr response: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Herdr API request timed out")]
    Timeout,
    #[error("Herdr API request was cancelled")]
    Cancelled,
    #[error("invalid Herdr API reply: {0}")]
    Protocol(String),
}

#[derive(Clone, Debug)]
pub struct Context {
    pub socket: PathBuf,
    pub session: Option<String>,
    pub cancel: CancellationToken,
}

impl Context {
    pub fn fresh(&self) -> Self {
        Self {
            socket: self.socket.clone(),
            session: self.session.clone(),
            cancel: CancellationToken::new(),
        }
    }

    pub fn discover(socket: Option<PathBuf>, session: Option<String>) -> Self {
        let socket = if let Some(socket) = socket {
            socket
        } else if let Some(name) = session.as_deref() {
            named_session_socket(name)
                .unwrap_or_else(|| PathBuf::from("/nonexistent/lictor-session-unavailable.sock"))
        } else if let Some(socket) = std::env::var_os("HERDR_SOCKET_PATH") {
            PathBuf::from(socket)
        } else {
            let root = std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
                });
            root.join("herdr/herdr.sock")
        };
        let socket = if socket.is_absolute() {
            socket
        } else {
            std::env::current_dir().unwrap_or_default().join(socket)
        };
        let socket = std::fs::canonicalize(&socket).unwrap_or(socket);
        Self {
            socket,
            session,
            cancel: CancellationToken::new(),
        }
    }

    pub async fn request(&self, method: &str, params: Value) -> Result<Value, Error> {
        let id = format!("lictor:{}", NEXT_ID.fetch_add(1, Ordering::Relaxed));
        let request = json!({"id":id,"method":method,"params":params});
        let bytes = serde_json::to_vec(&request)?;
        let operation = async {
            let mut stream = UnixStream::connect(&self.socket).await?;
            stream.write_all(&bytes).await?;
            stream.write_all(b"\n").await?;
            let reader = BufReader::new(stream);
            let mut line = Vec::new();
            let n = reader
                .take(8 * 1024 * 1024 + 1)
                .read_until(b'\n', &mut line)
                .await?;
            if n == 0 {
                return Err(Error::Protocol("socket closed before reply".into()));
            }
            if n > 8 * 1024 * 1024 {
                return Err(Error::Protocol("reply exceeded 8 MiB limit".into()));
            }
            let reply: Value = serde_json::from_slice(&line)?;
            if reply.get("id").and_then(Value::as_str) != Some(id.as_str()) {
                return Err(Error::Protocol(
                    "response ID did not match the request".into(),
                ));
            }
            if let Some(err) = reply.get("error") {
                return Err(Error::Api(
                    err.get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown error")
                        .to_owned(),
                ));
            }
            if reply.get("result").is_none() {
                return Err(Error::Protocol("response did not contain a result".into()));
            }
            Ok(reply)
        };
        tokio::select! {
            _ = self.cancel.cancelled() => Err(Error::Cancelled),
            result = timeout(Duration::from_secs(10), operation) => result.map_err(|_|Error::Timeout)?,
        }
    }

    pub async fn snapshot(&self) -> Result<Value, Error> {
        self.request("session.snapshot", json!({})).await
    }
    pub async fn focus(&self, kind: &str, id: &str) -> Result<Value, Error> {
        let (method, key) = match kind {
            "pane" => ("pane.focus", "pane_id"),
            "tab" => ("tab.focus", "tab_id"),
            "workspace" => ("workspace.focus", "workspace_id"),
            _ => return Err(Error::Api(format!("cannot focus {kind}"))),
        };
        self.request(method, json!({key:id})).await
    }
    pub async fn layout_apply(&self, params: Value) -> Result<Value, Error> {
        self.request("layout.apply", params).await
    }
    pub fn observer(&self, target: &str, cols: u16, rows: u16) -> Result<Child, Error> {
        let mut c = Command::new("herdr");
        if let Some(session) = &self.session {
            c.args(["--session", session]);
        }
        c.args([
            "terminal",
            "session",
            "observe",
            target,
            "--cols",
            &cols.to_string(),
            "--rows",
            &rows.to_string(),
        ]);
        c.env("HERDR_SOCKET_PATH", &self.socket)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(Error::Io)
    }
}

fn named_session_socket(name: &str) -> Option<PathBuf> {
    let output = Command::new("herdr")
        .args(["session", "list", "--json"])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value: Value = serde_json::from_slice(&output.stdout).ok()?;
    value
        .get("sessions")?
        .as_array()?
        .iter()
        .find(|session| session.get("name").and_then(Value::as_str) == Some(name))?
        .get("socket_path")?
        .as_str()
        .map(PathBuf::from)
}
