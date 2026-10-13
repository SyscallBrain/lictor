use crate::{domain::Kind, rotation::parse_duration};
use clap::{
    ArgAction, Parser,
    builder::styling::{AnsiColor, Color, RgbColor, Style, Styles},
};
use std::{path::PathBuf, time::Duration};

// Tokyo Night, matching the TUI.
const BLUE: RgbColor = RgbColor(122, 162, 247);
const PURPLE: RgbColor = RgbColor(187, 154, 247);
const GREEN: RgbColor = RgbColor(158, 206, 106);
const MUTED: RgbColor = RgbColor(86, 95, 137);

fn styles() -> Styles {
    let fg = |c: RgbColor| Style::new().fg_color(Some(Color::Rgb(c)));
    Styles::styled()
        .header(fg(BLUE).bold())
        .usage(fg(BLUE).bold())
        .literal(fg(GREEN).bold())
        .placeholder(fg(PURPLE))
        .valid(fg(GREEN))
        .invalid(Style::new().fg_color(Some(AnsiColor::Red.into())).bold())
        .error(Style::new().fg_color(Some(AnsiColor::Red.into())).bold())
}

/// Help text sections. clap strips the styling when output is not a terminal.
fn help_section(examples: &[(&str, &str)], long: bool) -> String {
    let header = fg_bold(BLUE);
    let literal = fg_bold(GREEN);
    let muted = Style::new().fg_color(Some(Color::Rgb(MUTED)));
    let mut out = format!("{header}Examples:{header:#}\n");
    let width = examples.iter().map(|(c, _)| c.len()).max().unwrap_or(0);
    for (command, comment) in examples {
        // The long list has wider commands, so comments go above them.
        out.push_str(&if long {
            format!("  {muted}# {comment}{muted:#}\n  {literal}{command}{literal:#}\n\n")
        } else {
            format!("  {literal}{command:<width$}{literal:#}  {muted}{comment}{muted:#}\n")
        });
    }
    if long {
        out.push_str(&format!(
            "{header}Selectors:{header:#}\n  Numbers and names are resolved in the selected groups: workspace {literal}2{literal:#} or {literal}api{literal:#},\n  tab {literal}1:2{literal:#}, pane {literal}2.1{literal:#}, agent {literal}claude{literal:#} (every agent of that type).\n  Prefix a kind to be explicit: {literal}workspace={literal:#}, {literal}tab={literal:#}, {literal}agent={literal:#}, {literal}pane={literal:#}.\n"
        ));
        out.push_str(&format!(
            "\n{header}Exit codes:{header:#}\n  {literal}0{literal:#} success   {literal}1{literal:#} runtime failure   {literal}2{literal:#} invalid arguments\n  {literal}3{literal:#} no targets, or no job to control   {literal}4{literal:#} Herdr unavailable\n"
        ));
    } else {
        out.push_str(&format!(
            "\nUse {literal}--help{literal:#} for details, selectors and exit codes.\n"
        ));
    }
    out
}
fn fg_bold(c: RgbColor) -> Style {
    Style::new().fg_color(Some(Color::Rgb(c))).bold()
}
const EXAMPLES: &[(&str, &str)] = &[
    ("lictor", "Pick targets in the keyboard controller"),
    (
        "lictor -W -A -s 5",
        "Focus each workspace, then each agent, every 5 seconds",
    ),
    (
        "lictor -W -A -s 5 -S w,a",
        "New tab: workspaces mirrored left, agents right",
    ),
    (
        "lictor --tui -W -A -s 5 -S w,a",
        "Same, but review it in the controller first",
    ),
    (
        "lictor -A --status blocked,done",
        "Visit only agents waiting for you",
    ),
    ("lictor --stop", "Stop running rotations"),
];
const MORE_EXAMPLES: &[(&str, &str)] = &[
    ("lictor", "Pick targets in the keyboard controller"),
    (
        "lictor -W -A -s 5",
        "Focus each workspace, then each agent, every 5 seconds",
    ),
    (
        "lictor -W -A -s 5 -S w,a",
        "New tab: workspaces mirrored left, agents right",
    ),
    (
        "lictor -S w,a --left-targets 1,3 --right-targets claude",
        "Choose what each split lane shows",
    ),
    (
        "lictor --tui -W -A -s 5 -S w,a",
        "Same, but review it in the controller first",
    ),
    (
        "lictor -A --status blocked,done",
        "Visit only agents waiting for you",
    ),
    (
        "lictor -T -p 1,2,4 -s 10",
        "Rotate three tabs of the current workspace",
    ),
    (
        "lictor -W -A -d -n 8",
        "Preview eight steps without changing anything",
    ),
    ("lictor -A -l -a", "List every agent in the session"),
    (
        "lictor --jobs",
        "Show running jobs and retained mirror tabs",
    ),
    ("lictor --stop", "Stop running rotations"),
];

