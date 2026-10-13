mod cli;
mod domain;
mod herdr;
mod mirror;
mod platform;
mod rotation;
mod runtime;
mod tui;

use clap::Parser;
use cli::Args;
use domain::{Kind, Snapshot, Target};
use herdr::Context;
use rand::seq::SliceRandom;
use std::{
    collections::HashSet,
    io::{self, IsTerminal},
    process::ExitCode,
};

#[tokio::main]
async fn main() -> ExitCode {
    let mut argv = std::env::args().skip(1);
    if argv.next().as_deref() == Some("__viewer") {
        return viewer(std::env::args().skip(2).collect()).await;
    }
    let args = Args::parse();
    if let Some(what) = &args.generate {
        return match cli::generate(what, &mut io::stdout()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("lictor: {error}");
                ExitCode::from(1)
            }
        };
    }
    if let Err(e) = args.validate() {
        // Reported like clap's own errors: same style, usage tip and exit code 2.
        use clap::CommandFactory;
        Args::command()
            .error(clap::error::ErrorKind::ArgumentConflict, e)
            .exit();
    }
    if args.jobs {
        return match runtime::list() {
            Ok(jobs) if jobs.is_empty() => {
                println!("No Lictor jobs.");
                ExitCode::SUCCESS
            }
            Ok(jobs) => {
                println!("{:<26} {:<10} {:<6} MIRROR TAB", "JOB", "STATE", "MODE");
                for job in jobs {
                    println!(
                        "{:<26} {:<10} {:<6} {}",
                        job.id,
                        job.state,
                        if job.tab_id.is_some() {
                            "split"
                        } else {
                            "focus"
                        },
                        job.tab_id.as_deref().unwrap_or("-")
                    );
                }
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("lictor: {error}");
                ExitCode::from(1)
            }
        };
    }
    if args.stop {
        return match runtime::control("stop", args.job.as_deref()).await {
            Ok(0) => {
                eprintln!("lictor: no matching active rotation job");
                ExitCode::from(3)
            }
            Ok(count) => {
                println!("Stopped {count} rotation job(s)");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("lictor: {error}");
                ExitCode::from(3)
            }
        };
    }
    let ctx = Context::discover(args.socket_path.clone(), args.session.clone());
    install_signals(ctx.clone()).await;
    if args.tui || (std::env::args_os().len() == 1) {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            eprintln!(
                "lictor: no arguments opens the TUI; pass -p, -T, -W, or -A for non-interactive use"
            );
            return ExitCode::from(2);
        }
        return match tui::run(args, ctx).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("lictor: {e}");
                ExitCode::from(1)
            }
        };
    }
    if args.pause || args.resume || args.close_view {
        let action = if args.pause {
            "pause"
        } else if args.resume {
            "resume"
        } else {
            "close-view"
        };
        return match runtime::control(action, args.job.as_deref()).await {
            Ok(0) => {
                eprintln!("lictor: no matching job or retained view");
                ExitCode::from(3)
            }
            Ok(count) => {
                println!("{action}: {count} job(s)");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("lictor: {error}");
                ExitCode::from(3)
            }
        };
    }
    if let Some(id) = args.job.as_deref() {
        return match runtime::list() {
            Ok(jobs) => match jobs.into_iter().find(|job| job.id == id) {
                Some(job) => {
                    println!(
                        "{}\t{}\t{}\t{}",
                        job.id,
                        job.state,
                        job.pid,
                        job.tab_id.as_deref().unwrap_or("-")
                    );
                    ExitCode::SUCCESS
                }
                None => {
                    eprintln!("lictor: no job found with ID {id}");
                    ExitCode::from(3)
                }
            },
            Err(error) => {
                eprintln!("lictor: {error}");
                ExitCode::from(3)
            }
        };
    }
    let raw = match ctx.snapshot().await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("lictor: {e}");
            return ExitCode::from(4);
        }
    };
    let mut snap = Snapshot::from_value(&raw);
    runtime::filter_owned(&mut snap, &ctx.socket);
    if let Some(selector) = &args.target {
        let Some(target) = snap
            .targets
            .iter()
            .find(|t| t.id == *selector || t.label == *selector)
        else {
            eprintln!("lictor: --target did not match a live Herdr target: {selector}");
            return ExitCode::from(3);
        };
        match target.kind {
            Kind::Workspace => {
                snap.baseline_workspace = Some(target.id.clone());
                snap.baseline_tab = snap
                    .active_tabs
                    .iter()
                    .find(|(w, _)| w == &target.id)
                    .map(|(_, t)| t.clone());
            }
            Kind::Tab => {
                snap.baseline_workspace = target.workspace_id.clone();
                snap.baseline_tab = Some(target.id.clone());
            }
            Kind::Pane | Kind::Agent => {
                snap.baseline_workspace = target.workspace_id.clone();
                snap.baseline_tab = target.tab_id.clone();
            }
        }
    }
    if args.list {
        let kinds = args.kinds();
        let allowed: HashSet<String> = args.status.iter().map(|s| s.to_lowercase()).collect();
        let rows = snap.targets.iter().filter(|t| {
            if !kinds.contains(&t.kind) {
                return false;
            }
            if t.kind == Kind::Agent
                && !allowed.is_empty()
                && !t
                    .status
                    .as_ref()
                    .is_some_and(|s| allowed.contains(&s.to_lowercase()))
            {
                return false;
            }
            if args.all || args.workspaces || args.agents || t.kind == Kind::Workspace {
                return true;
            }
            match t.kind {
                Kind::Pane | Kind::Agent => snap
                    .baseline_tab
                    .as_ref()
                    .is_none_or(|tab| t.tab_id.as_ref() == Some(tab)),
                Kind::Tab => snap
                    .baseline_workspace
                    .as_ref()
                    .is_none_or(|ws| t.workspace_id.as_ref() == Some(ws)),
                Kind::Workspace => true,
            }
        });
        for t in rows {
            println!("{}\t{}\t{}", kind_label(&t.kind), t.id, t.label)
        }
        return ExitCode::SUCCESS;
    }
    if let Some(split) = &args.split {
        return split_mode(&args, &ctx, &snap, split).await;
    }
    let kinds = args.kinds();
    let mut queue = match rotation::resolve(&snap, &kinds, &args.targets) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("lictor: {e}");
            return ExitCode::from(3);
        }
    };
    if args.targets.is_empty() {
        queue.retain(|target| match target.kind {
            Kind::Pane => snap
                .baseline_tab
                .as_ref()
                .is_none_or(|tab| target.tab_id.as_ref() == Some(tab)),
            Kind::Tab => snap
                .baseline_workspace
                .as_ref()
                .is_none_or(|ws| target.workspace_id.as_ref() == Some(ws)),
            Kind::Workspace | Kind::Agent => true,
        });
    }
    if args.workspaces && args.agents {
        for target in &mut queue {
            if target.kind == Kind::Workspace {
                let tab = if args.current_tab {
                    let active = snap
                        .active_tabs
                        .iter()
                        .find(|(workspace, _)| workspace == &target.id)
                        .map(|(_, tab)| tab);
                    if active.is_some_and(|id| runtime::owned_view_ids(&ctx.socket).0.contains(id))
                    {
                        snap.targets
                            .iter()
                            .filter(|candidate| {
                                candidate.kind == Kind::Tab
                                    && candidate.workspace_id.as_deref() == Some(&target.id)
                                    && !runtime::owned_view_ids(&ctx.socket)
                                        .0
                                        .contains(&candidate.id)
                            })
                            .max_by_key(|candidate| candidate.number)
                    } else {
                        None
                    }
                } else {
                    snap.targets
                        .iter()
                        .filter(|t| {
                            t.kind == Kind::Tab && t.workspace_id.as_deref() == Some(&target.id)
                        })
                        .min_by_key(|t| t.number)
                };
                if let Some(tab) = tab {
                    *target = tab.clone();
                }
            }
        }
    }
    if args.dry_run {
        let count = if args.iterations == 0 {
            5
        } else {
            args.iterations
        };
        println!("Dry run against one Herdr snapshot; dynamic status changes cannot be predicted.");
        println!("Mode: focus rotation");
        let mut preview = queue
            .iter()
            .filter(|t| {
                t.kind != Kind::Agent
                    || args.status.is_empty()
                    || t.status
                        .as_ref()
                        .is_some_and(|s| args.status.iter().any(|wanted| wanted == s))
            })
            .cloned()
            .collect::<Vec<_>>();
        if preview.is_empty() {
            println!(
                "No selected agents currently match --status; rotation will wait for a live match."
            );
            return ExitCode::SUCCESS;
        }
        let mut rng = rand::rng();
        for index in 0..count {
            if args.random && index > 0 && index % preview.len() == 0 {
                preview.shuffle(&mut rng);
            }
            let t = &preview[index % preview.len()];
            println!("{} {} ({})", kind_label(&t.kind), t.id, t.label)
        }
        return ExitCode::SUCCESS;
    }
    let mut sidebar = if let Some(mode) = &args.sidebar {
        match platform::SidebarGuard::apply(
            &ctx,
            mode,
            args.sidebar_keep,
            args.verbose && !args.quiet,
        )
        .await
        {
            Ok(guard) => Some(guard),
            Err(error) => {
                eprintln!("lictor: sidebar: {error}; continuing rotation");
                None
            }
        }
    } else {
        None
    };
    let statuses: HashSet<String> = args.status.iter().map(|s| s.to_lowercase()).collect();
    let job = match runtime::register(&ctx) {
        Ok(job) => job,
        Err(error) => {
            eprintln!("lictor: cannot manage rotation lifecycle: {error}");
            return ExitCode::from(3);
        }
    };
    if args.verbose && !args.quiet {
        eprintln!("lictor: rotation job {}", job.id);
    }
    let result = rotation::run(
        &ctx,
        queue,
        args.interval().unwrap(),
        args.iterations,
        args.random,
        &statuses,
        args.verbose && !args.quiet,
    )
    .await;
    if let Some(guard) = &mut sidebar {
        for warning in guard.restore().await {
            eprintln!("lictor: {warning}");
        }
    }
    match result {
        Ok(changes) => {
            platform::notify_stop(changes, args.no_notify).await;
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("lictor: {e}");
            ExitCode::from(3)
        }
    }
}

