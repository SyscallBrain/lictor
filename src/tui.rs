//! Vim-style controller. Rotation runs in a detached, separately managed job.
//!
//! The screen is a board of two panels that mirrors what will run: in split mode
//! the left and right lanes sit side by side like the observation tab they
//! create; in focus mode a target browser sits beside the rotation queue.
use crate::{
    cli::Args,
    domain::{Kind, Snapshot, Target},
    herdr::{Context, Error},
    rotation, runtime,
};
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};
use std::{
    io,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

// Tokyo Night.
const BG: Color = Color::Rgb(26, 27, 38);
const SURFACE: Color = Color::Rgb(36, 40, 59);
const SELECT: Color = Color::Rgb(41, 46, 66);
const FG: Color = Color::Rgb(192, 202, 245);
const MUTED: Color = Color::Rgb(86, 95, 137);
const SUBTLE: Color = Color::Rgb(115, 122, 162);
const BLUE: Color = Color::Rgb(122, 162, 247);
const PURPLE: Color = Color::Rgb(187, 154, 247);
const GREEN: Color = Color::Rgb(158, 206, 106);
const YELLOW: Color = Color::Rgb(224, 175, 104);
const RED: Color = Color::Rgb(247, 118, 142);

const KINDS: [Kind; 4] = [Kind::Workspace, Kind::Tab, Kind::Agent, Kind::Pane];
const LEFT: usize = 0;
const RIGHT: usize = 1;
const JOBS: usize = 2;
const SETTINGS: usize = 8;

struct Restore;
impl Drop for Restore {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), crossterm::cursor::Show, LeaveAlternateScreen);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Identity {
    kind: Kind,
    id: String,
}
impl From<&Target> for Identity {
    fn from(t: &Target) -> Self {
        Self {
            kind: t.kind.clone(),
            id: t.id.clone(),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Normal,
    Search,
    Settings,
    Edit,
    Interval,
    Help,
}
struct App {
    snapshot: Snapshot,
    args: Args,
    /// Focus mode uses queue 0; split mode has one queue per lane.
    queues: [Vec<Identity>; 2],
    split: Option<[Kind; 2]>,
    /// Lane kinds restored when switching back to split mode.
    last_split: [Kind; 2],
    /// Kind listed by the focus-mode browser.
    browse: Kind,
    panel: usize,
    rows: [usize; 3],
    search: String,
    mode: Mode,
    input: String,
    input_error: Option<String>,
    setting: usize,
    visual: Option<usize>,
    pending_g: bool,
    message: String,
    error: bool,
    jobs: Vec<runtime::JobInfo>,
    job: Option<String>,
    started: Vec<String>,
}
impl App {
    fn new(args: Args, mut snapshot: Snapshot) -> Self {
        let baseline_error = set_baseline(&mut snapshot, args.target.as_deref()).err();
        let split = args.split.as_ref().and_then(|s| {
            let kinds = s.split(',').map(parse_kind).collect::<Vec<_>>();
            (kinds.len() == 2).then(|| [kinds[0].clone(), kinds[1].clone()])
        });
        let mut app = Self {
            job: args.job.clone(),
            args,
            snapshot,
            queues: Default::default(),
            last_split: split.clone().unwrap_or([Kind::Workspace, Kind::Agent]),
            split,
            browse: Kind::Workspace,
            panel: LEFT,
            rows: [0; 3],
            search: String::new(),
            mode: Mode::Normal,
            input: String::new(),
            input_error: None,
            setting: 0,
            visual: None,
            pending_g: false,
            message: String::new(),
            error: false,
            jobs: Vec::new(),
            started: Vec::new(),
        };
        if let Some(kinds) = app.split.clone() {
            for (lane, kind) in kinds.iter().enumerate() {
                let selectors = if lane == LEFT {
                    &app.args.left_targets
                } else {
                    &app.args.right_targets
                };
                match rotation::resolve(&app.snapshot, std::slice::from_ref(kind), selectors) {
                    Ok(targets) if targets.iter().all(|t| t.kind == *kind) => {
                        app.queues[lane] = targets.iter().map(Identity::from).collect()
                    }
                    Ok(_) => app.fail("A prefilled selector has the wrong lane kind."),
                    Err(error) => app.fail(format!("Selection: {error}")),
                }
            }
        } else if app.args.tabs
            || app.args.tab_group
            || app.args.workspaces
            || app.args.agents
            || !app.args.targets.is_empty()
            || !app.args.status.is_empty()
        {
            let kinds = app.args.kinds();
            if let Some(kind) = kinds.first() {
                app.browse = kind.clone();
            }
            match rotation::resolve(&app.snapshot, &kinds, &app.args.targets) {
                Ok(targets) => app.queues[0] = targets.iter().map(Identity::from).collect(),
                Err(error) => app.fail(format!("Selection: {error}")),
            }
        }
        if let Some(error) = baseline_error {
            app.queues = Default::default();
            app.fail(error);
        }
        app.refresh_jobs();
        if app.job.is_some() && !app.jobs.is_empty() {
            app.panel = JOBS;
        }
        app
    }
    fn info(&mut self, message: impl Into<String>) {
        self.message = message.into();
        self.error = false;
    }
    fn fail(&mut self, message: impl Into<String>) {
        self.message = message.into();
        self.error = true;
    }
    /// Queue edited from the active panel.
    fn lane(&self) -> usize {
        if self.split.is_some() && self.panel == RIGHT {
            RIGHT
        } else {
            LEFT
        }
    }
    /// Kind listed in a panel; `None` for the focus queue and jobs.
    fn panel_kind(&self, panel: usize) -> Option<Kind> {
        match (&self.split, panel) {
            (Some(kinds), LEFT | RIGHT) => Some(kinds[panel].clone()),
            (None, LEFT) => Some(self.browse.clone()),
            _ => None,
        }
    }
    fn row(&self) -> usize {
        self.rows[self.panel]
    }
    fn set_row(&mut self, row: usize) {
        self.rows[self.panel] = row;
    }
    fn matches(&self, id: &Identity) -> bool {
        let term = self.search.to_lowercase();
        term.is_empty()
            || self.target(id).is_some_and(|t| {
                format!(
                    "{} {} {} {} {}",
                    t.id,
                    t.label,
                    self.workspace_label(t).unwrap_or_default(),
                    t.agent_type.as_deref().unwrap_or(""),
                    t.status.as_deref().unwrap_or("")
                )
                .to_lowercase()
                .contains(&term)
            })
    }
    fn visible_in(&self, panel: usize) -> Vec<Identity> {
        let source: Vec<Identity> = match (self.panel_kind(panel), panel) {
            (Some(kind), _) => self
                .snapshot
                .targets
                .iter()
                .filter(|t| t.kind == kind)
                .map(Identity::from)
                .collect(),
            (None, RIGHT) => self.queues[LEFT].clone(),
            _ => return Vec::new(),
        };
        source.into_iter().filter(|id| self.matches(id)).collect()
    }
    fn visible(&self) -> Vec<Identity> {
        self.visible_in(self.panel)
    }
    fn target(&self, id: &Identity) -> Option<&Target> {
        self.snapshot
            .targets
            .iter()
            .find(|t| t.kind == id.kind && t.id == id.id)
    }
    fn workspace_label(&self, t: &Target) -> Option<String> {
        let id = if t.kind == Kind::Workspace {
            &t.id
        } else {
            t.workspace_id.as_ref()?
        };
        self.snapshot
            .targets
            .iter()
            .find(|w| w.kind == Kind::Workspace && w.id == *id)
            .map(|w| w.label.clone())
    }
    fn row_count(&self) -> usize {
        if self.panel == JOBS {
            self.jobs.len()
        } else {
            self.visible().len()
        }
    }
    fn panels(&self) -> usize {
        if self.jobs.is_empty() { 2 } else { 3 }
    }
    fn go_panel(&mut self, panel: usize) {
        if panel != self.panel {
            self.panel = panel;
            self.visual = None;
            self.pending_g = false;
        }
    }
    fn reset_rows(&mut self) {
        self.rows = [0; 3];
        self.visual = None;
        self.pending_g = false;
    }
    fn configure_split(&mut self, kinds: [Kind; 2], select_all: bool) {
        let selected = self.queues.iter().flatten().cloned().collect::<Vec<_>>();
        self.queues = std::array::from_fn(|lane| {
            let candidates = if select_all {
                self.snapshot.targets.iter().map(Identity::from).collect()
            } else {
                selected.clone()
            };
            let mut queue = Vec::new();
            for id in candidates {
                if id.kind == kinds[lane] && !queue.contains(&id) {
                    queue.push(id);
                }
            }
            queue
        });
        self.last_split = kinds.clone();
        self.split = Some(kinds);
        self.panel = LEFT;
        self.search.clear();
        self.reset_rows();
    }
    fn toggle_mode(&mut self) {
        if self.split.is_some() {
            self.split = None;
            self.args.close_on_exit = false;
            for id in std::mem::take(&mut self.queues[RIGHT]) {
                if !self.queues[LEFT].contains(&id) {
                    self.queues[LEFT].push(id);
                }
            }
            self.browse = self.last_split[LEFT].clone();
            self.panel = LEFT;
            self.reset_rows();
            self.info("Focus mode: Herdr focus moves through the queue on the right.");
        } else {
            self.configure_split(self.last_split.clone(), false);
            self.info("Split mode: a new tab mirrors the left and right lanes live.");
        }
    }
    fn cycle_kind(&mut self, forward: bool) {
        let step = |kind: &Kind| {
            let pos = KINDS.iter().position(|k| k == kind).unwrap_or(0);
            KINDS[(pos + if forward { 1 } else { 3 }) % 4].clone()
        };
        match (&mut self.split, self.panel) {
            (Some(kinds), LEFT | RIGHT) => {
                kinds[self.panel] = step(&kinds[self.panel]);
                self.last_split = kinds.clone();
                self.queues[self.panel].clear();
            }
            (None, LEFT) => self.browse = step(&self.browse),
            _ => return,
        }
        self.rows[self.panel] = 0;
        self.visual = None;
    }
    fn swap_lanes(&mut self) {
        if let Some(kinds) = &mut self.split {
            kinds.swap(LEFT, RIGHT);
            self.last_split = kinds.clone();
            self.queues.swap(LEFT, RIGHT);
            self.rows.swap(LEFT, RIGHT);
        }
    }
    fn toggle(&mut self) {
        if self.panel == JOBS {
            return;
        }
        let rows = self.visible();
        let first = self.visual.take().unwrap_or(self.row());
        let start = first.min(self.row());
        let end = first.max(self.row());
        let lane = self.lane();
        for id in rows.into_iter().skip(start).take(end - start + 1) {
            if let Some(i) = self.queues[lane].iter().position(|t| t == &id) {
                self.queues[lane].remove(i);
            } else {
                self.queues[lane].push(id);
            }
        }
        let len = self.row_count();
        self.set_row(self.row().min(len.saturating_sub(1)));
    }
    fn select_all(&mut self) {
        if self.panel_kind(self.panel).is_none() {
            return;
        }
        let lane = self.lane();
        for id in self.visible() {
            if !self.queues[lane].contains(&id) {
                self.queues[lane].push(id);
            }
        }
    }
    fn clear_selection(&mut self) {
        let lane = self.lane();
        match self.panel_kind(self.panel) {
            Some(kind) if self.split.is_none() => self.queues[lane].retain(|id| id.kind != kind),
            Some(_) => self.queues[lane].clear(),
            None if self.panel == RIGHT => self.queues[LEFT].clear(),
            None => {}
        }
        if self.panel == RIGHT && self.split.is_none() {
            self.set_row(0);
        }
    }
    fn reorder(&mut self, down: bool) {
        if self.split.is_some() || self.panel != RIGHT {
            return;
        }
        let Some(id) = self.visible().get(self.row()).cloned() else {
            return;
        };
        let queue = &mut self.queues[LEFT];
        if let Some(pos) = queue.iter().position(|t| *t == id) {
            let next = if down {
                (pos + 1).min(queue.len() - 1)
            } else {
                pos.saturating_sub(1)
            };
            queue.swap(pos, next);
            let row = self.visible().iter().position(|t| *t == id).unwrap_or(0);
            self.set_row(row);
        }
    }
    fn refresh(&mut self, snapshot: Snapshot) {
        let old: [Vec<Identity>; 2] = [self.visible_in(LEFT), self.visible_in(RIGHT)];
        let selected: [Option<Identity>; 2] =
            std::array::from_fn(|p| old[p].get(self.rows[p]).cloned());
        self.snapshot = snapshot;
        let mut removed = 0;
        for queue in &mut self.queues {
            let before = queue.len();
            queue.retain(|id| {
                self.snapshot
                    .targets
                    .iter()
                    .any(|t| t.kind == id.kind && t.id == id.id)
            });
            removed += before - queue.len();
        }
        for panel in [LEFT, RIGHT] {
            let rows = self.visible_in(panel);
            if panel == self.panel && rows != old[panel] {
                self.visual = None;
            }
            self.rows[panel] = selected[panel]
                .as_ref()
                .and_then(|id| rows.iter().position(|t| t == id))
                .unwrap_or_else(|| self.rows[panel].min(rows.len().saturating_sub(1)));
        }
        if removed > 0 {
            self.info(format!(
                "Removed {removed} selected target(s) that disappeared."
            ));
            self.visual = None;
        }
    }
    fn refresh_jobs(&mut self) {
        let selected = self.jobs.get(self.rows[JOBS]).map(|j| j.id.clone());
        match runtime::list() {
            Ok(jobs) => {
                self.jobs = jobs;
                self.rows[JOBS] = selected
                    .and_then(|id| self.jobs.iter().position(|j| j.id == id))
                    .unwrap_or_else(|| self.rows[JOBS].min(self.jobs.len().saturating_sub(1)));
                if self.jobs.is_empty() && self.panel == JOBS {
                    self.panel = LEFT;
                }
            }
            Err(error) => self.fail(format!("Job registry: {error}")),
        }
    }
    fn selected_job(&self) -> Option<String> {
        if self.panel == JOBS {
            self.jobs
                .get(self.rows[JOBS])
                .map(|j| j.id.clone())
                .or_else(|| self.job.clone())
        } else {
            self.job.clone()
        }
    }
    async fn job_action(&mut self, action: &str) {
        let Some(id) = self.selected_job() else {
            self.fail("No job yet. Start one with Enter, or pick one in Jobs (Tab).");
            return;
        };
        match runtime::control(action, Some(&id)).await {
            Ok(0) => self.fail(format!("No matching job for {action}: {id}")),
            Ok(_) => {
                let done = match action {
                    "pause" => "Paused",
                    "resume" => "Resumed",
                    "stop" => "Stopped",
                    _ => "Closed the view of",
                };
                self.info(format!("{done} job {id}."));
                self.job = Some(id);
            }
            Err(e) => self.fail(e),
        }
        self.refresh_jobs();
    }
    fn setting_name(field: usize) -> &'static str {
        [
            "Interval",
            "Iterations",
            "Random order",
            "Agent statuses",
            "Sidebar",
            "Keep sidebar change",
            "Close mirror on exit",
            "Desktop notification",
        ][field]
    }
    fn setting_hint(field: usize) -> &'static str {
        [
            "Seconds between moves; decimals allowed.",
            "Moves before stopping; 0 runs until stopped.",
            "Shuffle the order after every lap.",
            "Only visit agents in these states.",
            "Herdr sidebar while the rotation runs.",
            "Leave the sidebar as set when the job stops.",
            "Close the mirror tab when the job stops (split only).",
            "Notify the desktop when the job stops.",
        ][field]
    }
    fn setting_value(&self, field: usize) -> String {
        let on_off = |on: bool| if on { "on" } else { "off" }.to_owned();
        match field {
            0 => format!("{}s", self.args.interval),
            1 => {
                if self.args.iterations == 0 {
                    "unlimited".into()
                } else {
                    self.args.iterations.to_string()
                }
            }
            2 => on_off(self.args.random),
            3 => {
                if self.args.status.is_empty() {
                    "all".into()
                } else {
                    self.args.status.join(", ")
                }
            }
            4 => self
                .args
                .sidebar
                .clone()
                .unwrap_or_else(|| "unchanged".into()),
            5 => on_off(self.args.sidebar_keep),
            6 => on_off(self.args.close_on_exit),
            _ => on_off(!self.args.no_notify),
        }
    }
    fn edit_setting(&mut self) {
        match self.setting {
            0 | 1 | 3 => {
                self.input_error = None;
                self.input = match self.setting {
                    0 => self.args.interval.clone(),
                    1 => self.args.iterations.to_string(),
                    _ => self.args.status.join(","),
                };
                self.mode = Mode::Edit;
            }
            2 => self.args.random = !self.args.random,
            4 => {
                self.args.sidebar = match self.args.sidebar.as_deref() {
                    None => Some("hide".into()),
                    Some("hide") => Some("show".into()),
                    Some("show") => Some("toggle".into()),
                    _ => None,
                }
            }
            5 => self.args.sidebar_keep = !self.args.sidebar_keep,
            6 => {
                if self.split.is_some() {
                    self.args.close_on_exit = !self.args.close_on_exit;
                } else {
                    self.fail("Close mirror on exit only applies to split mode (m).");
                }
            }
            _ => self.args.no_notify = !self.args.no_notify,
        }
    }
    fn save_setting(&mut self) {
        let result = match self.setting {
            0 => rotation::parse_duration(&self.input)
                .map(|_| self.args.interval = self.input.trim().to_owned()),
            1 => self
                .input
                .trim()
                .parse::<usize>()
                .map(|n| self.args.iterations = n)
                .map_err(|_| "Use a whole number; 0 means unlimited.".into()),
            3 => {
                let values = self
                    .input
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                if values.iter().all(|s| {
                    ["idle", "working", "blocked", "done", "unknown"].contains(&s.as_str())
                }) {
                    self.args.status = values;
                    Ok(())
                } else {
                    Err("Use idle, working, blocked, done or unknown, comma separated.".into())
                }
            }
            _ => Ok(()),
        };
        match result {
            Ok(()) => {
                self.input_error = None;
                if self.mode == Mode::Interval {
                    self.info(format!("Interval set to {}s.", self.args.interval));
                    self.mode = Mode::Normal;
                } else {
                    self.mode = Mode::Settings;
                }
            }
            Err(e) => self.input_error = Some(e),
        }
    }
    fn launch_argv(&self, ctx: &Context) -> Result<Vec<String>, String> {
        let mut argv = vec![
            "--socket-path".into(),
            ctx.socket.to_string_lossy().into_owned(),
            "--seconds".into(),
            self.args.interval.clone(),
            "--iterations".into(),
            self.args.iterations.to_string(),
        ];
        if let Some(kinds) = &self.split {
            if let Some(lane) = self.queues.iter().position(Vec::is_empty) {
                return Err(format!(
                    "The {} lane is empty: select at least one {}.",
                    if lane == LEFT { "left" } else { "right" },
                    kind_name(&kinds[lane])
                ));
            }
            argv.extend([
                "--split".into(),
                format!("{},{}", short_kind(&kinds[0]), short_kind(&kinds[1])),
            ]);
            for lane in [LEFT, RIGHT] {
                argv.push(
                    if lane == LEFT {
                        "--left-targets"
                    } else {
                        "--right-targets"
                    }
                    .into(),
                );
                argv.push(
                    self.queues[lane]
                        .iter()
                        .map(selector)
                        .collect::<Vec<_>>()
                        .join(","),
                );
            }
            if self.args.close_on_exit {
                argv.push("--close-on-exit".into());
            }
        } else {
            if self.queues[LEFT].is_empty() {
                return Err("The queue is empty: select at least one target.".into());
            }
            for (kind, flag) in [
                (Kind::Workspace, "--workspaces"),
                (Kind::Tab, "--tabs"),
                (Kind::Agent, "--agents"),
            ] {
                if self.queues[LEFT].iter().any(|id| id.kind == kind) {
                    argv.push(flag.into());
                }
            }
            // Typed IDs preserve queue order and distinguish agent rows from pane rows.
            argv.extend([
                "--panes".into(),
                self.queues[LEFT]
                    .iter()
                    .map(selector)
                    .collect::<Vec<_>>()
                    .join(","),
            ]);
            if self.args.current_tab {
                argv.push("--active-tab".into());
            }
        }
        if self.args.random {
            argv.push("--random".into());
        }
        if !self.args.status.is_empty() {
            argv.extend(["--status".into(), self.args.status.join(",")]);
        }
        if let Some(mode) = &self.args.sidebar {
            argv.extend(["--sidebar".into(), mode.clone()]);
        }
        if self.args.sidebar_keep {
            argv.push("--sidebar-keep".into());
        }
        if self.args.no_notify {
            argv.push("--no-notify".into());
        }
        argv.push("--verbose".into());
        let parsed =
            Args::try_parse_from(std::iter::once("lictor".to_owned()).chain(argv.iter().cloned()))
                .map_err(|e| e.to_string())?;
        parsed.validate()?;
        for (lane, queue) in self.queues.iter().enumerate() {
            if self.split.is_none() && lane == RIGHT {
                continue;
            }
            if queue.iter().any(|id| self.target(id).is_none()) {
                return Err("A selected target disappeared; press R to refresh.".into());
            }
        }
        Ok(argv)
    }
}
fn set_baseline(snapshot: &mut Snapshot, selector: Option<&str>) -> Result<(), String> {
    let Some(value) = selector else {
        return Ok(());
    };
    let target = snapshot
        .targets
        .iter()
        .find(|t| {
            t.id == value
                || t.label == value
                || (t.kind == Kind::Workspace && t.number.is_some_and(|n| n.to_string() == value))
        })
        .ok_or_else(|| format!("Baseline target not found: {value}"))?;
    let (workspace, tab) = match target.kind {
        Kind::Workspace => (
            Some(target.id.clone()),
            snapshot
                .active_tabs
                .iter()
                .find(|(w, _)| *w == target.id)
                .map(|(_, t)| t.clone()),
        ),
        Kind::Tab => (target.workspace_id.clone(), Some(target.id.clone())),
        _ => (target.workspace_id.clone(), target.tab_id.clone()),
    };
    snapshot.baseline_workspace = workspace;
    snapshot.baseline_tab = tab;
    Ok(())
}

