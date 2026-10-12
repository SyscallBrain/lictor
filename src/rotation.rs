use crate::{
    domain::{Kind, Snapshot, Target},
    herdr::{Context, Error},
};
use rand::seq::SliceRandom;
use std::{collections::HashSet, time::Duration};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ResolveError {
    #[error("no targets matched the selection")]
    Empty,
    #[error("unknown target selector: {0}")]
    Unknown(String),
}

pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let n: f64 = s
        .replace(',', ".")
        .parse()
        .map_err(|_| "interval must be a positive number of seconds".to_owned())?;
    if !n.is_finite() || n <= 0.0 {
        return Err("interval must be finite and greater than zero".into());
    }
    let duration = Duration::try_from_secs_f64(n)
        .map_err(|_| "interval is too large to represent".to_owned())?;
    if duration.is_zero() {
        return Err("interval is too small to represent a positive delay".into());
    }
    Ok(duration)
}

pub fn resolve(
    snapshot: &Snapshot,
    kinds: &[Kind],
    selectors: &[String],
) -> Result<Vec<Target>, ResolveError> {
    let mut out = Vec::new();
    if selectors.is_empty() {
        for kind in kinds {
            out.extend(snapshot.by_kind(kind).into_iter().cloned());
        }
    } else {
        for selector in selectors {
            let (typed, value) = selector.split_once('=').unwrap_or(("", selector));
            if !typed.is_empty() && !["workspace", "tab", "pane", "agent"].contains(&typed) {
                return Err(ResolveError::Unknown(selector.clone()));
            }
            let explicit_kind = match typed {
                "workspace" => Some(Kind::Workspace),
                "tab" => Some(Kind::Tab),
                "pane" => Some(Kind::Pane),
                "agent" => Some(Kind::Agent),
                _ => None,
            };
            if explicit_kind == Some(Kind::Agent) {
                let exact: Vec<_> = snapshot
                    .targets
                    .iter()
                    .filter(|t| t.kind == Kind::Agent && (t.id == value || t.label == value))
                    .cloned()
                    .collect();
                if !exact.is_empty() {
                    out.extend(exact);
                    continue;
                }
                let typed_agents: Vec<_> = snapshot
                    .targets
                    .iter()
                    .filter(|t| t.kind == Kind::Agent && t.agent_type.as_deref() == Some(value))
                    .cloned()
                    .collect();
                if !typed_agents.is_empty() {
                    out.extend(typed_agents);
                    continue;
                }
                if let Ok(n) = value.parse::<usize>()
                    && n > 0
                    && let Some(agent) = snapshot.by_kind(&Kind::Agent).get(n.saturating_sub(1))
                {
                    out.push((*agent).clone());
                    continue;
                }
            }
            let mut candidates = explicit_kind.clone().into_iter().collect::<Vec<_>>();
            if candidates.is_empty() {
                if kinds.contains(&Kind::Workspace) && kinds.contains(&Kind::Agent) {
                    candidates.push(Kind::Workspace);
                }
                for kind in kinds {
                    if !candidates.contains(kind) {
                        candidates.push(kind.clone());
                    }
                }
            }
            let mut matched = candidates.iter().find_map(|kind| {
                snapshot
                    .targets
                    .iter()
                    .find(|t| {
                        if &t.kind != kind {
                            return false;
                        }
                        if t.id == value || t.label == value {
                            return true;
                        }
                        match kind {
                            Kind::Workspace => t.number.is_some_and(|n| n.to_string() == value),
                            Kind::Tab => tab_selector_matches(snapshot, t, value),
                            Kind::Agent => pane_selector_matches(snapshot, t, value),
                            Kind::Pane => pane_selector_matches(snapshot, t, value),
                        }
                    })
                    .cloned()
            });
            if matched.is_none() && explicit_kind.is_none() {
                let exact = snapshot
                    .targets
                    .iter()
                    .find(|target| target.id == value)
                    .cloned();
                matched = exact.and_then(|target| {
                    if kinds.contains(&Kind::Agent) && target.kind == Kind::Pane {
                        snapshot
                            .targets
                            .iter()
                            .find(|candidate| {
                                candidate.kind == Kind::Agent && candidate.id == target.id
                            })
                            .cloned()
                    } else if kinds.contains(&Kind::Agent) && kinds.len() == 1 {
                        None
                    } else {
                        Some(target)
                    }
                });
            }
            if matched.is_none() && explicit_kind.is_none() && kinds.contains(&Kind::Agent) {
                let typed_agents: Vec<_> = snapshot
                    .targets
                    .iter()
                    .filter(|t| t.kind == Kind::Agent && t.agent_type.as_deref() == Some(value))
                    .cloned()
                    .collect();
                if !typed_agents.is_empty() {
                    out.extend(typed_agents);
                    continue;
                }
                if let Ok(n) = value.parse::<usize>()
                    && n > 0
                    && let Some(agent) = snapshot.by_kind(&Kind::Agent).get(n.saturating_sub(1))
                {
                    matched = Some((*agent).clone());
                }
            }
            if let Some(t) = matched {
                out.push(t);
            } else {
                return Err(ResolveError::Unknown(selector.clone()));
            }
        }
    }
    if out.is_empty() {
        return Err(ResolveError::Empty);
    }
    Ok(out)
}

fn tab_selector_matches(snapshot: &Snapshot, tab: &Target, value: &str) -> bool {
    let Some((workspace, number)) = value.split_once(':') else {
        return tab.number.is_some_and(|n| {
            n.to_string() == value && tab.workspace_id == snapshot.baseline_workspace
        });
    };
    let workspace = snapshot.targets.iter().find(|t| {
        t.kind == Kind::Workspace
            && (t.id == workspace
                || t.label == workspace
                || t.number.is_some_and(|n| n.to_string() == workspace))
    });
    workspace.is_some_and(|w| {
        tab.workspace_id.as_deref() == Some(&w.id)
            && tab.number.is_some_and(|n| n.to_string() == number)
    })
}

