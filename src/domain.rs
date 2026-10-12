use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum Kind {
    #[default]
    Pane,
    Tab,
    Workspace,
    Agent,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct Target {
    pub kind: Kind,
    pub id: String,
    pub label: String,
    pub status: Option<String>,
    pub workspace_id: Option<String>,
    pub tab_id: Option<String>,
    pub number: Option<u64>,
    pub pane_index: Option<usize>,
    pub agent_type: Option<String>,
    pub terminal_id: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub targets: Vec<Target>,
    pub focused: Option<String>,
    pub focused_tab: Option<String>,
    pub focused_workspace: Option<String>,
    pub baseline_workspace: Option<String>,
    pub baseline_tab: Option<String>,
    pub layouts: Vec<serde_json::Value>,
    pub active_tabs: Vec<(String, String)>,
}

impl Snapshot {
    pub fn from_value(v: &serde_json::Value) -> Self {
        let s = v.get("result").and_then(|v| v.get("snapshot")).unwrap_or(v);
        let mut targets = Vec::new();
        for (field, kind, id_field, label_field) in [
            ("panes", Kind::Pane, "pane_id", "terminal_title"),
            ("tabs", Kind::Tab, "tab_id", "label"),
            ("workspaces", Kind::Workspace, "workspace_id", "label"),
            ("agents", Kind::Agent, "pane_id", "name"),
        ] {
            if let Some(items) = s.get(field).and_then(|x| x.as_array()) {
                for item in items {
                    let Some(id) = item.get(id_field).and_then(|x| x.as_str()) else {
                        continue;
                    };
                    let label = item
                        .get(label_field)
                        .or_else(|| {
                            (kind == Kind::Pane || kind == Kind::Agent)
                                .then(|| item.get("terminal_title_stripped"))
                                .flatten()
                        })
                        .and_then(|x| x.as_str())
                        .unwrap_or(id)
                        .to_owned();
                    let status = item
                        .get("status")
                        .and_then(|x| x.as_str())
                        .or_else(|| item.get("agent_status").and_then(|x| x.as_str()))
                        .or_else(|| (kind == Kind::Agent).then_some("unknown"))
                        .map(str::to_owned);
                    targets.push(Target {
                        kind: kind.clone(),
                        id: id.to_owned(),
                        label,
                        status,
                        workspace_id: item
                            .get("workspace_id")
                            .and_then(|x| x.as_str())
                            .map(str::to_owned),
                        tab_id: item
                            .get("tab_id")
                            .and_then(|x| x.as_str())
                            .map(str::to_owned),
                        number: item.get("number").and_then(|x| x.as_u64()),
                        pane_index: None,
                        agent_type: item
                            .get("agent")
                            .and_then(|x| x.as_str())
                            .or_else(|| item.get("display_agent").and_then(|x| x.as_str()))
                            .map(str::to_owned),
                        terminal_id: item
                            .get("terminal_id")
                            .and_then(|x| x.as_str())
                            .map(str::to_owned),
                    });
                }
            }
        }
        if let Some(layouts) = s.get("layouts").and_then(|x| x.as_array()) {
            for layout in layouts {
                if let (Some(tab_id), Some(panes)) = (
                    layout.get("tab_id").and_then(|x| x.as_str()),
                    layout.get("panes").and_then(|x| x.as_array()),
                ) {
                    for (index, pane) in panes.iter().enumerate() {
                        if let Some(id) = pane.get("pane_id").and_then(|x| x.as_str())
                            && let Some(target) = targets
                                .iter_mut()
                                .find(|t| t.kind == Kind::Pane && t.id == id)
                        {
                            target.tab_id = Some(tab_id.to_owned());
                            target.pane_index = Some(index + 1);
                        }
                    }
                }
            }
        }
        let focused = s
            .get("focused_pane_id")
            .and_then(|x| x.as_str())
            .map(str::to_owned);
        let focused_target = focused
            .as_ref()
            .and_then(|id| targets.iter().find(|t| &t.id == id));
        let baseline_workspace =
            focused_target
                .and_then(|t| t.workspace_id.clone())
                .or_else(|| {
                    s.get("focused_workspace_id")
                        .and_then(|x| x.as_str())
                        .map(str::to_owned)
                });
        let baseline_tab = focused_target.and_then(|t| t.tab_id.clone()).or_else(|| {
            s.get("focused_tab_id")
                .and_then(|x| x.as_str())
                .map(str::to_owned)
        });
        Self {
            targets,
            focused,
            focused_tab: s
                .get("focused_tab_id")
                .and_then(|x| x.as_str())
                .map(str::to_owned),
            focused_workspace: s
                .get("focused_workspace_id")
                .and_then(|x| x.as_str())
                .map(str::to_owned),
            baseline_workspace,
            baseline_tab,
            layouts: s
                .get("layouts")
                .and_then(|x| x.as_array())
                .cloned()
                .unwrap_or_default(),
            active_tabs: s
                .get("workspaces")
                .and_then(|x| x.as_array())
                .into_iter()
                .flatten()
                .filter_map(|w| {
                    Some((
                        w.get("workspace_id")?.as_str()?.to_owned(),
                        w.get("active_tab_id")?.as_str()?.to_owned(),
                    ))
                })
                .collect(),
        }
    }

    pub fn by_kind(&self, kind: &Kind) -> Vec<&Target> {
        self.targets.iter().filter(|t| &t.kind == kind).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_keeps_opaque_ids() {
        let s = Snapshot::from_value(
            &serde_json::json!({"result":{"snapshot":{"focused_pane_id":"w2:p9","panes":[{"pane_id":"w2:p9","terminal_title":"shell"}]}}}),
        );
        assert_eq!(s.focused.as_deref(), Some("w2:p9"));
        assert_eq!(s.targets[0].id, "w2:p9");
    }
}