fn parse_kind(s: &str) -> Kind {
    match s {
        "w" => Kind::Workspace,
        "t" => Kind::Tab,
        "a" => Kind::Agent,
        _ => Kind::Pane,
    }
}
fn short_kind(k: &Kind) -> &'static str {
    match k {
        Kind::Workspace => "w",
        Kind::Tab => "t",
        Kind::Agent => "a",
        Kind::Pane => "p",
    }
}
fn kind_name(k: &Kind) -> &'static str {
    match k {
        Kind::Workspace => "workspace",
        Kind::Tab => "tab",
        Kind::Agent => "agent",
        Kind::Pane => "pane",
    }
}
fn kind_plural(k: &Kind) -> &'static str {
    match k {
        Kind::Workspace => "Workspaces",
        Kind::Tab => "Tabs",
        Kind::Agent => "Agents",
        Kind::Pane => "Panes",
    }
}
fn selector(id: &Identity) -> String {
    format!("{}={}", kind_name(&id.kind), id.id)
}

fn spawn_job(argv: &[String]) -> io::Result<(std::process::Child, std::path::PathBuf)> {
    use std::{
        fs,
        os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    };
    let uid = unsafe { libc::getuid() };
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute() && p.is_dir())
        .unwrap_or_else(std::env::temp_dir);
    let directory = base.join(format!("lictor-launches-{uid}"));
    match fs::DirBuilder::new().mode(0o700).create(&directory) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    let meta = fs::symlink_metadata(&directory)?;
    if !meta.is_dir()
        || meta.file_type().is_symlink()
        || meta.uid() != uid
        || meta.mode() & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "launch log directory must be private and owned by this user",
        ));
    }
    let path = directory.join(format!("{:016x}.log", rand::random::<u64>()));
    let file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&path)?;
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args(argv)
        .stdin(Stdio::null())
        .stdout(file.try_clone()?)
        .stderr(file);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // A controller exiting or losing its terminal must not send SIGHUP to its job.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    command.spawn().map(|child| (child, path))
}