fn interval(value: &str) -> Result<String, String> {
    parse_duration(value).map(|_| value.to_owned())
}

#[derive(Clone, Debug, Parser)]
#[command(
    name = "lictor",
    version,
    author,
    about = "Rotate Herdr focus, or mirror live terminals side by side in a split tab.",
    long_about = "Rotate Herdr focus between workspaces, tabs, panes and agents, or mirror them \
live in a new split tab without moving the originals.\n\nWithout arguments, Lictor opens the \
keyboard controller. With arguments it runs directly; add --tui to review them in the controller \
first.",
    styles = styles(),
    after_help = help_section(EXAMPLES, false),
    after_long_help = help_section(MORE_EXAMPLES, true),
    max_term_width = 100,
    next_line_help = false,
    disable_help_flag = true,
    disable_version_flag = true
)]
pub struct Args {
    // Targets ---------------------------------------------------------------
    /// Rotate workspaces
    #[arg(short = 'W', long = "workspaces", help_heading = "Targets")]
    pub workspaces: bool,
    /// Rotate agents (Claude, Codex, …)
    #[arg(short = 'A', long = "agents", help_heading = "Targets")]
    pub agents: bool,
    /// Rotate tabs of the current workspace
    #[arg(short = 'T', long = "tabs", help_heading = "Targets")]
    pub tabs: bool,
    /// Only these targets, in this order
    #[arg(
        short = 'p',
        long = "targets",
        alias = "panes",
        value_name = "LIST",
        value_delimiter = ',',
        help_heading = "Targets",
        long_help = "Only these targets, in this order; repeat one to stay longer on it. Without \
group flags, entries are panes of the current tab. See Selectors below. Alias: --panes."
    )]
    pub targets: Vec<String>,
    /// Only visit agents in these states: idle, working, blocked, done, unknown
    #[arg(
        long = "status",
        value_name = "STATES",
        value_delimiter = ',',
        value_parser = ["idle", "working", "blocked", "done", "unknown"],
        hide_possible_values = true,
        help_heading = "Targets",
        long_help = "Only visit agents in these states, checked live at every step. Implies -A; \
other groups are unaffected. While no agent matches, the rotation waits. States: idle, \
working, blocked, done, unknown."
    )]
    pub status: Vec<String>,
    /// With -W -A, show each workspace's active tab instead of its first
    #[arg(
        short = 'c',
        long = "active-tab",
        alias = "current-tab",
        help_heading = "Targets"
    )]
    pub current_tab: bool,
    /// Resolve relative selectors from this workspace or tab
    #[arg(
        short = 't',
        long = "target",
        value_name = "ID",
        help_heading = "Targets",
        long_help = "Resolve relative selectors (pane numbers, tab numbers) from this workspace, \
tab or pane instead of the one Lictor runs in."
    )]
    pub target: Option<String>,
    /// Legacy alias of -T
    #[arg(short = 'w', long = "windows", alias = "tab-group", hide = true)]
    pub tab_group: bool,

    // Timing ----------------------------------------------------------------
    /// Seconds on each target; decimals allowed
    #[arg(
        short = 's',
        long = "seconds",
        alias = "interval",
        value_name = "SECONDS",
        default_value = "3",
        value_parser = interval,
        help_heading = "Timing"
    )]
    pub interval: String,
    /// Stop after N moves [default: run until stopped]
    #[arg(
        short = 'n',
        long = "iterations",
        value_name = "N",
        default_value = "0",
        hide_default_value = true,
        help_heading = "Timing",
        long_help = "Stop after N successful moves; 0 runs until stopped. In split mode every lane \
advance counts, so one tick of two lanes counts twice."
    )]
    pub iterations: usize,
    /// Shuffle the order after every lap
    #[arg(short = 'r', long = "random", help_heading = "Timing")]
    pub random: bool,

    // Split view -------------------------------------------------------------
    /// Open a new tab mirroring two lanes: w, t, a or p for each side
    #[arg(
        short = 'S',
        long = "split",
        value_name = "LEFT,RIGHT",
        help_heading = "Split view",
        long_help = "Open a new Herdr tab with two live, read-only mirrors side by side. Give one \
kind per lane: w workspaces, t tabs, a agents, p panes (e.g. w,a). Each lane moves through its \
own targets at every interval; the original terminals keep running where they are."
    )]
    pub split: Option<String>,
    /// Targets for the left lane [default: all of its kind]
    #[arg(
        long = "left-targets",
        value_name = "LIST",
        value_delimiter = ',',
        help_heading = "Split view"
    )]
    pub left_targets: Vec<String>,
    /// Targets for the right lane [default: all of its kind]
    #[arg(
        long = "right-targets",
        value_name = "LIST",
        value_delimiter = ',',
        help_heading = "Split view"
    )]
    pub right_targets: Vec<String>,
    /// Close the mirror tab when the rotation stops
    #[arg(
        long = "close-on-exit",
        help_heading = "Split view",
        long_help = "Close the mirror tab when the rotation stops. By default it stays open, still \
live, on the last targets; close it later with --close-view."
    )]
    pub close_on_exit: bool,

    // Preview ---------------------------------------------------------------
    /// Open the keyboard controller with these settings
    #[arg(long = "tui", help_heading = "Preview")]
    pub tui: bool,
    /// List matching targets and exit
    #[arg(short = 'l', long = "list", help_heading = "Preview")]
    pub list: bool,
    /// With --list, include the whole session
    #[arg(short = 'a', long = "all", help_heading = "Preview")]
    pub all: bool,
    /// Show the plan without changing Herdr
    #[arg(
        short = 'd',
        long = "dry-run",
        help_heading = "Preview",
        long_help = "Show the targets and the first steps (5, or -n) without focusing anything or \
creating tabs. Agent states can change later, so the preview uses the current ones."
    )]
    pub dry_run: bool,

    // Jobs --------------------------------------------------------------------
    /// List running jobs and retained mirror tabs
    #[arg(long = "jobs", help_heading = "Jobs")]
    pub jobs: bool,
    /// Stop running rotations (all, or --job ID)
    #[arg(short = 'k', long = "stop", help_heading = "Jobs")]
    pub stop: bool,
    /// Pause rotations (all, or --job ID)
    #[arg(long = "pause", help_heading = "Jobs")]
    pub pause: bool,
    /// Resume paused rotations (all, or --job ID)
    #[arg(long = "resume", help_heading = "Jobs")]
    pub resume: bool,
    /// Close retained mirror tabs (all, or --job ID)
    #[arg(long = "close-view", help_heading = "Jobs")]
    pub close_view: bool,
    /// Act on one job; alone, print its state
    #[arg(
        long = "job",
        value_name = "ID",
        help_heading = "Jobs",
        long_help = "Act on one job with --stop, --pause, --resume or --close-view. Alone, print \
its state; with --tui, open the controller on it (pausing it while you look)."
    )]
    pub job: Option<String>,

    // Herdr -------------------------------------------------------------------
    /// Use a named Herdr session
    #[arg(
        short = 'L',
        long = "session",
        value_name = "NAME",
        conflicts_with = "socket_path",
        help_heading = "Herdr"
    )]
    pub session: Option<String>,
    /// Use this Herdr API socket
    #[arg(long = "socket-path", value_name = "PATH", help_heading = "Herdr")]
    pub socket_path: Option<PathBuf>,
    /// Show, hide or toggle the sidebar while rotating
    #[arg(
        short = 'b',
        long = "sidebar",
        value_name = "ACTION",
        value_parser = ["show", "hide", "toggle"],
        hide_possible_values = true,
        help_heading = "Herdr",
        long_help = "Show, hide or toggle the Herdr sidebar while rotating; it is restored when the \
rotation stops. Needs tmux or X11 with xdotool."
    )]
    pub sidebar: Option<String>,
    /// Keep the sidebar as set after stopping
    #[arg(long = "sidebar-keep", help_heading = "Herdr")]
    pub sidebar_keep: bool,
    /// No desktop notification when the rotation stops
    #[arg(long = "no-notify", help_heading = "Herdr")]
    pub no_notify: bool,

    // Output ----------------------------------------------------------------
    /// Print every move with a timestamp
    #[arg(short = 'v', long = "verbose", help_heading = "Output")]
    pub verbose: bool,
    /// Print less
    #[arg(short = 'q', long = "quiet", help_heading = "Output")]
    pub quiet: bool,

    /// Print a man page or shell completion script (used for packaging)
    #[arg(long = "generate", value_name = "WHAT", hide = true, exclusive = true,
          value_parser = ["man", "bash", "zsh", "fish"])]
    pub generate: Option<String>,

    // General -----------------------------------------------------------------
    /// Show help (--help for details)
    #[arg(short = 'h', long = "help", action = ArgAction::Help, help_heading = "General")]
    help: Option<bool>,
    /// Show the version
    #[arg(short = 'V', long = "version", action = ArgAction::Version, help_heading = "General")]
    version: Option<bool>,
}