fn kind_label(k: &Kind) -> &'static str {
    match k {
        Kind::Pane => "pane",
        Kind::Tab => "tab",
        Kind::Workspace => "workspace",
        Kind::Agent => "agent",
    }
}

fn lane_label(k: &str) -> &'static str {
    match k {
        "w" => "workspace",
        "t" => "tab",
        "a" => "agent",
        _ => "pane",
    }
}

fn source_pane_rects(
    snapshot: &Snapshot,
    kind: &str,
    target: &str,
    socket: &std::path::Path,
    preferred_tab: Option<&str>,
) -> Option<Vec<(String, ratatui::layout::Rect)>> {
    let tab_id = match kind {
        "tab" => Some(target.to_owned()),
        "workspace" => {
            let active = snapshot
                .active_tabs
                .iter()
                .find(|(workspace, _)| workspace == target)
                .map(|(_, tab)| tab.clone());
            let own_tab = std::env::var("HERDR_TAB_ID").ok();
            let (owned_tabs, _) = runtime::owned_view_ids(socket);
            let active_is_owned = active
                .as_ref()
                .is_some_and(|id| Some(id) == own_tab.as_ref() || owned_tabs.contains(id));
            if active_is_owned {
                preferred_tab
                    .filter(|id| {
                        !owned_tabs.contains(*id)
                            && snapshot.targets.iter().any(|t| {
                                t.kind == Kind::Tab
                                    && t.id.as_str() == *id
                                    && t.workspace_id.as_deref() == Some(target)
                            })
                    })
                    .map(str::to_owned)
                    .or_else(|| {
                        snapshot
                            .targets
                            .iter()
                            .filter(|t| {
                                t.kind == Kind::Tab
                                    && t.workspace_id.as_deref() == Some(target)
                                    && Some(&t.id) != own_tab.as_ref()
                                    && !owned_tabs.contains(&t.id)
                            })
                            .max_by_key(|t| t.number)
                            .map(|t| t.id.clone())
                    })
            } else {
                active.or_else(|| {
                    snapshot
                        .targets
                        .iter()
                        .filter(|t| {
                            t.kind == Kind::Tab
                                && t.workspace_id.as_deref() == Some(target)
                                && Some(&t.id) != own_tab.as_ref()
                                && !owned_tabs.contains(&t.id)
                        })
                        .min_by_key(|t| t.number)
                        .map(|t| t.id.clone())
                })
            }
        }
        "pane" | "agent" => snapshot
            .targets
            .iter()
            .find(|t| (t.kind == Kind::Pane || t.kind == Kind::Agent) && t.id == target)
            .and_then(|t| t.tab_id.clone()),
        _ => None,
    }?;
    let layout = snapshot.layouts.iter().find(|layout| {
        layout.get("tab_id").and_then(serde_json::Value::as_str) == Some(tab_id.as_str())
    })?;
    let area = layout.get("area")?;
    let ax = area
        .get("x")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let ay = area
        .get("y")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let sw = area.get("width")?.as_u64()?.max(1) as f64;
    let sh = area.get("height")?.as_u64()?.max(1) as f64;
    let zoomed = layout
        .get("zoomed")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let focused = layout
        .get("focused_pane_id")
        .and_then(serde_json::Value::as_str);
    let panes = layout.get("panes")?.as_array()?;
    let mut result = Vec::new();
    for pane in panes {
        let Some(id) = pane.get("pane_id").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if (kind == "pane" || kind == "agent") && id != target {
            continue;
        }
        if kind != "pane" && kind != "agent" && zoomed && focused != Some(id) {
            continue;
        }
        let rect = pane.get("rect")?;
        let n = |key: &str| {
            rect.get(key)
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
        };
        result.push((
            id.to_owned(),
            ratatui::layout::Rect::new(
                (n("x").saturating_sub(ax) as f64 / sw * 10000.) as u16,
                (n("y").saturating_sub(ay) as f64 / sh * 10000.) as u16,
                (n("width") as f64 / sw * 10000.) as u16,
                (n("height") as f64 / sh * 10000.) as u16,
            ),
        ));
    }
    if (kind == "pane" || kind == "agent") && result.len() == 1 {
        result[0].1 = ratatui::layout::Rect::new(0, 0, 10000, 10000);
    }
    (!result.is_empty()).then_some(result)
}