// ---------------------------------------------------------------------------
// Drawing

/// Truncates to a display width, ending with an ellipsis when cut.
fn fit(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_owned();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w + 1 > width {
            break;
        }
        out.push(c);
        used += w;
    }
    if width > 0 {
        out.push('…');
    }
    out
}
fn pad(text: &str, width: usize) -> String {
    let text = fit(text, width);
    let gap = width.saturating_sub(text.width());
    format!("{text}{}", " ".repeat(gap))
}

fn status_style(status: &str) -> (&'static str, Color) {
    match status {
        "working" => ("◐", BLUE),
        "blocked" => ("■", RED),
        "done" => ("✓", GREEN),
        "idle" => ("○", SUBTLE),
        _ => ("·", MUTED),
    }
}

fn key_hint(key: &str, label: &str) -> Vec<Span<'static>> {
    vec![
        Span::styled(
            key.to_owned(),
            Style::default().fg(BLUE).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {label}   "), Style::default().fg(MUTED)),
    ]
}

fn panel_block(title: Line<'static>, count: String, active: bool) -> Block<'static> {
    Block::default()
        .title(title)
        .title_top(Line::from(count).right_aligned())
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if active { BLUE } else { MUTED }))
        .style(Style::default().bg(BG).fg(FG))
}

fn draw(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    frame.render_widget(Block::default().style(Style::default().bg(BG).fg(FG)), area);
    if area.width < 40 || area.height < 10 {
        frame.render_widget(
            Paragraph::new("Lictor needs at least 40 × 10.\nq quits · Ctrl-C stops the job.")
                .style(Style::default().fg(FG)),
            area,
        );
        return;
    }
    let jobs_height = if app.jobs.is_empty() {
        0
    } else {
        (app.jobs.len().min(3) as u16) + 2
    };
    let [header, body, jobs, message, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(4),
        Constraint::Length(jobs_height),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);
    draw_header(frame, app, header);
    let [left, right] = if body.width >= 72 {
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(body)
    } else {
        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(body)
    };
    draw_list(frame, app, LEFT, left);
    draw_list(frame, app, RIGHT, right);
    if jobs_height > 0 {
        draw_jobs(frame, app, jobs);
    }
    if !app.message.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    if app.error { " ✗ " } else { " › " },
                    Style::default().fg(if app.error { RED } else { GREEN }),
                ),
                Span::styled(
                    app.message.clone(),
                    Style::default().fg(if app.error { RED } else { FG }),
                ),
            ])),
            message,
        );
    }
    draw_footer(frame, app, footer);
    match app.mode {
        Mode::Help => draw_help(frame, app),
        Mode::Settings => draw_settings(frame, app),
        Mode::Edit | Mode::Interval => draw_input(frame, app),
        Mode::Normal | Mode::Search => {}
    }
}