impl Args {
    pub fn interval(&self) -> Result<Duration, String> {
        parse_duration(&self.interval)
    }
    pub fn kinds(&self) -> Vec<Kind> {
        let argv: Vec<String> = std::env::args().collect();
        let mut ordered = Vec::<(usize, Kind)>::new();
        let mut push = |i: usize, kind: Kind| {
            if !ordered.iter().any(|(_, existing)| *existing == kind) {
                ordered.push((i, kind));
            }
        };
        for (i, arg) in argv.iter().enumerate() {
            let kind = match arg.as_str() {
                "-T" | "--tabs" | "-w" | "--windows" | "--tab-group" => Some(Kind::Tab),
                "-W" | "--workspaces" => Some(Kind::Workspace),
                "-A" | "--agents" | "--status" => Some(Kind::Agent),
                _ if arg.starts_with("--status=") => Some(Kind::Agent),
                _ => None,
            };
            if let Some(kind) = kind {
                push(i, kind);
            } else if arg.starts_with('-') && !arg.starts_with("--") && arg.len() > 2 {
                for flag in arg.chars().skip(1) {
                    let kind = match flag {
                        'T' => Some(Kind::Tab),
                        'W' => Some(Kind::Workspace),
                        'A' => Some(Kind::Agent),
                        'w' => Some(Kind::Tab),
                        _ => None,
                    };
                    if let Some(kind) = kind {
                        push(i, kind);
                    }
                    if matches!(flag, 'p' | 's' | 'n' | 'S' | 't' | 'L' | 'b') {
                        break;
                    }
                }
            }
        }
        let has_group = self.tabs
            || self.tab_group
            || self.workspaces
            || self.agents
            || !self.status.is_empty();
        if !has_group {
            return vec![Kind::Pane];
        }
        if self.tabs || self.tab_group {
            push(usize::MAX - 3, Kind::Tab);
        }
        if self.workspaces {
            push(usize::MAX - 2, Kind::Workspace);
        }
        if self.agents || !self.status.is_empty() {
            push(usize::MAX - 1, Kind::Agent);
        }
        if !self.targets.is_empty() && !ordered.iter().any(|(_, kind)| *kind == Kind::Pane) {
            // The legacy -p option is resolved against the selected legacy groups.
        }
        ordered.sort_by_key(|(i, _)| *i);
        ordered.into_iter().map(|(_, kind)| kind).collect()
    }
    pub fn validate(&self) -> Result<(), String> {
        self.interval()?;
        if [
            self.jobs,
            self.stop,
            self.pause,
            self.resume,
            self.close_view,
        ]
        .into_iter()
        .filter(|enabled| *enabled)
        .count()
            > 1
        {
            return Err(
                "use only one of --jobs, --stop, --pause, --resume and --close-view".into(),
            );
        }
        if let Some(split) = &self.split {
            let v: Vec<_> = split.split(',').collect();
            if v.len() != 2 || v.iter().any(|x| !["w", "t", "a", "p"].contains(x)) {
                return Err(format!(
                    "invalid --split '{split}': give one kind per lane, such as w,a \
                     (w workspaces, t tabs, a agents, p panes)"
                ));
            }
            if !self.targets.is_empty() {
                return Err(
                    "-p cannot choose split lanes; use --left-targets and --right-targets".into(),
                );
            }
            if self.current_tab {
                return Err(
                    "-c has no effect with --split; workspace lanes already show the active tab"
                        .into(),
                );
            }
            let shown: Vec<_> = v
                .iter()
                .map(|x| match *x {
                    "w" => Kind::Workspace,
                    "t" => Kind::Tab,
                    "a" => Kind::Agent,
                    _ => Kind::Pane,
                })
                .collect();
            let conflict = |flag: &str, kind: &str, letter: &str| {
                format!(
                    "{flag} asks for {kind}, but neither lane of --split {split} shows them; \
                     drop {flag} or use '{letter}' in --split"
                )
            };
            if (self.tabs || self.tab_group) && !shown.contains(&Kind::Tab) {
                return Err(conflict("-T", "tabs", "t"));
            }
            if self.workspaces && !shown.contains(&Kind::Workspace) {
                return Err(conflict("-W", "workspaces", "w"));
            }
            if self.agents && !shown.contains(&Kind::Agent) {
                return Err(conflict("-A", "agents", "a"));
            }
            if !self.status.is_empty() && !shown.contains(&Kind::Agent) {
                return Err(conflict("--status", "agents", "a"));
            }
        } else if !self.left_targets.is_empty() || !self.right_targets.is_empty() {
            return Err("--left-targets and --right-targets only apply with --split".into());
        }
        if self.current_tab && !self.workspaces {
            return Err("-c only applies to workspaces; add -W".into());
        }
        if (self.tabs || self.tab_group)
            && (self.workspaces || self.agents || !self.status.is_empty())
            && self.targets.iter().any(|target| !target.contains('='))
        {
            return Err(
                "when -T is mixed with -W or -A, prefix each -p entry with its kind, \
                        such as tab=1, workspace=api or agent=claude"
                    .into(),
            );
        }
        if self.close_on_exit && self.split.is_none() {
            return Err("--close-on-exit only applies with --split".into());
        }
        Ok(())
    }
}

