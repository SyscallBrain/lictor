//! Private local job records used to stop only the exact Lictor process that
//! registered a focus rotation.
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
};

static PAUSED: AtomicBool = AtomicBool::new(false);
static PAUSE_NOTIFY: std::sync::LazyLock<tokio::sync::Notify> =
    std::sync::LazyLock::new(tokio::sync::Notify::new);
pub fn set_paused(paused: bool) {
    PAUSED.store(paused, Ordering::SeqCst);
    PAUSE_NOTIFY.notify_one();
}
pub fn is_paused() -> bool {
    PAUSED.load(Ordering::SeqCst)
}
pub async fn wait_if_paused(cancel: &tokio_util::sync::CancellationToken) {
    if is_paused() {
        acknowledge_state("paused");
    }
    while is_paused() {
        tokio::select! { _ = cancel.cancelled() => return, _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {} }
    }
}
pub async fn sleep_or_pause(
    duration: std::time::Duration,
    cancel: &tokio_util::sync::CancellationToken,
) -> bool {
    loop {
        tokio::select! {
            _=cancel.cancelled()=>return false,
            _=tokio::time::sleep(duration)=>return true,
            _=PAUSE_NOTIFY.notified()=>{wait_if_paused(cancel).await;if cancel.is_cancelled(){return false;}}
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Record {
    id: String,
    pid: u32,
    start_ticks: u64,
    #[serde(default)]
    executable_device: u64,
    #[serde(default)]
    executable_inode: u64,
    uid: u32,
    #[serde(default)]
    session: Option<String>,
    #[serde(default)]
    socket: Option<PathBuf>,
    #[serde(default)]
    tab_id: Option<String>,
    #[serde(default)]
    pane_ids: Vec<String>,
    #[serde(default)]
    terminal_ids: Vec<String>,
    #[serde(default = "running_state")]
    state: String,
}

fn running_state() -> String {
    "running".into()
}

#[derive(Clone, Debug)]
pub struct JobInfo {
    pub id: String,
    pub pid: u32,
    pub state: String,
    pub tab_id: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct LaneCommand {
    pub generation: u64,
    pub target: String,
    #[serde(default)]
    pub ack_generation: u64,
    #[serde(default)]
    pub acknowledged: bool,
    #[serde(default = "enabled_default")]
    pub enabled: bool,
}
fn enabled_default() -> bool {
    true
}

pub struct SplitControls {
    pub left: PathBuf,
    pub right: PathBuf,
}
impl SplitControls {
    pub fn create(
        left_target: &str,
        right_target: &str,
        left_enabled: bool,
        right_enabled: bool,
    ) -> Result<Self, String> {
        let uid = unsafe { libc::getuid() };
        let dir = runtime_directory(uid)?;
        let nonce = format!("{:016x}", rand::random::<u64>());
        let left = dir.join(format!("split-{nonce}-left.json"));
        let right = dir.join(format!("split-{nonce}-right.json"));
        for (path, target) in [(&left, left_target), (&right, right_target)] {
            let cmd = LaneCommand {
                generation: 1,
                target: target.to_owned(),
                ack_generation: 0,
                acknowledged: false,
                enabled: if path == &left {
                    left_enabled
                } else {
                    right_enabled
                },
            };
            fs::write(path, serde_json::to_vec(&cmd).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        }
        Ok(Self { left, right })
    }
    pub fn update(path: &PathBuf, generation: u64, target: &str) -> Result<(), String> {
        let mut cmd = Self::read(path)?;
        cmd.generation = generation;
        cmd.target = target.to_owned();
        cmd.acknowledged = false;
        cmd.enabled = true;
        write_lane(path, &cmd)
    }
    pub fn read(path: &PathBuf) -> Result<LaneCommand, String> {
        serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())
    }
    pub fn ack(path: &PathBuf, generation: u64, ok: bool) -> Result<(), String> {
        let mut cmd = Self::read(path)?;
        if cmd.generation != generation {
            return Ok(());
        }
        cmd.ack_generation = generation;
        cmd.acknowledged = ok;
        write_lane(path, &cmd)
    }
    pub fn remove(&self) {
        let _ = fs::remove_file(&self.left);
        let _ = fs::remove_file(&self.right);
    }
}
impl Drop for SplitControls {
    fn drop(&mut self) {
        self.remove();
    }
}
fn write_lane(path: &PathBuf, command: &LaneCommand) -> Result<(), String> {
    let temp = path.with_extension("tmp");
    fs::write(
        &temp,
        serde_json::to_vec(command).map_err(|e| e.to_string())?,
    )
    .and_then(|_| fs::rename(temp, path))
    .map_err(|e| e.to_string())
}

pub fn owned_view_ids(socket: &std::path::Path) -> (HashSet<String>, HashSet<String>) {
    let mut tabs = HashSet::new();
    let mut panes = HashSet::new();
    let uid = unsafe { libc::getuid() };
    let Ok(directory) = runtime_directory(uid) else {
        return (tabs, panes);
    };
    let Ok(entries) = fs::read_dir(directory) else {
        return (tabs, panes);
    };
    for entry in entries.flatten() {
        let Ok(bytes) = fs::read(entry.path()) else {
            continue;
        };
        let Ok(record) = serde_json::from_slice::<Record>(&bytes) else {
            continue;
        };
        if record.uid == uid
            && record
                .socket
                .as_deref()
                .is_some_and(|record_socket| same_socket(record_socket, socket))
        {
            if let Some(tab_id) = record.tab_id {
                tabs.insert(tab_id);
            }
            panes.extend(record.pane_ids);
        }
    }
    (tabs, panes)
}

pub fn filter_owned(snapshot: &mut crate::domain::Snapshot, socket: &std::path::Path) {
    validate_owned_records(snapshot, socket);
    let (tabs, panes) = owned_view_ids(socket);
    snapshot.targets.retain(|target| {
        !((target.kind == crate::domain::Kind::Tab && tabs.contains(&target.id))
            || ((target.kind == crate::domain::Kind::Pane
                || target.kind == crate::domain::Kind::Agent)
                && panes.contains(&target.id)))
    });
    snapshot.layouts.retain(|layout| {
        layout
            .get("tab_id")
            .and_then(serde_json::Value::as_str)
            .is_none_or(|id| !tabs.contains(id))
    });
}

fn validate_owned_records(snapshot: &crate::domain::Snapshot, socket: &std::path::Path) {
    let uid = unsafe { libc::getuid() };
    let Ok(directory) = runtime_directory(uid) else {
        return;
    };
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(bytes) = fs::read(&path) else { continue };
        let Ok(mut record) = serde_json::from_slice::<Record>(&bytes) else {
            continue;
        };
        if record.uid != uid
            || !record
                .socket
                .as_deref()
                .is_some_and(|candidate| same_socket(candidate, socket))
        {
            continue;
        }
        let Some(tab) = record.tab_id.as_deref() else {
            continue;
        };
        let live: HashSet<(String, String)> = snapshot
            .targets
            .iter()
            .filter(|target| {
                target.kind == crate::domain::Kind::Pane && target.tab_id.as_deref() == Some(tab)
            })
            .filter_map(|target| Some((target.id.clone(), target.terminal_id.clone()?)))
            .collect();
        let owned: HashSet<(String, String)> = record
            .pane_ids
            .iter()
            .cloned()
            .zip(record.terminal_ids.iter().cloned())
            .collect();
        let tab_live = snapshot
            .layouts
            .iter()
            .any(|layout| layout.get("tab_id").and_then(serde_json::Value::as_str) == Some(tab));
        if tab_live && record.pane_ids.len() == record.terminal_ids.len() && live == owned {
            continue;
        }
        if record.state == "view-only" {
            let _ = fs::remove_file(path);
        } else {
            record.tab_id = None;
            record.pane_ids.clear();
            record.terminal_ids.clear();
            if let Ok(bytes) = serde_json::to_vec(&record) {
                let tmp = path.with_extension("tmp");
                if fs::write(&tmp, bytes).is_ok() {
                    let _ = fs::rename(tmp, path);
                }
            }
        }
    }
}

fn same_socket(a: &std::path::Path, b: &std::path::Path) -> bool {
    fs::canonicalize(a).unwrap_or_else(|_| a.to_path_buf())
        == fs::canonicalize(b).unwrap_or_else(|_| b.to_path_buf())
}

pub struct JobGuard {
    path: PathBuf,
    pub id: String,
    retained: bool,
}

impl Drop for JobGuard {
    fn drop(&mut self) {
        if !self.retained {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(target_os = "linux")]
pub fn register(context: &crate::herdr::Context) -> Result<JobGuard, String> {
    use std::os::unix::fs::OpenOptionsExt;
    let pid = std::process::id();
    let uid = unsafe { libc::getuid() };
    let start_ticks = process_start(pid).ok_or("cannot read this process identity")?;
    let (executable_device, executable_inode) =
        executable_identity(pid).ok_or("cannot read this executable identity")?;
    let id = format!("{}-{:016x}", pid, rand::random::<u64>());
    let directory = runtime_directory(uid)?;
    let path = directory.join(format!("{id}.json"));
    let record = Record {
        id: id.clone(),
        pid,
        start_ticks,
        executable_device,
        executable_inode,
        uid,
        session: context.session.clone(),
        socket: Some(context.socket.clone()),
        tab_id: None,
        pane_ids: Vec::new(),
        terminal_ids: Vec::new(),
        state: running_state(),
    };
    let bytes = serde_json::to_vec(&record).map_err(|e| e.to_string())?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .map_err(|e| format!("cannot register rotation job: {e}"))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| format!("cannot persist rotation job: {e}"))?;
    Ok(JobGuard {
        path,
        id,
        retained: false,
    })
}

pub fn register_split(
    context: &crate::herdr::Context,
    tab_id: String,
    pane_ids: Vec<String>,
    terminal_ids: Vec<String>,
) -> Result<JobGuard, String> {
    let guard = register(context)?;
    let mut record: Record =
        serde_json::from_slice(&fs::read(&guard.path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    record.tab_id = Some(tab_id);
    record.pane_ids = pane_ids;
    record.terminal_ids = terminal_ids;
    fs::write(
        &guard.path,
        serde_json::to_vec(&record).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(guard)
}

impl JobGuard {
    pub fn retain_view(&mut self) -> Result<(), String> {
        let bytes = fs::read(&self.path).map_err(|e| e.to_string())?;
        let mut record: Record = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        record.pid = 0;
        record.start_ticks = 0;
        record.state = "view-only".into();
        let temp = self.path.with_extension("tmp");
        fs::write(
            &temp,
            serde_json::to_vec(&record).map_err(|e| e.to_string())?,
        )
        .and_then(|_| fs::rename(&temp, &self.path))
        .map_err(|e| e.to_string())?;
        self.retained = true;
        Ok(())
    }
}

pub fn list() -> Result<Vec<JobInfo>, String> {
    let uid = unsafe { libc::getuid() };
    let directory = runtime_directory(uid)?;
    let mut jobs = Vec::new();
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let Ok(bytes) = fs::read(&path) else { continue };
        let Ok(record) = serde_json::from_slice::<Record>(&bytes) else {
            continue;
        };
        if record.uid != uid {
            continue;
        }
        if record.state == "view-only"
            || (process_start(record.pid) == Some(record.start_ticks)
                && process_uid(record.pid) == Some(uid)
                && executable_identity(record.pid)
                    == Some((record.executable_device, record.executable_inode)))
        {
            jobs.push(JobInfo {
                id: record.id,
                pid: record.pid,
                state: record.state,
                tab_id: record.tab_id,
            });
        } else {
            let _ = fs::remove_file(path);
        }
    }
    jobs.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(jobs)
}

pub async fn control(action: &str, id: Option<&str>) -> Result<usize, String> {
    if action == "close-view" {
        return close_views(id).await;
    }
    let (signal, expected) = match action {
        "stop" => (libc::SIGINT, "stopping"),
        "pause" => (libc::SIGUSR1, "paused"),
        "resume" => (libc::SIGUSR2, "running"),
        _ => return Err(format!("unknown job action: {action}")),
    };
    let ids = signal_jobs(signal, id)?;
    wait_for_ack(&ids, action, expected).await?;
    Ok(ids.len())
}

async fn wait_for_ack(ids: &[String], action: &str, expected: &str) -> Result<(), String> {
    for job_id in ids {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(4);
        loop {
            let record = find_record(job_id)?;
            let acknowledged = if action == "stop" {
                record.as_ref().is_none_or(|r| r.state == "view-only")
            } else {
                record.as_ref().is_some_and(|r| r.state == expected)
            };
            if acknowledged {
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(format!("job {job_id} did not acknowledge {action}"));
            }
            tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        }
    }
    Ok(())
}

fn signal_jobs(signal: i32, id: Option<&str>) -> Result<Vec<String>, String> {
    let uid = unsafe { libc::getuid() };
    let directory = runtime_directory(uid)?;
    let mut sent = Vec::new();
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let Ok(bytes) = fs::read(&path) else { continue };
        let Ok(record) = serde_json::from_slice::<Record>(&bytes) else {
            continue;
        };
        if record.uid != uid || record.state == "view-only" || id.is_some_and(|id| id != record.id)
        {
            continue;
        }
        if process_start(record.pid) != Some(record.start_ticks)
            || process_uid(record.pid) != Some(uid)
            || executable_identity(record.pid)
                != Some((record.executable_device, record.executable_inode))
        {
            let _ = fs::remove_file(path);
            continue;
        }
        if unsafe { libc::kill(record.pid as libc::pid_t, signal) } == 0 {
            sent.push(record.id);
        }
    }
    Ok(sent)
}

pub fn acknowledge_state(state: &str) {
    let uid = unsafe { libc::getuid() };
    let pid = std::process::id();
    let Ok(directory) = runtime_directory(uid) else {
        return;
    };
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(bytes) = fs::read(&path) else { continue };
        let Ok(mut record) = serde_json::from_slice::<Record>(&bytes) else {
            continue;
        };
        if record.pid != pid || record.uid != uid {
            continue;
        }
        if record.state == state {
            continue;
        }
        record.state = state.to_owned();
        let tmp = path.with_extension("tmp");
        if let Ok(bytes) = serde_json::to_vec(&record)
            && fs::write(&tmp, bytes).is_ok()
        {
            let _ = fs::rename(tmp, path);
        }
    }
}

fn find_record(id: &str) -> Result<Option<Record>, String> {
    let uid = unsafe { libc::getuid() };
    let path = runtime_directory(uid)?.join(format!("{id}.json"));
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

async fn close_views(id: Option<&str>) -> Result<usize, String> {
    use crate::{domain::Snapshot, herdr::Context};
    let uid = unsafe { libc::getuid() };
    let directory = runtime_directory(uid)?;
    let mut closed = 0;
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let Ok(bytes) = fs::read(&path) else { continue };
        let Ok(record) = serde_json::from_slice::<Record>(&bytes) else {
            continue;
        };
        if record.uid != uid || record.tab_id.is_none() || id.is_some_and(|id| id != record.id) {
            continue;
        }
        if record.state != "view-only" {
            let ids = signal_jobs(libc::SIGINT, Some(&record.id))?;
            wait_for_ack(&ids, "stop", "view-only").await?;
            let Some(updated) = find_record(&record.id)? else {
                continue;
            };
            if updated.state != "view-only" {
                continue;
            }
        }
        let socket = record
            .socket
            .clone()
            .ok_or("retained view is missing its socket")?;
        let context = Context::discover(Some(socket), record.session.clone());
        let raw = context
            .snapshot()
            .await
            .map_err(|e| format!("cannot verify retained tab: {e}"))?;
        let snapshot = Snapshot::from_value(&raw);
        let tab_id = record.tab_id.as_ref().unwrap();
        let Some(layout) = snapshot.layouts.iter().find(|layout| {
            layout.get("tab_id").and_then(serde_json::Value::as_str) == Some(tab_id.as_str())
        }) else {
            fs::remove_file(path).map_err(|e| e.to_string())?;
            continue;
        };
        let live: HashSet<(String, String)> = layout
            .get("panes")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|pane| pane.get("pane_id")?.as_str().map(str::to_owned))
            .filter_map(|id| {
                snapshot
                    .targets
                    .iter()
                    .find(|t| t.kind == crate::domain::Kind::Pane && t.id == id)
                    .and_then(|t| t.terminal_id.clone())
                    .map(|terminal| (id, terminal))
            })
            .collect();
        let owned: HashSet<(String, String)> = record
            .pane_ids
            .iter()
            .cloned()
            .zip(record.terminal_ids.iter().cloned())
            .collect();
        if live != owned {
            return Err(format!(
                "refusing to close tab {tab_id}: its pane ownership changed"
            ));
        }
        context
            .request("tab.close", serde_json::json!({"tab_id":tab_id}))
            .await
            .map_err(|e| e.to_string())?;
        fs::remove_file(path).map_err(|e| e.to_string())?;
        closed += 1;
    }
    Ok(closed)
}

#[cfg(not(target_os = "linux"))]
pub fn register(_: &crate::herdr::Context) -> Result<JobGuard, String> {
    Err("managed job stopping is currently supported on Linux only".into())
}

#[cfg(target_os = "linux")]
fn runtime_directory(uid: u32) -> Result<PathBuf, String> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && p.exists())
        .unwrap_or_else(std::env::temp_dir);
    let directory = base.join(format!("lictor-{uid}"));
    if !directory.exists() {
        fs::create_dir(&directory)
            .map_err(|e| format!("cannot create private job directory: {e}"))?;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    let metadata = fs::symlink_metadata(&directory).map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink()
        || !metadata.is_dir()
        || metadata.uid() != uid
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err("job directory is not a private directory owned by this user".into());
    }
    Ok(directory)
}

#[cfg(target_os = "linux")]
fn process_uid(pid: u32) -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(format!("/proc/{pid}")).ok().map(|m| m.uid())
}

#[cfg(target_os = "linux")]
fn executable_identity(pid: u32) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::metadata(format!("/proc/{pid}/exe")).ok()?;
    Some((metadata.dev(), metadata.ino()))
}

#[cfg(target_os = "linux")]
fn process_start(pid: u32) -> Option<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let fields = stat
        .rsplit_once(')')?
        .1
        .split_whitespace()
        .collect::<Vec<_>>();
    fields.get(19)?.parse().ok() // field 22, starttime; field 3 begins at index 0.
}