fn draw_header(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let chip = |label: &str, on: bool| {
        Span::styled(
            format!(" {label} "),
            if on {
                Style::default()
                    .fg(BG)
                    .bg(PURPLE)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(MUTED)
            },
        )
    };
    let mut spans = vec![
        Span::styled(
            " LICTOR ",
            Style::default()
                .fg(BG)
                .bg(BLUE)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        chip("Focus", app.split.is_none()),
        chip("Split", app.split.is_some()),
        Span::styled("   ⏱ ", Style::default().fg(MUTED)),
        Span::styled(format!("{}s", app.args.interval), Style::default().fg(FG)),
        Span::styled("   ↻ ", Style::default().fg(MUTED)),
        Span::styled(
            if app.args.iterations == 0 {
                "∞".to_owned()
            } else {
                app.args.iterations.to_string()
            },
            Style::default().fg(FG),
        ),
    ];
    if app.args.random {
        spans.push(Span::styled("   ⤨ random", Style::default().fg(FG)));
    }
    if !app.args.status.is_empty() {
        spans.push(Span::styled(
            format!("   ◐ {}", app.args.status.join(",")),
            Style::default().fg(FG),
        ));
    }
    if !app.search.is_empty() || app.mode == Mode::Search {
        spans.push(Span::styled("   / ", Style::default().fg(YELLOW)));
        spans.push(Span::styled(app.search.clone(), Style::default().fg(FG)));
        if app.mode == Mode::Search {
            spans.push(Span::styled("▏", Style::default().fg(YELLOW)));
        }
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
    let running = app.jobs.iter().filter(|j| j.state == "running").count();
    let jobs = if app.jobs.is_empty() {
        Span::styled("no jobs ", Style::default().fg(MUTED))
    } else {
        Span::styled(
            format!(
                "● {} job{}{} ",
                app.jobs.len(),
                if app.jobs.len() == 1 { "" } else { "s" },
                if running == app.jobs.len() {
                    String::new()
                } else {
                    format!(" · {running} running")
                }
            ),
            Style::default().fg(if running > 0 { GREEN } else { YELLOW }),
        )
    };
    frame.render_widget(Paragraph::new(Line::from(jobs).right_aligned()), area);
}

fn panel_title(app: &App, panel: usize) -> Line<'static> {
    let active = app.panel == panel;
    let strong = Style::default()
        .fg(if active { BLUE } else { FG })
        .add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(MUTED);
    match (&app.split, panel) {
        (Some(kinds), _) => Line::from(vec![
            Span::styled(
                format!(
                    " {} ",
                    if panel == LEFT {
                        "◧ Left"
                    } else {
                        "◨ Right"
                    }
                ),
                strong,
            ),
            Span::styled("· ", dim),
            Span::styled(format!("{} ", kind_plural(&kinds[panel])), strong),
            Span::styled(if active { "▾ " } else { "" }, dim),
        ]),
        (None, LEFT) => Line::from(vec![
            Span::styled(" Targets ", strong),
            Span::styled("· ", dim),
            Span::styled(format!("{} ", kind_plural(&app.browse)), strong),
            Span::styled(if active { "▾ " } else { "" }, dim),
        ]),
        _ => Line::from(vec![
            Span::styled(" Queue ", strong),
            Span::styled("· rotation order ", dim),
            Span::styled(if active { "J/K move " } else { "" }, dim),
        ]),
    }
}

fn draw_list(frame: &mut Frame<'_>, app: &App, panel: usize, area: Rect) {
    let active = app.panel == panel;
    let rows = app.visible_in(panel);
    let lane = if app.split.is_some() { panel } else { LEFT };
    let queue = &app.queues[lane];
    let count = match app.panel_kind(panel) {
        Some(kind) => {
            let total = app.snapshot.by_kind(&kind).len();
            let chosen = queue.iter().filter(|id| id.kind == kind).count();
            format!(" {chosen}/{total} ")
        }
        None => format!(" {} ", queue.len()),
    };
    let block = panel_block(panel_title(app, panel), count, active);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if rows.is_empty() {
        let text = match app.panel_kind(panel) {
            _ if !app.search.is_empty() => format!("Nothing matches “{}”.", app.search),
            Some(kind) => format!("No {} in this session.", kind_plural(&kind).to_lowercase()),
            None => "Empty. Select targets on the left with ␣.".to_owned(),
        };
        frame.render_widget(
            Paragraph::new(text)
                .style(Style::default().fg(MUTED))
                .alignment(Alignment::Center),
            inner.inner(Margin::new(1, inner.height.saturating_sub(1) / 2)),
        );
        return;
    }
    let width = inner.width as usize;
    let height = inner.height as usize;
    let row = app.rows[panel].min(rows.len() - 1);
    let offset = (row + 1).saturating_sub(height);
    let primary_width = rows
        .iter()
        .filter_map(|id| app.target(id))
        .map(|t| primary_text(app, t).width())
        .max()
        .unwrap_or(8)
        .clamp(8, (width / 2).max(8));
    let in_focus_queue = app.split.is_none() && panel == RIGHT;
    let lines = rows
        .iter()
        .enumerate()
        .skip(offset)
        .take(height)
        .map(|(i, id)| {
            let current = active && i == row;
            let in_range = active
                && app
                    .visual
                    .is_some_and(|start| i >= start.min(row) && i <= start.max(row));
            let bg = if current || in_range { SELECT } else { BG };
            let Some(t) = app.target(id) else {
                return Line::from(Span::styled(
                    format!("  ✗ missing {}", id.id),
                    Style::default().fg(RED),
                ))
                .style(Style::default().bg(bg));
            };
            let position = queue.iter().position(|q| q == id);
            let mut spans = vec![Span::styled(
                if current { "▌" } else { " " },
                Style::default().fg(BLUE),
            )];
            if in_focus_queue {
                spans.push(Span::styled(
                    format!("{:>2} ", i + 1),
                    Style::default().fg(MUTED),
                ));
                spans.push(Span::styled(
                    format!("{} ", kind_icon(&t.kind)),
                    Style::default().fg(PURPLE),
                ));
            } else {
                spans.push(Span::styled(
                    if position.is_some() { "● " } else { "○ " },
                    Style::default().fg(if position.is_some() { GREEN } else { MUTED }),
                ));
            }
            spans.push(Span::styled(
                pad(&primary_text(app, t), primary_width),
                Style::default()
                    .fg(if current { BLUE } else { FG })
                    .add_modifier(if current {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ));
            let focused = app.snapshot.focused.as_deref() == Some(t.id.as_str())
                || app.snapshot.focused_tab.as_deref() == Some(t.id.as_str())
                || app.snapshot.focused_workspace.as_deref() == Some(t.id.as_str());
            let status = t
                .status
                .as_deref()
                .filter(|s| *s != "unknown" || t.kind == Kind::Agent);
            // Fixed-width columns keep statuses and lane positions aligned.
            let status_width = 10;
            let order_width = if app.split.is_some() { 4 } else { 0 };
            let tail_width = status_width + order_width;
            let mut tail = Vec::new();
            match status {
                Some(status) => {
                    let (icon, color) = status_style(status);
                    tail.push(Span::styled(
                        pad(&format!(" {icon} {status}"), status_width),
                        Style::default().fg(color),
                    ));
                }
                None => tail.push(Span::raw(" ".repeat(status_width))),
            }
            if app.split.is_some() {
                let text = position.map(|n| format!("#{}", n + 1)).unwrap_or_default();
                tail.push(Span::styled(
                    format!("{text:>order_width$}"),
                    Style::default().fg(MUTED),
                ));
            }
            let used = 3 + if in_focus_queue { 3 } else { 0 } + primary_width;
            let room = width.saturating_sub(used + tail_width + 1);
            let mut secondary = secondary_text(app, t);
            if focused {
                secondary = format!("◆ {secondary}");
            }
            spans.push(Span::styled(
                format!(" {}", pad(&secondary, room)),
                Style::default().fg(if focused { PURPLE } else { SUBTLE }),
            ));
            spans.extend(tail);
            Line::from(spans).style(Style::default().bg(bg))
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), inner);
}

fn kind_icon(kind: &Kind) -> &'static str {
    match kind {
        Kind::Workspace => "▣",
        Kind::Tab => "▭",
        Kind::Agent => "◉",
        Kind::Pane => "▢",
    }
}

fn primary_text(app: &App, t: &Target) -> String {
    match t.kind {
        Kind::Agent => t.agent_type.clone().unwrap_or_else(|| t.label.clone()),
        Kind::Tab if t.number.is_some_and(|n| n.to_string() == t.label) => {
            let ws = app.workspace_label(t).unwrap_or_default();
            format!("{ws} › tab {}", t.label)
        }
        _ => t.label.clone(),
    }
}

fn secondary_text(app: &App, t: &Target) -> String {
    let ws = app.workspace_label(t).unwrap_or_default();
    match t.kind {
        Kind::Workspace => t.id.clone(),
        Kind::Tab => ws,
        Kind::Agent => {
            if t.agent_type.as_deref() == Some(t.label.as_str()) {
                ws
            } else {
                format!("{ws} · {}", t.label)
            }
        }
        Kind::Pane => match t.pane_index {
            Some(n) => format!("{ws} · pane {n}"),
            None => ws,
        },
    }
}

fn draw_jobs(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let active = app.panel == JOBS;
    let title = Line::from(vec![Span::styled(
        " Jobs ",
        Style::default()
            .fg(if active { BLUE } else { FG })
            .add_modifier(Modifier::BOLD),
    )]);
    let hint = if active {
        " p pause · x stop · X close view "
    } else {
        " ↹ to manage "
    };
    let block = panel_block(title, hint.into(), active);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let row = app.rows[JOBS].min(app.jobs.len().saturating_sub(1));
    let offset = (row + 1).saturating_sub(inner.height as usize);
    let lines = app
        .jobs
        .iter()
        .enumerate()
        .skip(offset)
        .take(inner.height as usize)
        .map(|(i, job)| {
            let current = active && i == row;
            let color = match job.state.as_str() {
                "running" => GREEN,
                "paused" => YELLOW,
                _ => SUBTLE,
            };
            let mine = app.job.as_deref() == Some(job.id.as_str());
            Line::from(vec![
                Span::styled(if current { "▌" } else { " " }, Style::default().fg(BLUE)),
                Span::styled(format!("● {:<9}", job.state), Style::default().fg(color)),
                Span::styled(
                    format!(
                        "{:<7}",
                        if job.tab_id.is_some() {
                            "split"
                        } else {
                            "focus"
                        }
                    ),
                    Style::default().fg(FG),
                ),
                Span::styled(job.id.clone(), Style::default().fg(SUBTLE)),
                Span::styled(
                    job.tab_id
                        .as_ref()
                        .map(|t| format!("  mirror tab {t}"))
                        .unwrap_or_default(),
                    Style::default().fg(SUBTLE),
                ),
                Span::styled(
                    if mine { "  ◆ this session" } else { "" },
                    Style::default().fg(PURPLE),
                ),
            ])
            .style(Style::default().bg(if current { SELECT } else { BG }))
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_footer(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let (badge, color) = match app.mode {
        Mode::Search => (" SEARCH ", YELLOW),
        Mode::Edit | Mode::Interval | Mode::Settings => (" EDIT ", YELLOW),
        Mode::Help => (" HELP ", PURPLE),
        Mode::Normal if app.visual.is_some() => (" VISUAL ", PURPLE),
        Mode::Normal => (" NORMAL ", BLUE),
    };
    let mut spans = vec![
        Span::styled(
            badge,
            Style::default()
                .fg(BG)
                .bg(color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
    ];
    let hints: &[(&str, &str)] = match app.mode {
        Mode::Search => &[("⏎", "keep filter"), ("esc", "clear")],
        Mode::Edit | Mode::Interval => &[("⏎", "save"), ("esc", "cancel"), ("^u", "clear")],
        Mode::Settings => &[("j/k", "move"), ("⏎", "change"), ("esc", "done")],
        Mode::Help => &[("esc", "close")],
        Mode::Normal if app.visual.is_some() => {
            &[("j/k", "extend"), ("␣", "apply"), ("esc", "cancel")]
        }
        Mode::Normal if app.panel == JOBS => &[
            ("p", "pause/resume"),
            ("x", "stop"),
            ("X", "close view"),
            ("↹", "panel"),
            ("?", "help"),
        ],
        Mode::Normal if app.split.is_none() && app.panel == RIGHT => &[
            ("J/K", "reorder"),
            ("␣", "remove"),
            ("↹", "panel"),
            ("m", "split"),
            ("⏎", "start"),
            ("?", "help"),
        ],
        Mode::Normal if app.split.is_some() => &[
            ("␣", "select"),
            ("a", "all"),
            ("c", "kind"),
            ("↹", "lane"),
            ("m", "focus mode"),
            ("⏎", "start"),
            ("?", "help"),
        ],
        Mode::Normal => &[
            ("␣", "select"),
            ("a", "all"),
            ("c", "kind"),
            ("↹", "panel"),
            ("m", "split mode"),
            ("⏎", "start"),
            ("?", "help"),
        ],
    };
    for (key, label) in hints {
        spans.extend(key_hint(key, label));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn popup(frame: &mut Frame<'_>, width: u16, height: u16, title: &str, footer: &str) -> Rect {
    let area = frame.area();
    let width = width.min(area.width.saturating_sub(2));
    let height = height.min(area.height.saturating_sub(2));
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    let block = Block::default()
        .title(Span::styled(
            format!(" {title} "),
            Style::default().fg(BLUE).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(
            Line::from(Span::styled(
                format!(" {footer} "),
                Style::default().fg(MUTED),
            ))
            .right_aligned(),
        )
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BLUE))
        .style(Style::default().bg(SURFACE).fg(FG));
    let inner = block.inner(rect);
    frame.render_widget(Clear, rect);
    frame.render_widget(block, rect);
    inner.inner(Margin::new(1, 0))
}

type Section = (&'static str, &'static [(&'static str, &'static str)]);

fn help_sections(app: &App) -> Vec<Section> {
    let lanes: Section = if app.split.is_some() {
        (
            "Lanes",
            &[
                ("m", "focus mode"),
                ("c", "change lane kind"),
                ("s", "swap lanes"),
                ("t", "interval"),
                ("i", "settings"),
            ],
        )
    } else {
        (
            "Configure",
            &[
                ("m", "split mode"),
                ("c", "change kind"),
                ("J/K", "reorder queue"),
                ("t", "interval"),
                ("i", "settings"),
            ],
        )
    };
    vec![
        (
            "Navigate",
            &[
                ("j/k", "move"),
                ("h/l", "left / right"),
                ("↹", "next panel"),
                ("gg/G", "top / bottom"),
                ("^d/^u", "half page"),
                ("/", "filter"),
            ],
        ),
        (
            "Select",
            &[
                ("␣", "toggle"),
                ("v", "range"),
                ("a", "all visible"),
                ("u", "clear"),
            ],
        ),
        (
            "Run",
            &[
                ("⏎", "start"),
                ("p", "pause / resume"),
                ("x", "stop job"),
                ("X", "close mirror"),
                ("R", "refresh"),
            ],
        ),
        lanes,
        (
            "Leave",
            &[
                ("q", "quit, job continues"),
                ("^C", "stop job and exit"),
                ("esc", "cancel"),
            ],
        ),
    ]
}

fn draw_help(frame: &mut Frame<'_>, app: &App) {
    let sections = help_sections(app);
    let columns = if frame.area().width >= 84 { 3 } else { 1 };
    let col_width = 26usize;
    let mut grid: Vec<Vec<Line>> = Vec::new();
    for chunk in sections.chunks(columns) {
        let height = chunk.iter().map(|(_, keys)| keys.len()).max().unwrap_or(0) + 1;
        let mut block = vec![Vec::<Span>::new(); height];
        for (title, keys) in chunk {
            block[0].push(Span::styled(
                pad(title, col_width),
                Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
            ));
            for (row, line) in block.iter_mut().enumerate().skip(1) {
                match keys.get(row - 1) {
                    Some((key, label)) => {
                        line.push(Span::styled(
                            pad(key, 6),
                            Style::default().fg(BLUE).add_modifier(Modifier::BOLD),
                        ));
                        line.push(Span::styled(
                            pad(label, col_width - 6),
                            Style::default().fg(FG),
                        ));
                    }
                    None => line.push(Span::raw(" ".repeat(col_width))),
                }
            }
        }
        grid.push(block.into_iter().map(Line::from).collect());
    }
    let mut lines: Vec<Line> = Vec::new();
    for (i, block) in grid.into_iter().enumerate() {
        if i > 0 {
            lines.push(Line::default());
        }
        lines.extend(block);
    }
    let mode = if app.split.is_some() {
        "Split"
    } else {
        "Focus"
    };
    let width = (col_width * columns) as u16 + 4;
    let height = lines.len() as u16 + 2;
    let inner = popup(frame, width, height, &format!("Keys · {mode}"), "esc close");
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_settings(frame: &mut Frame<'_>, app: &App) {
    let width = 58u16;
    let inner = popup(
        frame,
        width,
        SETTINGS as u16 + 4,
        "Settings",
        "⏎ change · esc done",
    );
    let label_width = 22;
    let mut lines = (0..SETTINGS)
        .map(|i| {
            let current = app.setting == i;
            let disabled = i == 6 && app.split.is_none();
            Line::from(vec![
                Span::styled(if current { "▌" } else { " " }, Style::default().fg(BLUE)),
                Span::styled(
                    pad(App::setting_name(i), label_width),
                    Style::default().fg(if disabled { MUTED } else { FG }),
                ),
                Span::styled(
                    app.setting_value(i),
                    Style::default()
                        .fg(if disabled { MUTED } else { BLUE })
                        .add_modifier(Modifier::BOLD),
                ),
            ])
            .style(Style::default().bg(if current { SELECT } else { SURFACE }))
        })
        .collect::<Vec<_>>();
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        App::setting_hint(app.setting),
        Style::default().fg(SUBTLE),
    )));
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_input(frame: &mut Frame<'_>, app: &App) {
    let title = App::setting_name(app.setting);
    let inner = popup(frame, 52, 6, title, "⏎ save · esc cancel");
    let lines = vec![
        Line::from(vec![
            Span::styled("› ", Style::default().fg(BLUE)),
            Span::styled(app.input.clone(), Style::default().fg(FG)),
            Span::styled("▏", Style::default().fg(BLUE)),
            Span::styled(
                if app.setting == 0 { " seconds" } else { "" },
                Style::default().fg(MUTED),
            ),
        ]),
        Line::default(),
        match &app.input_error {
            Some(error) => Line::from(Span::styled(format!("✗ {error}"), Style::default().fg(RED))),
            None => Line::from(Span::styled(
                App::setting_hint(app.setting),
                Style::default().fg(SUBTLE),
            )),
        },
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

// ---------------------------------------------------------------------------
// Input

/// Returns true when the controller should leave; jobs have their own lifetime.
async fn key(app: &mut App, ctx: &Context, key: KeyEvent, page: usize) -> Result<bool, Error> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        if app.selected_job().is_some() {
            app.job_action("stop").await;
        }
        return Ok(true);
    }
    match app.mode {
        Mode::Search => {
            match key.code {
                KeyCode::Esc => {
                    app.search.clear();
                    app.mode = Mode::Normal;
                }
                KeyCode::Enter => app.mode = Mode::Normal,
                KeyCode::Backspace => {
                    app.search.pop();
                    app.reset_rows();
                }
                KeyCode::Char(c) => {
                    app.search.push(c);
                    app.reset_rows();
                }
                _ => {}
            }
            return Ok(false);
        }
        Mode::Edit | Mode::Interval => {
            match key.code {
                KeyCode::Esc => {
                    app.mode = if app.mode == Mode::Interval {
                        Mode::Normal
                    } else {
                        Mode::Settings
                    };
                    app.input_error = None;
                }
                KeyCode::Enter => app.save_setting(),
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.input.clear();
                    app.input_error = None;
                }
                KeyCode::Backspace => {
                    app.input.pop();
                    app.input_error = None;
                }
                KeyCode::Char(c) => {
                    app.input.push(c);
                    app.input_error = None;
                }
                _ => {}
            }
            return Ok(false);
        }
        Mode::Help => {
            if matches!(
                key.code,
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') | KeyCode::Enter
            ) {
                app.mode = Mode::Normal;
            }
            return Ok(false);
        }
        Mode::Settings => {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('i') => app.mode = Mode::Normal,
                KeyCode::Char('j') | KeyCode::Down => app.setting = (app.setting + 1) % SETTINGS,
                KeyCode::Char('k') | KeyCode::Up => {
                    app.setting = (app.setting + SETTINGS - 1) % SETTINGS
                }
                KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Char('h') | KeyCode::Char('l') => {
                    app.edit_setting()
                }
                _ => {}
            }
            return Ok(false);
        }
        Mode::Normal => {}
    }
    let len = app.row_count();
    let last = len.saturating_sub(1);
    match key.code {
        KeyCode::Char('q') => return Ok(true),
        KeyCode::Esc => {
            if app.visual.take().is_none() {
                if app.search.is_empty() {
                    app.message.clear();
                } else {
                    app.search.clear();
                    app.reset_rows();
                }
            }
        }
        KeyCode::Char('?') => app.mode = Mode::Help,
        KeyCode::Char('/') => {
            app.mode = Mode::Search;
            app.search.clear();
            app.reset_rows();
        }
        KeyCode::Char('i') => app.mode = Mode::Settings,
        KeyCode::Char('t') => {
            app.setting = 0;
            app.input = app.args.interval.clone();
            app.input_error = None;
            app.mode = Mode::Interval;
        }
        KeyCode::Char('m') => app.toggle_mode(),
        KeyCode::Char('c') => app.cycle_kind(true),
        KeyCode::Char('C') => app.cycle_kind(false),
        KeyCode::Char('s') => app.swap_lanes(),
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.set_row(app.row().saturating_add(page / 2).min(last))
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.set_row(app.row().saturating_sub(page / 2))
        }
        KeyCode::Char('j') | KeyCode::Down => app.set_row(app.row().saturating_add(1).min(last)),
        KeyCode::Char('k') | KeyCode::Up => app.set_row(app.row().saturating_sub(1)),
        KeyCode::Char('h') | KeyCode::Left => app.go_panel(LEFT),
        KeyCode::Char('l') | KeyCode::Right => app.go_panel(RIGHT),
        KeyCode::Tab => app.go_panel((app.panel + 1) % app.panels()),
        KeyCode::BackTab => app.go_panel((app.panel + app.panels() - 1) % app.panels()),
        KeyCode::Char('g') => {
            if app.pending_g {
                app.set_row(0);
                app.pending_g = false;
            } else {
                app.pending_g = true;
            }
        }
        KeyCode::Char('G') => app.set_row(last),
        KeyCode::Char('v') if app.panel != JOBS => {
            app.visual = if app.visual.is_some() {
                None
            } else {
                Some(app.row())
            }
        }
        KeyCode::Char(' ') => app.toggle(),
        KeyCode::Char('a') => app.select_all(),
        KeyCode::Char('u') => app.clear_selection(),
        KeyCode::Char('J') => app.reorder(true),
        KeyCode::Char('K') => app.reorder(false),
        KeyCode::Char('p') => {
            let id = app.selected_job();
            let paused = id
                .as_ref()
                .and_then(|id| app.jobs.iter().find(|j| j.id == *id))
                .is_some_and(|j| j.state == "paused");
            app.job_action(if paused { "resume" } else { "pause" })
                .await;
        }
        KeyCode::Char('R') => {
            let mut snapshot = Snapshot::from_value(&ctx.snapshot().await?);
            runtime::filter_owned(&mut snapshot, &ctx.socket);
            app.refresh(snapshot);
            app.refresh_jobs();
            app.info("Targets refreshed.");
        }
        KeyCode::Char('x') => app.job_action("stop").await,
        KeyCode::Char('X') => app.job_action("close-view").await,
        KeyCode::Enter if app.panel == JOBS => app.job_action("resume").await,
        KeyCode::Enter => start(app, ctx).await?,
        _ => {}
    }
    if key.code != KeyCode::Char('g') {
        app.pending_g = false;
    }
    Ok(false)
}

/// Launches the selection as a detached job; the controller stays open to manage it.
async fn start(app: &mut App, ctx: &Context) -> Result<(), Error> {
    let argv = match app.launch_argv(ctx) {
        Ok(argv) => argv,
        Err(error) => {
            app.fail(error);
            return Ok(());
        }
    };
    let (mut child, log) = spawn_job(&argv)?;
    let pid = child.id();
    let split = app.split.is_some();
    for _ in 0..40 {
        if let Some(status) = child.try_wait()? {
            let details = std::fs::read_to_string(&log).unwrap_or_default();
            let details = details.lines().last().unwrap_or("").trim().to_owned();
            if status.success() {
                app.info(format!("Job finished: {details}"));
            } else {
                app.fail(format!(
                    "Job failed ({status}): {details} — log {}",
                    log.display()
                ));
            }
            app.refresh_jobs();
            return Ok(());
        }
        app.refresh_jobs();
        if let Some(job) = app.jobs.iter().find(|j| j.pid == pid) {
            let id = job.id.clone();
            app.job = Some(id.clone());
            app.started.push(id.clone());
            app.info(if split {
                format!("Started {id}. The mirror opens in a new Herdr tab; p pauses, x stops.")
            } else {
                format!("Started {id}. Herdr focus now rotates; p pauses, x stops.")
            });
            // Reap finite jobs without blocking or cancelling persistent jobs.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    app.info(format!(
        "Job PID {pid} started; it will appear under Jobs. Log: {}",
        log.display()
    ));
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

pub async fn run(args: Args, ctx: Context) -> Result<(), Error> {
    let mut snapshot = Snapshot::from_value(&ctx.snapshot().await?);
    runtime::filter_owned(&mut snapshot, &ctx.socket);
    let mut app = App::new(args, snapshot);
    if app.job.is_some() {
        app.job_action("pause").await;
    }
    enable_raw_mode()?;
    let restore = Restore;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let refresh_ctx = ctx.clone();
    let refresh_task = tokio::spawn(async move {
        loop {
            tokio::select! { _ = refresh_ctx.cancel.cancelled() => break, _ = tokio::time::sleep(Duration::from_secs(2)) => {} }
            let result = refresh_ctx.snapshot().await.map(|v| {
                let mut snapshot = Snapshot::from_value(&v);
                runtime::filter_owned(&mut snapshot, &refresh_ctx.socket);
                snapshot
            });
            if tx.send(result).await.is_err() {
                break;
            }
        }
    });
    let mut last_jobs = Instant::now();
    let result = async {
        loop {
            if ctx.cancel.is_cancelled() {
                if app.selected_job().is_some() {
                    app.job_action("stop").await;
                }
                break;
            }
            if let Ok(result) = rx.try_recv() {
                match result {
                    Ok(snapshot) => app.refresh(snapshot),
                    Err(error) => app.fail(format!("Refresh failed: {error}")),
                }
            }
            if last_jobs.elapsed() > Duration::from_millis(500) {
                app.refresh_jobs();
                last_jobs = Instant::now();
            }
            terminal.draw(|frame| draw(frame, &app))?;
            if event::poll(Duration::from_millis(80))?
                && let Event::Key(event) = event::read()?
                && event.kind == KeyEventKind::Press
            {
                let page = terminal.size()?.height.saturating_sub(8) as usize;
                if key(&mut app, &ctx, event, page).await? {
                    break;
                }
            }
        }
        Ok(())
    }
    .await;
    refresh_task.abort();
    drop(terminal);
    drop(restore);
    let live: Vec<_> = app
        .started
        .iter()
        .filter(|id| app.jobs.iter().any(|j| &j.id == *id))
        .collect();
    for id in live {
        println!("lictor: job {id} keeps running; manage it with: lictor --tui --job {id}");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> App {
        let args = Args::parse_from(["lictor"]);
        let snapshot = Snapshot {
            targets: (0..3)
                .map(|n| Target {
                    kind: Kind::Workspace,
                    id: format!("w{n}"),
                    label: format!("Work {n}"),
                    ..Target::default()
                })
                .collect(),
            ..Snapshot::default()
        };
        App::new(args, snapshot)
    }
    fn ctx() -> Context {
        Context::discover(Some("/tmp/test.sock".into()), None)
    }
    async fn press(app: &mut App, codes: impl IntoIterator<Item = KeyCode>) {
        let ctx = ctx();
        for code in codes {
            assert!(
                !key(app, &ctx, KeyEvent::new(code, KeyModifiers::NONE), 10)
                    .await
                    .unwrap()
            );
        }
    }
    fn render(app: &App, width: u16, height: u16) -> String {
        let mut terminal =
            Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    #[test]
    fn selections_follow_identity_after_refresh_and_remove_missing() {
        let mut app = app();
        app.set_row(1);
        app.toggle();
        let mut snapshot = app.snapshot.clone();
        snapshot.targets.reverse();
        snapshot.targets[1].label = "Renamed".into();
        app.refresh(snapshot.clone());
        assert_eq!(app.queues[0][0].id, "w1");
        assert_eq!(app.visible()[app.row()].id, "w1");
        snapshot.targets.retain(|t| t.id != "w1");
        app.refresh(snapshot);
        assert!(app.queues[0].is_empty());
    }
    #[test]
    fn visual_and_filtered_selection_do_not_select_hidden_rows() {
        let mut app = app();
        app.visual = Some(0);
        app.set_row(1);
        app.toggle();
        assert_eq!(app.queues[0].len(), 2);
        app.search = "Work 2".into();
        app.set_row(0);
        app.toggle();
        assert_eq!(app.queues[0].len(), 3);
    }
    #[test]
    fn launch_preserves_queue_order_and_validates_both_lanes() {
        let mut app = app();
        app.set_row(2);
        app.toggle();
        app.set_row(0);
        app.toggle();
        let argv = app.launch_argv(&ctx()).unwrap();
        assert!(argv.contains(&"workspace=w2,workspace=w0".to_owned()));
        app.split = Some([Kind::Workspace, Kind::Workspace]);
        assert!(app.launch_argv(&ctx()).is_err());
    }
    #[tokio::test]
    async fn interval_shortcut_validates_cancels_and_sets_launch_seconds() {
        let mut app = app();
        let original = app.args.interval.clone();
        press(
            &mut app,
            [
                KeyCode::Char('t'),
                KeyCode::Backspace,
                KeyCode::Char('0'),
                KeyCode::Enter,
            ],
        )
        .await;
        assert_eq!(app.mode, Mode::Interval);
        assert!(app.input_error.is_some());
        assert_eq!(app.args.interval, original);
        press(&mut app, [KeyCode::Esc]).await;
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.args.interval, original);

        press(&mut app, [KeyCode::Char('t')]).await;
        key(
            &mut app,
            &ctx(),
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
            10,
        )
        .await
        .unwrap();
        press(
            &mut app,
            [
                KeyCode::Char('2'),
                KeyCode::Char(','),
                KeyCode::Char('5'),
                KeyCode::Enter,
            ],
        )
        .await;
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.args.interval, "2,5");
        app.toggle();
        let argv = app.launch_argv(&ctx()).unwrap();
        let index = argv.iter().position(|arg| arg == "--seconds").unwrap();
        assert_eq!(argv[index + 1], "2,5");

        press(&mut app, [KeyCode::Char('i'), KeyCode::Enter]).await;
        assert_eq!(app.input, "2,5");
        press(&mut app, [KeyCode::Enter]).await;
        assert_eq!(app.mode, Mode::Settings);
    }
    #[test]
    fn header_and_footer_show_mode_interval_and_key_hints() {
        let app = app();
        let screen = render(&app, 100, 24);
        assert!(screen.contains("LICTOR"));
        assert!(screen.contains(&format!("⏱ {}s", app.args.interval)));
        assert!(screen.contains("NORMAL"));
        assert!(screen.contains("m split mode"));
        assert!(screen.contains("Targets · Workspaces"));
        assert!(screen.contains("Queue · rotation order"));
    }
    #[test]
    fn split_board_shows_both_lanes_side_by_side() {
        let mut app = app();
        app.snapshot.targets.push(Target {
            kind: Kind::Agent,
            id: "p1".into(),
            label: "claude".into(),
            agent_type: Some("claude".into()),
            status: Some("blocked".into()),
            ..Target::default()
        });
        app.configure_split([Kind::Workspace, Kind::Agent], true);
        let screen = render(&app, 100, 20);
        let lanes = screen.lines().find(|line| line.contains("◧ Left")).unwrap();
        assert!(lanes.contains("◨ Right · Agents"));
        assert!(screen.contains("3/3"));
        assert!(screen.contains("■ blocked"));
        // Narrow terminals stack the lanes instead.
        let narrow = render(&app, 60, 24);
        assert!(
            !narrow
                .lines()
                .any(|line| line.contains("◧ Left") && line.contains("◨ Right"))
        );
    }
    #[test]
    fn help_popup_groups_keys_by_purpose() {
        let mut app = app();
        app.mode = Mode::Help;
        let screen = render(&app, 100, 30);
        for heading in ["Navigate", "Select", "Run", "Configure", "Leave"] {
            assert!(screen.contains(heading), "{heading}");
        }
        assert!(screen.contains("Keys · Focus"));
    }
    #[tokio::test]
    async fn mode_key_reaches_every_lane_kind_combination_in_either_order() {
        let kinds = [Kind::Workspace, Kind::Tab, Kind::Agent, Kind::Pane];
        for left in 0..4 {
            for right in 0..4 {
                let mut app = app();
                app.args.interval = "5".into();
                for (kind, id) in [(Kind::Tab, "t1"), (Kind::Agent, "p1"), (Kind::Pane, "p1")] {
                    app.snapshot.targets.push(Target {
                        kind,
                        id: id.into(),
                        ..Target::default()
                    });
                }
                // m enters split (workspace | agent); c cycles the active lane's kind.
                let mut codes = vec![KeyCode::Char('m')];
                codes.extend(std::iter::repeat_n(KeyCode::Char('c'), left));
                codes.push(KeyCode::Char('a'));
                codes.push(KeyCode::Char('l'));
                codes.extend(std::iter::repeat_n(KeyCode::Char('c'), (right + 2) % 4));
                codes.push(KeyCode::Char('a'));
                press(&mut app, codes).await;
                assert_eq!(app.mode, Mode::Normal);
                assert_eq!(app.split, Some([kinds[left].clone(), kinds[right].clone()]));
                for (lane, kind) in [(0, &kinds[left]), (1, &kinds[right])] {
                    assert_eq!(app.queues[lane].len(), app.snapshot.by_kind(kind).len());
                    assert!(app.queues[lane].iter().all(|id| id.kind == *kind));
                }
                let argv = app.launch_argv(&ctx()).unwrap();
                let args =
                    Args::try_parse_from(std::iter::once("lictor".to_owned()).chain(argv)).unwrap();
                assert_eq!(
                    args.split,
                    Some(format!(
                        "{},{}",
                        short_kind(&kinds[left]),
                        short_kind(&kinds[right])
                    ))
                );
                assert_eq!(args.interval, "5");
                assert!(!args.left_targets.is_empty() && !args.right_targets.is_empty());
            }
        }
    }
    #[tokio::test]
    async fn cli_split_flags_prefill_both_lanes_for_tui() {
        let args = Args::parse_from(["lictor", "--tui", "-W", "-A", "-s", "5", "-S", "w,a"]);
        let mut snapshot = app().snapshot;
        snapshot.targets.push(Target {
            kind: Kind::Agent,
            id: "p1".into(),
            ..Target::default()
        });
        let app = App::new(args, snapshot);
        assert_eq!(app.split, Some([Kind::Workspace, Kind::Agent]));
        assert_eq!(app.queues[0].len(), 3);
        assert_eq!(app.queues[1].len(), 1);
        let argv = app.launch_argv(&ctx()).unwrap();
        assert!(argv.windows(2).any(|w| w == ["--split", "w,a"]));
        assert!(argv.windows(2).any(|w| w == ["--seconds", "5"]));
    }
    #[tokio::test]
    async fn split_preserves_selected_targets_and_swaps_lanes() {
        let mut app = app();
        app.snapshot.targets.push(Target {
            kind: Kind::Agent,
            id: "p1".into(),
            ..Target::default()
        });
        app.set_row(1);
        app.toggle();
        app.browse = Kind::Agent;
        app.set_row(0);
        app.toggle();
        press(&mut app, [KeyCode::Char('m'), KeyCode::Char('s')]).await;
        assert_eq!(app.split, Some([Kind::Agent, Kind::Workspace]));
        assert_eq!(app.queues[0][0].id, "p1");
        assert_eq!(app.queues[1][0].id, "w1");
        press(&mut app, [KeyCode::Char('m')]).await;
        assert!(app.split.is_none());
        assert_eq!(app.panel, LEFT);
        assert_eq!(app.queues[0].len(), 2);
        assert!(app.queues[1].is_empty());
        // The lane kinds come back when returning to split mode.
        press(&mut app, [KeyCode::Char('m')]).await;
        assert_eq!(app.split, Some([Kind::Agent, Kind::Workspace]));
    }
    #[tokio::test]
    async fn focus_queue_reorders_and_removes() {
        let mut app = app();
        press(&mut app, [KeyCode::Char('a'), KeyCode::Char('l')]).await;
        assert_eq!(app.visible().len(), 3);
        press(&mut app, [KeyCode::Char('J')]).await;
        assert_eq!(app.queues[0][1].id, "w0");
        assert_eq!(app.row(), 1);
        press(&mut app, [KeyCode::Char(' ')]).await;
        assert_eq!(app.queues[0].len(), 2);
        assert!(app.queues[0].iter().all(|id| id.id != "w0"));
    }
    #[tokio::test]
    async fn search_q_and_escape_do_not_quit_controller() {
        let mut app = app();
        app.mode = Mode::Search;
        press(&mut app, [KeyCode::Char('q')]).await;
        assert_eq!(app.search, "q");
        press(&mut app, [KeyCode::Esc]).await;
        assert!(app.search.is_empty());
    }
}