/// Writes the man page or a completion script for packaging. Help sections are
/// rendered without terminal colors, since roff and shells cannot use them.
pub fn generate(what: &str, out: &mut dyn std::io::Write) -> std::io::Result<()> {
    use clap::CommandFactory;
    let mut command = Args::command()
        .after_help(strip_ansi(&help_section(EXAMPLES, false)))
        .after_long_help(strip_ansi(&help_section(MORE_EXAMPLES, true)));
    let shell = match what {
        "man" => return man_page(command, out),
        "bash" => clap_complete::Shell::Bash,
        "zsh" => clap_complete::Shell::Zsh,
        _ => clap_complete::Shell::Fish,
    };
    clap_complete::generate(shell, &mut command, "lictor", out);
    Ok(())
}

/// The man page uses conventional sections instead of clap_mangen's generic
/// "EXTRA" section for the long help epilogue.
fn man_page(command: clap::Command, out: &mut dyn std::io::Write) -> std::io::Result<()> {
    let man = clap_mangen::Man::new(command);
    man.render_title(out)?;
    man.render_name_section(out)?;
    man.render_synopsis_section(out)?;
    man.render_description_section(out)?;
    man.render_options_section(out)?;
    let escape = |text: &str| text.replace('\\', "\\e").replace('-', "\\-");
    writeln!(out, ".SH EXAMPLES")?;
    for (command, comment) in MORE_EXAMPLES {
        writeln!(out, ".TP\n\\fB{}\\fR\n{}", escape(command), escape(comment))?;
    }
    writeln!(
        out,
        ".SH SELECTORS\nNumbers and names are resolved in the selected groups: workspace \\fB2\\fR or \
         \\fBapi\\fR, tab \\fB1:2\\fR, pane \\fB2.1\\fR, agent \\fBclaude\\fR (every agent of that \
         type). Prefix a kind to be explicit: \\fBworkspace=\\fR, \\fBtab=\\fR, \\fBagent=\\fR, \
         \\fBpane=\\fR."
    )?;
    writeln!(out, ".SH EXIT STATUS")?;
    for (code, meaning) in [
        ("0", "Success or clean stop."),
        ("1", "Runtime failure."),
        ("2", "Invalid arguments."),
        (
            "3",
            "No targets, every target disappeared, or no job to control.",
        ),
        ("4", "Herdr is unavailable."),
    ] {
        writeln!(out, ".TP\n\\fB{code}\\fR\n{meaning}")?;
    }
    writeln!(
        out,
        ".SH SEE ALSO\n\\fBherdr\\fR(1). The Lictor book: https://syscallbrain.github.io/lictor/"
    )?;
    man.render_version_section(out)?;
    man.render_authors_section(out)
}

fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip a CSI sequence: ESC [ parameters final-byte.
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_man_page_and_completions_are_plain_text() {
        for what in ["man", "bash", "zsh", "fish"] {
            let mut out = Vec::new();
            generate(what, &mut out).unwrap();
            let text = String::from_utf8(out).unwrap();
            assert!(text.contains("split"), "{what}");
            assert!(!text.contains('\x1b'), "{what} contains ANSI escapes");
        }
    }
}