async fn install_signals(ctx: Context) {
    #[cfg(unix)]
    {
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            use tokio::signal::unix::{SignalKind, signal};
            let Ok(mut term) = signal(SignalKind::terminate()) else {
                return;
            };
            let Ok(mut hup) = signal(SignalKind::hangup()) else {
                return;
            };
            let Ok(mut pause) = signal(SignalKind::user_defined1()) else {
                return;
            };
            let Ok(mut resume) = signal(SignalKind::user_defined2()) else {
                return;
            };
            let _ = ready_tx.send(());
            loop {
                tokio::select! {
                    _=tokio::signal::ctrl_c()=>{ctx.cancel.cancel();break},
                    _=term.recv()=>{ctx.cancel.cancel();break},
                    _=hup.recv()=>{ctx.cancel.cancel();break},
                    _=pause.recv()=>{runtime::set_paused(true)},
                    _=resume.recv()=>{runtime::set_paused(false);runtime::acknowledge_state("running")},
                }
            }
        });
        let _ = ready_rx.await;
    }
    #[cfg(not(unix))]
    {
        let _ = ctx;
    }
}

struct ViewerState {
    pane_rects: Vec<(String, ratatui::layout::Rect)>,
    children: Vec<std::process::Child>,
    mirrors: std::collections::HashMap<String, mirror::TerminalMirror>,
    rx: std::sync::mpsc::Receiver<(String, Result<Option<String>, String>)>,
    notices: std::collections::HashMap<String, String>,
    selected_tab: Option<String>,
    source_signature: SourceSignature,
    title: String,
    pane_titles: std::collections::HashMap<String, String>,
}
type SourceSignature = Vec<(String, String, ratatui::layout::Rect)>;
struct SourceView {
    signature: SourceSignature,
    selected_tab: Option<String>,
}
impl Drop for ViewerState {
    fn drop(&mut self) {
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
struct ViewerTerminalGuard;
impl Drop for ViewerTerminalGuard {
    fn drop(&mut self) {
        use crossterm::terminal::{LeaveAlternateScreen, disable_raw_mode};
        let _ = disable_raw_mode();
        let _ = crossterm::execute!(io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
    }
}

fn inspect_source(
    snapshot: &Snapshot,
    kind: &str,
    target: &str,
    socket: &std::path::Path,
    preferred: Option<&str>,
) -> Result<SourceView, String> {
    let rects = source_pane_rects(snapshot, kind, target, socket, preferred)
        .ok_or_else(|| format!("no live source layout for {kind} {target}"))?;
    let selected = rects.first().and_then(|(pane, _)| {
        snapshot
            .targets
            .iter()
            .find(|t| t.kind == Kind::Pane && t.id == *pane)
            .and_then(|t| t.tab_id.clone())
    });
    let signature = rects
        .iter()
        .map(|(pane, rect)| {
            snapshot
                .targets
                .iter()
                .find(|t| t.kind == Kind::Pane && t.id == *pane)
                .and_then(|t| t.terminal_id.clone())
                .map(|terminal| (pane.clone(), terminal, *rect))
                .ok_or_else(|| format!("missing terminal identity for source pane {pane}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SourceView {
        signature,
        selected_tab: selected,
    })
}

async fn open_viewer_state(
    ctx: &Context,
    kind: &str,
    target: &str,
    preferred_tab: Option<&str>,
) -> Result<ViewerState, String> {
    use std::{io::BufReader, sync::mpsc as std_mpsc, thread, time::Duration};
    let (tx, rx) = std_mpsc::sync_channel::<(String, Result<Option<String>, String>)>(128);
    let raw = ctx.snapshot().await.map_err(|e| e.to_string())?;
    let mut snapshot = Snapshot::from_value(&raw);
    runtime::filter_owned(&mut snapshot, &ctx.socket);
    let pane_rects = source_pane_rects(&snapshot, kind, target, &ctx.socket, preferred_tab)
        .ok_or_else(|| format!("no live source layout for {kind} {target}"))?;
    let source = inspect_source(&snapshot, kind, target, &ctx.socket, preferred_tab)?;
    let source_signature = source.signature;
    let selected_tab = source.selected_tab;
    let mut children: Vec<std::process::Child> = Vec::new();
    let mut mirrors = std::collections::HashMap::new();
    let (ready_tx, ready_rx) = std_mpsc::channel::<String>();
    for (pane, _) in &pane_rects {
        // Observe at the source size: a smaller viewport drops the source's bottom
        // rows, which hold the newest output.
        let (cols, rows) = source_pane_size(&snapshot, pane).unwrap_or((120, 40));
        let mut child = match ctx.observer(pane, cols, rows) {
            Ok(child) => child,
            Err(error) => {
                for child in &mut children {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                return Err(error.to_string());
            }
        };
        if let Some(out) = child.stdout.take() {
            let tx = tx.clone();
            let ready_tx = ready_tx.clone();
            let pane = pane.clone();
            thread::spawn(move || {
                let mut validator = mirror::TerminalMirror::new(40, 120);
                let mut ready = false;
                let mut reader = BufReader::new(out);
                loop {
                    let line = match mirror::read_line(&mut reader) {
                        Ok(Some(line)) => line,
                        Ok(None) => {
                            let _ = tx.send((pane.clone(), Ok(None)));
                            break;
                        }
                        Err(error) => {
                            let _ = tx.send((pane.clone(), Err(error.to_string())));
                            break;
                        }
                    };
                    match validator.apply_json(&line) {
                        Err(error) => {
                            let _ = tx.send((pane.clone(), Err(error.to_string())));
                            break;
                        }
                        Ok(mirror::StreamEvent::Closed(_reason)) => {
                            let _ = tx.send((pane.clone(), Ok(Some(line))));
                            break;
                        }
                        Ok(_) => {}
                    }
                    if !ready && validator.initialized() {
                        let _ = ready_tx.send(pane.clone());
                        ready = true;
                    }
                    if tx.send((pane.clone(), Ok(Some(line)))).is_err() {
                        break;
                    }
                }
            });
        }
        if let Some(err) = child.stderr.take() {
            let tx = tx.clone();
            let pane = pane.clone();
            thread::spawn(move || {
                for line in std::io::BufRead::lines(BufReader::new(err)).map_while(Result::ok) {
                    let _ = tx.send((pane.clone(), Err(format!("observer: {line}"))));
                }
            });
        }
        mirrors.insert(pane.clone(), mirror::TerminalMirror::new(40, 120));
        children.push(child);
    }
    drop(ready_tx);
    let expected: HashSet<String> = pane_rects.iter().map(|(id, _)| id.clone()).collect();
    let mut ready = HashSet::new();
    for _ in 0..expected.len() {
        match ready_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(pane) => {
                ready.insert(pane);
            }
            Err(error) => {
                for child in &mut children {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                return Err(format!(
                    "observer did not provide an initial full frame: {error}"
                ));
            }
        }
    }
    if ready != expected {
        for child in &mut children {
            let _ = child.kill();
            let _ = child.wait();
        }
        return Err("observer full-frame handshake omitted one or more source panes".into());
    }
    let title = describe_source(&snapshot, kind, target, selected_tab.as_deref());
    let pane_titles = pane_rects
        .iter()
        .map(|(pane, _)| {
            let label = snapshot
                .targets
                .iter()
                .find(|t| t.kind == Kind::Agent && t.id == *pane)
                .and_then(|t| t.agent_type.clone())
                .or_else(|| {
                    snapshot
                        .targets
                        .iter()
                        .find(|t| t.kind == Kind::Pane && t.id == *pane)
                        .map(|t| t.label.clone())
                })
                .unwrap_or_else(|| pane.clone());
            (pane.clone(), label)
        })
        .collect();
    Ok(ViewerState {
        pane_rects,
        children,
        mirrors,
        rx,
        notices: std::collections::HashMap::new(),
        selected_tab,
        source_signature,
        title,
        pane_titles,
    })
}

fn close_viewer_state(state: &mut ViewerState) {
    for child in &mut state.children {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn empty_viewer_state() -> ViewerState {
    let (_tx, rx) = std::sync::mpsc::sync_channel(1);
    ViewerState {
        pane_rects: Vec::new(),
        children: Vec::new(),
        mirrors: std::collections::HashMap::new(),
        rx,
        notices: std::collections::HashMap::new(),
        selected_tab: None,
        source_signature: Vec::new(),
        title: String::new(),
        pane_titles: std::collections::HashMap::new(),
    }
}

fn source_pane_size(snapshot: &Snapshot, pane: &str) -> Option<(u16, u16)> {
    snapshot
        .layouts
        .iter()
        .filter_map(|layout| layout.get("panes")?.as_array())
        .flatten()
        .find(|p| p.get("pane_id").and_then(serde_json::Value::as_str) == Some(pane))
        .and_then(|p| {
            let rect = p.get("rect")?;
            let n = |k: &str| rect.get(k)?.as_u64().map(|v| v.clamp(1, 1000) as u16);
            Some((n("width")?, n("height")?))
        })
}

/// Human-readable name of what a viewer shows, e.g. `alpha › tab 2` or `claude · beta`.
fn describe_source(snapshot: &Snapshot, kind: &str, target: &str, tab: Option<&str>) -> String {
    let find = |kind: Kind, id: &str| {
        snapshot
            .targets
            .iter()
            .find(|t| t.kind == kind && t.id == id)
    };
    let workspace = |id: Option<&str>| {
        id.and_then(|id| find(Kind::Workspace, id))
            .map(|w| w.label.clone())
    };
    let tab_name = |id: &str| {
        find(Kind::Tab, id).map(|t| {
            if t.number.is_some_and(|n| n.to_string() == t.label) {
                format!("tab {}", t.label)
            } else {
                t.label.clone()
            }
        })
    };
    let joined = |parts: Vec<Option<String>>| {
        let parts: Vec<String> = parts.into_iter().flatten().collect();
        if parts.is_empty() {
            target.to_owned()
        } else {
            parts.join(" › ")
        }
    };
    match kind {
        "workspace" => joined(vec![workspace(Some(target)), tab.and_then(tab_name)]),
        "tab" => {
            let ws = find(Kind::Tab, target).and_then(|t| t.workspace_id.as_deref());
            joined(vec![workspace(ws), tab_name(target)])
        }
        "agent" => {
            let agent = find(Kind::Agent, target);
            let name = agent
                .and_then(|a| a.agent_type.clone())
                .unwrap_or_else(|| "agent".into());
            let ws = agent.and_then(|a| workspace(a.workspace_id.as_deref()));
            match ws {
                Some(ws) => format!("{name} · {ws}"),
                None => name,
            }
        }
        _ => {
            let pane = find(Kind::Pane, target);
            let ws = pane.and_then(|p| workspace(p.workspace_id.as_deref()));
            joined(vec![ws, pane.map(|p| p.label.clone())])
        }
    }
}

async fn viewer(args: Vec<String>) -> ExitCode {
    let (Some(kind), Some(target)) = (args.first(), args.get(1)) else {
        eprintln!("viewer requires a target kind and ID");
        return ExitCode::from(2);
    };
    let session = args
        .windows(2)
        .find(|w| w[0] == "--session")
        .map(|w| w[1].clone());
    let socket = args
        .windows(2)
        .find(|w| w[0] == "--socket-path")
        .map(|w| std::path::PathBuf::from(&w[1]));
    let lane = args
        .windows(2)
        .find(|w| w[0] == "--lane")
        .map(|w| w[1].clone())
        .unwrap_or_default();
    let control_file = args
        .windows(2)
        .find(|w| w[0] == "--control-file")
        .map(|w| std::path::PathBuf::from(&w[1]));
    let mut preferred_tabs: std::collections::HashMap<String, String> = args
        .windows(2)
        .find(|w| w[0] == "--preferred-tabs")
        .and_then(|w| serde_json::from_str(&w[1]).ok())
        .unwrap_or_default();
    let ctx = Context::discover(socket, session);
    install_signals(ctx.clone()).await;
    use crossterm::{
        event::{self, Event, KeyCode, KeyEventKind},
        execute,
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    };
    use ratatui::{
        prelude::*,
        widgets::{Block, BorderType, Borders, Paragraph},
    };
    use std::time::Duration;
    let initial_command = control_file
        .as_ref()
        .and_then(|path| runtime::SplitControls::read(path).ok());
    let initial_target = initial_command
        .as_ref()
        .map(|command| command.target.as_str())
        .unwrap_or(target);
    let initial_enabled = initial_command
        .as_ref()
        .is_none_or(|command| command.enabled);
    let mut state = match if initial_enabled {
        open_viewer_state(
            &ctx,
            kind,
            initial_target,
            preferred_tabs.get(initial_target).map(String::as_str),
        )
        .await
    } else {
        Ok(empty_viewer_state())
    } {
        Ok(state) => state,
        Err(error) => {
            eprintln!("observer: {error}");
            return ExitCode::from(3);
        }
    };
    let mut shown_target = initial_target.to_owned();
    let mut generation = initial_command
        .as_ref()
        .map(|command| command.generation)
        .unwrap_or(0);
    let mut next_reconcile = tokio::time::Instant::now() + Duration::from_secs(1);
    if let Some(path) = &control_file
        && initial_command.is_some()
    {
        let _ = runtime::SplitControls::ack(path, generation, true);
    }
    if enable_raw_mode().is_err() {
        return ExitCode::from(1);
    }
    let mut stdout = io::stdout();
    if execute!(stdout, EnterAlternateScreen).is_err() {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
        return ExitCode::from(1);
    }
    let _terminal_guard = ViewerTerminalGuard;
    let result = async {
        let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
        loop {
            if ctx.cancel.is_cancelled() {
                break;
            }
            if let Some(path) = &control_file
                && let Ok(command) = runtime::SplitControls::read(path)
                && command.generation > generation
            {
                let success = if command.target == shown_target && !state.mirrors.is_empty() {
                    true
                } else if let Ok(next) = open_viewer_state(
                    &ctx,
                    kind,
                    &command.target,
                    preferred_tabs.get(&command.target).map(String::as_str),
                )
                .await
                {
                    if let Some(tab) = next.selected_tab.clone() {
                        preferred_tabs.insert(command.target.clone(), tab);
                    }
                    close_viewer_state(&mut state);
                    state = next;
                    shown_target = command.target.clone();
                    true
                } else {
                    false
                };
                generation = command.generation;
                let _ = runtime::SplitControls::ack(path, generation, success);
            }
            while let Ok((pane, message)) = state.rx.try_recv() {
                match message {
                    Ok(Some(line)) => {
                        if let Some(m) = state.mirrors.get_mut(&pane) {
                            match m.apply_json(&line) {
                                Ok(mirror::StreamEvent::Closed(reason)) => {
                                    state.notices.insert(
                                        pane,
                                        reason.unwrap_or_else(|| "source closed".into()),
                                    );
                                }
                                Ok(mirror::StreamEvent::Applied | mirror::StreamEvent::Ignored) => {
                                }
                                Err(error) => {
                                    state.notices.insert(pane, error.to_string());
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        state
                            .notices
                            .entry(pane)
                            .or_insert_with(|| "observer ended".into());
                    }
                    Err(error) => {
                        state.notices.insert(pane, error);
                    }
                }
            }
            if tokio::time::Instant::now() >= next_reconcile
                && (!state.pane_rects.is_empty() || generation > 1)
            {
                let preferred = preferred_tabs.get(&shown_target).map(String::as_str);
                let inspect = ctx.snapshot().await.map(|raw| {
                    let mut snapshot = Snapshot::from_value(&raw);
                    runtime::filter_owned(&mut snapshot, &ctx.socket);
                    inspect_source(&snapshot, kind, &shown_target, &ctx.socket, preferred)
                });
                let unhealthy = state.notices.values().any(|notice| {
                    notice.contains("ended")
                        || notice.contains("closed")
                        || notice.contains("error")
                        || notice.contains("observer:")
                }) || state
                    .children
                    .iter_mut()
                    .any(|child| child.try_wait().ok().flatten().is_some());
                let restart = match inspect {
                    Ok(Ok(source)) => source.signature != state.source_signature || unhealthy,
                    _ => true,
                };
                if restart {
                    if let Ok(next) = open_viewer_state(&ctx, kind, &shown_target, preferred).await
                    {
                        if let Some(tab) = next.selected_tab.clone() {
                            preferred_tabs.insert(shown_target.clone(), tab);
                        }
                        close_viewer_state(&mut state);
                        state = next;
                    } else {
                        for (pane, _) in &state.pane_rects {
                            state
                                .notices
                                .insert(pane.clone(), "source layout unavailable".into());
                        }
                    }
                }
                next_reconcile = tokio::time::Instant::now() + Duration::from_secs(1);
            }
            terminal.draw(|f| {
                let [header, area] =
                    Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(f.area());
                let accent = Color::Rgb(122, 162, 247);
                let muted = Color::Rgb(86, 95, 137);
                let surface = Color::Rgb(36, 40, 59);
                let notice = state.notices.values().next().cloned();
                let mut spans = vec![
                    Span::styled(
                        format!(
                            " {} {} ",
                            if lane == "right" { "◨" } else { "◧" },
                            kind.to_uppercase()
                        ),
                        Style::default()
                            .fg(Color::Rgb(26, 27, 38))
                            .bg(accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(
                            " {} ",
                            if state.title.is_empty() {
                                "waiting…"
                            } else {
                                state.title.as_str()
                            }
                        ),
                        Style::default()
                            .fg(Color::Rgb(192, 202, 245))
                            .bg(surface)
                            .add_modifier(Modifier::BOLD),
                    ),
                ];
                if let Some(notice) = notice {
                    spans.push(Span::styled(
                        format!("  ⚠ {notice}"),
                        Style::default().fg(Color::Rgb(224, 175, 104)),
                    ));
                }
                f.render_widget(Paragraph::new(Line::from(spans)), header);
                if state.pane_rects.is_empty() {
                    f.render_widget(
                        Paragraph::new("Waiting for an eligible target…")
                            .style(Style::default().fg(muted)),
                        area,
                    );
                }
                let single = state.pane_rects.len() == 1;
                for (pane, norm) in &state.pane_rects {
                    let x = area.x + ((u32::from(area.width) * u32::from(norm.x) / 10000) as u16);
                    let y = area.y + ((u32::from(area.height) * u32::from(norm.y) / 10000) as u16);
                    let w = ((u32::from(area.width) * u32::from(norm.width) / 10000) as u16)
                        .max(1)
                        .min(area.right().saturating_sub(x));
                    let h = ((u32::from(area.height) * u32::from(norm.height) / 10000) as u16)
                        .max(1)
                        .min(area.bottom().saturating_sub(y));
                    let rect = Rect::new(x, y, w, h);
                    let Some(m) = state.mirrors.get(pane) else {
                        continue;
                    };
                    // A single terminal fills the lane; tab layouts keep thin borders
                    // so the source pane arrangement stays readable.
                    let block = if single {
                        Block::default()
                    } else {
                        let label = state.pane_titles.get(pane).unwrap_or(pane);
                        let title = match state.notices.get(pane) {
                            Some(notice) => format!(" {label} · {notice} "),
                            None => format!(" {label} "),
                        };
                        Block::default()
                            .title(title)
                            .borders(Borders::ALL)
                            .border_type(BorderType::Rounded)
                            .border_style(Style::default().fg(muted))
                    };
                    let inner = block.inner(rect);
                    f.render_widget(block, rect);
                    f.render_widget(Paragraph::new(screen_text(m.screen(), inner.height)), inner);
                }
            })?;
            if event::poll(Duration::from_millis(40))?
                && let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
                && (key.code == KeyCode::Char('q') || key.code == KeyCode::Esc)
            {
                break;
            }
        }
        Ok::<(), io::Error>(())
    }
    .await;
    close_viewer_state(&mut state);
    if let Err(e) = result {
        eprintln!("viewer: {e}");
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// Rows of the newest content that fit in `height`. Sources are usually taller
/// than a lane, and the latest output and agent prompts live at the bottom.
fn visible_rows(screen: &vt100::Screen, height: u16) -> std::ops::Range<u16> {
    let (rows, cols) = screen.size();
    let last_content = (0..rows)
        .rev()
        .find(|&row| {
            (0..cols).any(|col| {
                screen
                    .cell(row, col)
                    .is_some_and(|c| c.has_contents() && c.contents() != " ")
            })
        })
        .unwrap_or(0);
    let bottom = last_content
        .max(screen.cursor_position().0)
        .saturating_add(1)
        .min(rows);
    bottom.saturating_sub(height)..bottom
}

fn screen_text(screen: &vt100::Screen, height: u16) -> ratatui::text::Text<'static> {
    use ratatui::{
        prelude::*,
        text::{Line, Span, Text},
    };
    let (_, cols) = screen.size();
    let mut lines = Vec::new();
    for row in visible_rows(screen, height) {
        let mut spans = Vec::new();
        for col in 0..cols {
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            let value = if cell.is_wide_continuation() {
                String::new()
            } else if cell.has_contents() {
                cell.contents().to_owned()
            } else {
                " ".to_owned()
            };
            let color = |c: vt100::Color| match c {
                vt100::Color::Default => Color::Reset,
                vt100::Color::Idx(i) => Color::Indexed(i),
                vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
            };
            let mut style = Style::default()
                .fg(color(cell.fgcolor()))
                .bg(color(cell.bgcolor()));
            if cell.bold() {
                style = style.add_modifier(Modifier::BOLD);
            }
            if cell.underline() {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            if cell.italic() {
                style = style.add_modifier(Modifier::ITALIC);
            }
            if cell.inverse() {
                style = style.add_modifier(Modifier::REVERSED);
            }
            spans.push(Span::styled(value, style));
        }
        lines.push(Line::from(spans));
    }
    Text::from(lines)
}

fn split_workspace<'a>(snapshot: &'a Snapshot, first: &'a Target) -> Result<&'a str, String> {
    let candidates = [
        snapshot.baseline_workspace.as_deref(),
        snapshot.focused_workspace.as_deref(),
        first.workspace_id.as_deref(),
        (first.kind == Kind::Workspace).then_some(first.id.as_str()),
    ];
    candidates
        .into_iter()
        .flatten()
        .find(|id| {
            snapshot
                .targets
                .iter()
                .any(|target| target.kind == Kind::Workspace && target.id == *id)
        })
        .ok_or_else(|| "no live workspace is available for the observation tab".into())
}

async fn split_mode(args: &Args, ctx: &Context, snap: &Snapshot, split: &str) -> ExitCode {
    let kinds: Vec<_> = split.split(',').collect();
    let to_kind = |s: &str| match s {
        "w" => Kind::Workspace,
        "t" => Kind::Tab,
        "a" => Kind::Agent,
        _ => Kind::Pane,
    };
    let select = |kind: &Kind, selectors: &[String]| {
        rotation::resolve(snap, std::slice::from_ref(kind), selectors)
    };
    let left = select(&to_kind(kinds[0]), &args.left_targets);
    let right = select(&to_kind(kinds[1]), &args.right_targets);
    let (Ok(mut left), Ok(mut right)) = (left, right) else {
        eprintln!("lictor: split lane selection did not resolve; specify existing lane targets");
        return ExitCode::from(3);
    };
    let lane_kinds = [to_kind(kinds[0]), to_kind(kinds[1])];
    if left.iter().any(|t| t.kind != lane_kinds[0]) || right.iter().any(|t| t.kind != lane_kinds[1])
    {
        eprintln!("lictor: a typed split selector does not match its lane kind");
        return ExitCode::from(3);
    }
    if args.dry_run {
        let statuses: HashSet<String> = args.status.iter().map(|s| s.to_lowercase()).collect();
        let eligible = |queue: &[Target], kind: &Kind| {
            queue
                .iter()
                .filter(|target| {
                    *kind != Kind::Agent
                        || statuses.is_empty()
                        || target
                            .status
                            .as_ref()
                            .is_some_and(|state| statuses.contains(&state.to_lowercase()))
                })
                .count()
        };
        // Each lane presents at most one target initially; a limit of one leaves
        // the right lane waiting, as in split_job.
        let initial = usize::from(eligible(&left, &lane_kinds[0]) > 0)
            + usize::from(eligible(&right, &lane_kinds[1]) > 0 && args.iterations != 1);
        println!(
            "Dry run: vertical split, left={} ({} targets), right={} ({} targets)",
            kinds[0],
            left.len(),
            kinds[1],
            right.len()
        );
        println!(
            "Initial eligible presentations: {initial}; successful lane advance limit: {} (0 means unlimited).",
            args.iterations
        );
        for t in left.iter().chain(&right) {
            println!("{} {}", kind_label(&t.kind), t.id)
        }
        return ExitCode::SUCCESS;
    }
    let allowed: HashSet<String> = args.status.iter().map(|s| s.to_lowercase()).collect();
    let eligible_position = |queue: &[Target], kind: &Kind| {
        queue.iter().position(|target| {
            *kind != Kind::Agent
                || allowed.is_empty()
                || target
                    .status
                    .as_ref()
                    .is_some_and(|status| allowed.contains(&status.to_lowercase()))
        })
    };
    let left_eligible = eligible_position(&left, &lane_kinds[0]);
    let right_eligible = eligible_position(&right, &lane_kinds[1]);
    if let Some(position) = left_eligible {
        left.swap(0, position);
    }
    if let Some(position) = right_eligible {
        right.swap(0, position);
    }
    let left_initial_enabled = left_eligible.is_some();
    let right_initial_enabled = right_eligible.is_some() && args.iterations != 1;
    let workspace_id = match split_workspace(snap, &left[0]) {
        Ok(id) => id,
        Err(error) => {
            eprintln!("lictor: {error}");
            return ExitCode::from(3);
        }
    };
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("lictor: {e}");
            return ExitCode::from(1);
        }
    };
    let controls = match runtime::SplitControls::create(
        &left[0].id,
        &right[0].id,
        left_initial_enabled,
        right_initial_enabled,
    ) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("lictor: {e}");
            return ExitCode::from(3);
        }
    };
    let preferred_tabs: std::collections::HashMap<String, String> =
        snap.active_tabs.iter().cloned().collect();
    let preferred_json = serde_json::to_string(&preferred_tabs).unwrap_or_else(|_| "{}".into());
    let viewer = |lane: &str, lane_kind: &str, target: &Target, control_file: &std::path::Path| {
        let mut command = vec![
            exe.to_string_lossy().into_owned(),
            "__viewer".to_owned(),
            lane_kind.to_owned(),
            target.id.clone(),
            "--lane".to_owned(),
            lane.to_owned(),
            "--control-file".to_owned(),
            control_file.to_string_lossy().into_owned(),
            "--socket-path".to_owned(),
            ctx.socket.to_string_lossy().into_owned(),
            "--preferred-tabs".to_owned(),
            preferred_json.clone(),
        ];
        if let Some(s) = &args.session {
            command.extend(["--session".to_owned(), s.clone()]);
        }
        // The Herdr label is fixed at creation, so it names the lane; the viewer
        // header shows the target currently on display.
        serde_json::json!({"type":"pane","label":format!("Lictor · {lane} · {lane_kind}s"),"command":command})
    };
    let params = serde_json::json!({"workspace_id":workspace_id,"tab_label":"Lictor mirror","focus":true,"root":{"type":"split","direction":"right","ratio":0.5,"first":viewer("left",lane_label(kinds[0]),&left[0],&controls.left),"second":viewer("right",lane_label(kinds[1]),&right[0],&controls.right)}});
    match ctx.layout_apply(params).await {
        Ok(v) => {
            let layout = v.get("result").and_then(|r| r.get("layout"));
            let tab_id = layout
                .and_then(|l| l.get("tab_id"))
                .and_then(serde_json::Value::as_str);
            let mut pane_ids = Vec::new();
            if let Some(root) = layout.and_then(|l| l.get("root")) {
                collect_pane_ids(root, &mut pane_ids);
            }
            match tab_id {
                Some(tab_id) => {
                    let terminal_ids = match created_terminal_ids(ctx, tab_id, &pane_ids).await {
                        Ok(ids) => ids,
                        Err(error) => {
                            eprintln!("lictor: cannot verify created viewer terminals: {error}");
                            return ExitCode::from(4);
                        }
                    };
                    match runtime::register_split(
                        ctx,
                        tab_id.to_owned(),
                        pane_ids.clone(),
                        terminal_ids.clone(),
                    ) {
                        Ok(mut job) => {
                            println!("Created Lictor observation tab {tab_id} (job {})", job.id);
                            // layout.apply's focus flag moves server focus without switching
                            // attached clients to the new tab; focus it explicitly.
                            if let Err(error) = ctx.focus("tab", tab_id).await {
                                eprintln!(
                                    "lictor: could not focus observation tab {tab_id}: {error}"
                                );
                            }
                            split_job(
                                args,
                                ctx,
                                left.clone(),
                                right.clone(),
                                controls,
                                tab_id,
                                &pane_ids,
                                &terminal_ids,
                                &mut job,
                            )
                            .await
                        }
                        Err(error) => {
                            let _ = close_owned_tab(ctx, tab_id, &pane_ids, &terminal_ids).await;
                            eprintln!("lictor: could not register split job: {error}");
                            ExitCode::from(3)
                        }
                    }
                }
                None => {
                    eprintln!("lictor: Herdr created a view without returning its tab ID");
                    ExitCode::from(4)
                }
            }
        }
        Err(e) => {
            controls.remove();
            eprintln!("lictor: could not create observation tab: {e}");
            ExitCode::from(4)
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn split_job(
    args: &Args,
    ctx: &Context,
    mut left: Vec<Target>,
    mut right: Vec<Target>,
    controls: runtime::SplitControls,
    tab_id: &str,
    pane_ids: &[String],
    terminal_ids: &[String],
    job: &mut runtime::JobGuard,
) -> ExitCode {
    let mut sidebar = if let Some(action) = &args.sidebar {
        match platform::SidebarGuard::apply(
            ctx,
            action,
            args.sidebar_keep,
            args.verbose && !args.quiet,
        )
        .await
        {
            Ok(g) => Some(g),
            Err(e) => {
                eprintln!("lictor: sidebar: {e}; continuing split rotation");
                None
            }
        }
    } else {
        None
    };
    let left_initially_enabled =
        runtime::SplitControls::read(&controls.left).is_ok_and(|c| c.enabled);
    let right_initially_enabled =
        runtime::SplitControls::read(&controls.right).is_ok_and(|c| c.enabled);
    // Disabled placeholders must also read and acknowledge their initial command
    // before the manager can remove the control files at a short finite limit.
    let initial_ok = wait_lane_ack(&controls.left, 1, &ctx.cancel).await
        && wait_lane_ack(&controls.right, 1, &ctx.cancel).await;
    if !initial_ok {
        eprintln!(
            "lictor: split viewers did not acknowledge startup; closing the observation tab (check Herdr observation support and viewer output)"
        );
        if let Err(e) = close_owned_tab(ctx, tab_id, pane_ids, terminal_ids).await {
            eprintln!("lictor: split startup failed and rollback could not close {tab_id}: {e}");
        }
        if let Some(guard) = &mut sidebar {
            for warning in guard.restore().await {
                eprintln!("lictor: {warning}");
            }
        }
        return ExitCode::from(4);
    }
    let mut advances = usize::from(left_initially_enabled) + usize::from(right_initially_enabled);
    let mut changes = advances;
    let mut li = if left_initially_enabled {
        0
    } else {
        left.len().saturating_sub(1)
    };
    let mut ri = if right_initially_enabled {
        0
    } else {
        right.len().saturating_sub(1)
    };
    let mut lg = 1u64;
    let mut rg = 1u64;
    let interval = args.interval().unwrap();
    let allowed: HashSet<String> = args.status.iter().map(|s| s.to_lowercase()).collect();
    let mut all_gone = false;
    let mut manually_closed = false;
    let mut rng = rand::rng();
    let mut left_shuffle_pending = false;
    let mut right_shuffle_pending = false;
    loop {
        if args.iterations > 0 && advances >= args.iterations {
            break;
        }
        if ctx.cancel.is_cancelled() {
            break;
        }
        runtime::wait_if_paused(&ctx.cancel).await;
        if ctx.cancel.is_cancelled() {
            break;
        }
        if !runtime::sleep_or_pause(interval, &ctx.cancel).await {
            break;
        }
        runtime::wait_if_paused(&ctx.cancel).await;
        if ctx.cancel.is_cancelled() {
            break;
        }
        let raw = match ctx.snapshot().await {
            Ok(v) => v,
            Err(crate::herdr::Error::Cancelled) => break,
            Err(e) => {
                eprintln!("lictor: split snapshot: {e}");
                continue;
            }
        };
        let mut live = Snapshot::from_value(&raw);
        runtime::filter_owned(&mut live, &ctx.socket);
        let tab_live = raw
            .get("result")
            .and_then(|r| r.get("snapshot"))
            .and_then(|s| s.get("layouts"))
            .and_then(serde_json::Value::as_array)
            .is_some_and(|ls| {
                ls.iter()
                    .any(|l| l.get("tab_id").and_then(serde_json::Value::as_str) == Some(tab_id))
            });
        if !tab_live {
            manually_closed = true;
            break;
        }
        left.retain(|t| {
            live.targets
                .iter()
                .any(|x| x.kind == t.kind && x.id == t.id)
        });
        right.retain(|t| {
            live.targets
                .iter()
                .any(|x| x.kind == t.kind && x.id == t.id)
        });
        if left.is_empty() && right.is_empty() {
            all_gone = true;
            break;
        }
        for (lane, queue, index, generation, shuffle_pending, path) in [
            (
                0,
                &mut left,
                &mut li,
                &mut lg,
                &mut left_shuffle_pending,
                &controls.left,
            ),
            (
                1,
                &mut right,
                &mut ri,
                &mut rg,
                &mut right_shuffle_pending,
                &controls.right,
            ),
        ] {
            runtime::wait_if_paused(&ctx.cancel).await;
            if ctx.cancel.is_cancelled() {
                break;
            }
            if args.iterations > 0 && advances >= args.iterations {
                break;
            }
            if queue.is_empty() {
                continue;
            }
            if args.random && *shuffle_pending {
                use rand::seq::SliceRandom;
                let current = runtime::SplitControls::read(path)
                    .ok()
                    .map(|value| value.target);
                queue.shuffle(&mut rng);
                if let Some(current) = current {
                    *index = queue
                        .iter()
                        .position(|target| target.id == current)
                        .unwrap_or(0);
                }
                *shuffle_pending = false;
            }
            let candidate = (1..=queue.len()).find_map(|offset| {
                let slot = (*index + offset) % queue.len();
                let candidate = &queue[slot];
                let candidate_live = live
                    .targets
                    .iter()
                    .find(|x| x.kind == candidate.kind && x.id == candidate.id);
                let eligible = candidate_live.is_some_and(|x| {
                    x.kind != Kind::Agent
                        || allowed.is_empty()
                        || x.status
                            .as_ref()
                            .is_some_and(|s| allowed.contains(&s.to_lowercase()))
                });
                eligible.then_some((slot, candidate.clone()))
            });
            let Some((next, candidate)) = candidate else {
                continue;
            };
            *generation += 1;
            if let Err(e) = runtime::SplitControls::update(path, *generation, &candidate.id) {
                eprintln!("lictor: could not update split lane: {e}");
                continue;
            }
            if wait_lane_ack(path, *generation, &ctx.cancel).await {
                *index = next;
                if args.random && next + 1 == queue.len() {
                    *shuffle_pending = true;
                }
                advances += 1;
                changes += 1;
                if args.verbose && !args.quiet {
                    eprintln!(
                        "lictor: advanced {} lane to {}",
                        if lane == 0 { "left" } else { "right" },
                        candidate.id
                    );
                }
            }
            if ctx.cancel.is_cancelled() {
                let _ = wait_lane_ack(
                    path,
                    *generation,
                    &tokio_util::sync::CancellationToken::new(),
                )
                .await;
            }
            if ctx.cancel.is_cancelled() {
                break;
            }
        }
    }
    if let Some(guard) = &mut sidebar {
        for warning in guard.restore().await {
            eprintln!("lictor: {warning}");
        }
    }
    let tab_still_live = !manually_closed;
    let mut close_failed = false;
    if tab_still_live
        && args.close_on_exit
        && let Err(e) = close_owned_tab(ctx, tab_id, pane_ids, terminal_ids).await
    {
        eprintln!("lictor: could not close owned view {tab_id}: {e}");
        close_failed = true;
    }
    if tab_still_live
        && (!args.close_on_exit || close_failed)
        && let Err(e) = job.retain_view()
    {
        eprintln!("lictor: could not retain view ownership: {e}");
    }
    controls.remove();
    if changes > 0 {
        platform::notify_stop(changes, args.no_notify).await;
    }
    if all_gone {
        eprintln!("lictor: all split targets disappeared");
        ExitCode::from(3)
    } else {
        ExitCode::SUCCESS
    }
}

async fn wait_lane_ack(
    path: &std::path::PathBuf,
    generation: u64,
    cancel: &tokio_util::sync::CancellationToken,
) -> bool {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Ok(command) = runtime::SplitControls::read(path) {
            if command.ack_generation == generation {
                return command.acknowledged;
            }
            if command.generation > generation {
                return false;
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::select! { _=cancel.cancelled()=>return false, _=tokio::time::sleep(std::time::Duration::from_millis(25))=>{} }
    }
}

async fn close_owned_tab(
    ctx: &Context,
    tab_id: &str,
    pane_ids: &[String],
    terminal_ids: &[String],
) -> Result<(), String> {
    let raw = ctx.snapshot().await.map_err(|e| e.to_string())?;
    let snapshot = Snapshot::from_value(&raw);
    let layout = snapshot
        .layouts
        .iter()
        .find(|l| l.get("tab_id").and_then(serde_json::Value::as_str) == Some(tab_id))
        .ok_or_else(|| format!("tab {tab_id} no longer exists"))?;
    let mut live: Vec<(String, String)> = layout
        .get("panes")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|pane| pane.get("pane_id")?.as_str().map(str::to_owned))
        .filter_map(|id| {
            snapshot
                .targets
                .iter()
                .find(|t| t.kind == Kind::Pane && t.id == id)
                .and_then(|t| t.terminal_id.clone())
                .map(|terminal| (id, terminal))
        })
        .collect();
    live.sort();
    if pane_ids.len() != terminal_ids.len() {
        return Err("owned terminal identity record is incomplete".into());
    }
    let mut expected = pane_ids
        .iter()
        .cloned()
        .zip(terminal_ids.iter().cloned())
        .collect::<Vec<_>>();
    expected.sort();
    if live != expected {
        return Err(format!(
            "refusing to close {tab_id}: pane ownership changed"
        ));
    }
    ctx.request("tab.close", serde_json::json!({"tab_id":tab_id}))
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn created_terminal_ids(
    ctx: &Context,
    tab_id: &str,
    pane_ids: &[String],
) -> Result<Vec<String>, String> {
    let raw = ctx.snapshot().await.map_err(|e| e.to_string())?;
    let snapshot = Snapshot::from_value(&raw);
    let mut ids = Vec::with_capacity(pane_ids.len());
    for pane_id in pane_ids {
        ids.push(
            snapshot
                .targets
                .iter()
                .find(|t| {
                    t.kind == Kind::Pane && t.id == *pane_id && t.tab_id.as_deref() == Some(tab_id)
                })
                .and_then(|t| t.terminal_id.clone())
                .ok_or_else(|| format!("missing terminal identity for pane {pane_id}"))?,
        );
    }
    Ok(ids)
}

fn collect_pane_ids(node: &serde_json::Value, output: &mut Vec<String>) {
    if let Some(id) = node.get("pane_id").and_then(serde_json::Value::as_str) {
        output.push(id.to_owned());
    }
    for key in ["first", "second", "child"] {
        if let Some(child) = node.get(key) {
            collect_pane_ids(child, output);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewer_keeps_the_newest_rows_when_the_source_is_taller() {
        let mut parser = vt100::Parser::new(10, 20, 0);
        for n in 0..8 {
            parser.process(format!("line {n}\r\n").as_bytes());
        }
        // Content ends at row 7 and the cursor sits on row 8.
        assert_eq!(visible_rows(parser.screen(), 4), 5..9);
        assert_eq!(visible_rows(parser.screen(), 20), 0..9);
    }

    #[test]
    fn split_tab_uses_current_workspace_instead_of_first_rotation_target() {
        let snapshot = Snapshot::from_value(&serde_json::json!({
            "focused_workspace_id": "current-space",
            "workspaces": [
                {"workspace_id": "first-source"},
                {"workspace_id": "current-space"}
            ]
        }));
        let first = snapshot.by_kind(&Kind::Workspace)[0];
        assert_eq!(split_workspace(&snapshot, first).unwrap(), "current-space");

        let mut context = snapshot.clone();
        context.baseline_workspace = Some("first-source".into());
        assert_eq!(split_workspace(&context, first).unwrap(), "first-source");

        context.baseline_workspace = Some("closed-space".into());
        assert_eq!(split_workspace(&context, first).unwrap(), "current-space");

        context.focused_workspace = None;
        assert_eq!(split_workspace(&context, first).unwrap(), "first-source");
        context.targets.clear();
        assert!(split_workspace(&context, first).is_err());
    }
}