fn pane_selector_matches(snapshot: &Snapshot, pane: &Target, value: &str) -> bool {
    if let Some((workspace_tab, pane_number)) = value.rsplit_once('.') {
        let pane_match = pane
            .pane_index
            .is_some_and(|n| n.to_string() == pane_number);
        let scope_match = if let Some((workspace, tab_number)) = workspace_tab.split_once(':') {
            let workspace_id = snapshot
                .targets
                .iter()
                .find(|t| {
                    t.kind == Kind::Workspace
                        && (t.id == workspace
                            || t.label == workspace
                            || t.number.is_some_and(|n| n.to_string() == workspace))
                })
                .map(|t| t.id.as_str());
            workspace_id == pane.workspace_id.as_deref()
                && pane
                    .tab_id
                    .as_ref()
                    .and_then(|id| {
                        snapshot
                            .targets
                            .iter()
                            .find(|t| t.kind == Kind::Tab && &t.id == id)
                    })
                    .is_some_and(|t| t.number.is_some_and(|n| n.to_string() == tab_number))
        } else {
            pane.tab_id
                .as_ref()
                .and_then(|id| {
                    snapshot
                        .targets
                        .iter()
                        .find(|t| t.kind == Kind::Tab && &t.id == id)
                })
                .is_some_and(|t| t.number.is_some_and(|n| n.to_string() == workspace_tab))
                && pane.workspace_id == snapshot.baseline_workspace
        };
        return pane_match && scope_match;
    }
    pane.tab_id == snapshot.baseline_tab && pane.pane_index.is_some_and(|n| n.to_string() == value)
}

pub async fn run(
    ctx: &Context,
    mut queue: Vec<Target>,
    interval: Duration,
    limit: usize,
    random: bool,
    status: &HashSet<String>,
    verbose: bool,
) -> Result<usize, Error> {
    let mut done = 0usize;
    let mut first = true;
    let mut rng = rand::rng();
    loop {
        if !first && random {
            queue.shuffle(&mut rng);
        }
        first = false;
        let lap = queue
            .iter()
            .map(|target| target.id.clone())
            .collect::<Vec<_>>();
        let mut advanced = false;
        for target_id in lap {
            crate::runtime::wait_if_paused(&ctx.cancel).await;
            if ctx.cancel.is_cancelled() {
                return Ok(done);
            }
            if done >= limit && limit != 0 {
                return Ok(done);
            }
            let snapshot = match ctx.snapshot().await {
                Ok(value) => Snapshot::from_value(&value),
                Err(error) => {
                    if matches!(error, Error::Cancelled) {
                        return Ok(done);
                    }
                    eprintln!("Herdr snapshot failed; retrying: {error}");
                    if !crate::runtime::sleep_or_pause(Duration::from_millis(500), &ctx.cancel)
                        .await
                    {
                        return Ok(done);
                    }
                    continue;
                }
            };
            queue.retain(|target| {
                snapshot
                    .targets
                    .iter()
                    .any(|live| live.kind == target.kind && live.id == target.id)
            });
            if queue.is_empty() {
                return Err(Error::Api("all selected targets have disappeared".into()));
            }
            let Some(selected) = queue.iter().find(|target| target.id == target_id).cloned() else {
                continue;
            };
            let current = snapshot
                .targets
                .iter()
                .find(|target| target.kind == selected.kind && target.id == selected.id);
            let current_status = current.and_then(|target| target.status.as_ref());
            if selected.kind == Kind::Agent
                && !status.is_empty()
                && !current_status.is_some_and(|state| status.contains(state))
            {
                continue;
            }
            let kind = match selected.kind {
                Kind::Pane => "pane",
                Kind::Tab => "tab",
                Kind::Workspace => "workspace",
                Kind::Agent => "pane",
            };
            match ctx.focus(kind, &selected.id).await {
                Ok(_) => {
                    done += 1;
                    advanced = true;
                    if verbose {
                        println!("Focused {} {} ({done})", kind, selected.label);
                    }
                }
                Err(Error::Cancelled) => return Ok(done),
                Err(e) => eprintln!("Could not focus {}: {e}", selected.id),
            }
            if done >= limit && limit != 0 {
                return Ok(done);
            }
            if !crate::runtime::sleep_or_pause(interval, &ctx.cancel).await {
                return Ok(done);
            }
        }
        if !advanced && !crate::runtime::sleep_or_pause(interval, &ctx.cancel).await {
            return Ok(done);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decimal_comma_is_supported() {
        assert_eq!(parse_duration("1,25").unwrap(), Duration::from_millis(1250));
    }
    #[test]
    fn invalid_duration_is_rejected() {
        assert!(parse_duration("NaN").is_err());
        assert!(parse_duration("0").is_err());
        assert!(parse_duration("18446744073709551615").is_err());
        assert!(parse_duration("0.0000000001").is_err());
    }
    #[test]
    fn typed_selectors_preserve_duplicates() {
        let s = Snapshot {
            targets: vec![Target {
                kind: Kind::Workspace,
                id: "w7".into(),
                label: "Build".into(),
                status: None,
                ..Target::default()
            }],
            ..Snapshot::default()
        };
        let r = resolve(&s, &[], &["workspace=Build".into(), "workspace=w7".into()]).unwrap();
        assert_eq!(r.len(), 2);
    }
}
