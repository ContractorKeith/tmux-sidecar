use crate::model::AgentRun;
use serde::Deserialize;
#[derive(Clone, Debug, Deserialize)]
pub struct Run {
    #[serde(flatten)]
    pub run: AgentRun,
    pub pane_name: Option<String>,
}
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Group {
    pub name: String,
    pub server_start: Option<u64>,
    pub pane_id: Option<String>,
}
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub runs: Vec<Run>,
    pub groups: Vec<Group>,
    pub connected: bool,
    pub error: String,
}
#[derive(Default)]
pub struct App {
    pub data: Snapshot,
    pub selected: usize,
    pub manual: bool,
    pub filter: String,
    pub filtering: bool,
    pub message: String,
    pub color: bool,
}
pub enum Row<'a> {
    Heading(String),
    Run(&'a Run),
}
impl App {
    pub fn rows(&self) -> Vec<Row<'_>> {
        use crate::model::AgentState::*;
        let runs: Vec<_> = self
            .data
            .runs
            .iter()
            .filter(|r| {
                format!(
                    "{} {} {} {:?} {:?} {}",
                    r.run.id,
                    r.run.pane_id,
                    r.pane_name.as_deref().unwrap_or(""),
                    r.run.agent,
                    r.run.state,
                    r.run.cwd
                )
                .to_lowercase()
                .contains(&self.filter.to_lowercase())
            })
            .collect();
        let mut attention: Vec<_> = runs
            .iter()
            .copied()
            .filter(|r| {
                matches!(r.run.state, Blocked | Error | Waiting)
                    && r.run.ended_at.is_none()
                    && r.run.read_at.is_none_or(|at| at < r.run.state_since)
            })
            .collect();
        attention.sort_by_key(|r| {
            (
                match r.run.state {
                    Blocked => 0,
                    Error => 1,
                    _ => 2,
                },
                r.run.state_since,
                r.run.id[1..].parse::<u64>().unwrap_or(0),
            )
        });
        let mut rows = vec![Row::Heading(format!("Attention ({})", attention.len()))];
        rows.extend(attention.into_iter().map(Row::Run));
        let mut groups = std::collections::BTreeMap::<String, Vec<&Run>>::new();
        for run in runs {
            let names: Vec<_> = if self.manual {
                self.data
                    .groups
                    .iter()
                    .filter(|g| {
                        g.server_start == Some(run.run.server_start)
                            && g.pane_id.as_deref() == Some(&run.run.pane_id)
                    })
                    .map(|g| g.name.clone())
                    .collect()
            } else {
                vec![
                    run.run
                        .project_root
                        .clone()
                        .unwrap_or_else(|| run.run.cwd.clone()),
                ]
            };
            for name in if names.is_empty() {
                vec!["Ungrouped".into()]
            } else {
                names
            } {
                groups.entry(name).or_default().push(run);
            }
        }
        for (name, runs) in groups {
            rows.push(Row::Heading(name));
            rows.extend(runs.into_iter().map(Row::Run));
        }
        rows
    }
    pub fn selected_run(&self) -> Option<&Run> {
        self.rows()
            .into_iter()
            .filter_map(|r| match r {
                Row::Run(run) => Some(run),
                _ => None,
            })
            .nth(self.selected)
    }
    fn run_keys(&self) -> Vec<(String, String)> {
        let mut section = String::new();
        let mut keys = Vec::new();
        for (index, row) in self.rows().into_iter().enumerate() {
            match row {
                Row::Heading(name) => {
                    section = if index == 0 {
                        "attention".into()
                    } else {
                        format!("group:{name}")
                    }
                }
                Row::Run(run) => keys.push((section.clone(), run.run.id.clone())),
            }
        }
        keys
    }
    pub(super) fn replace(&mut self, data: Snapshot) {
        let key = self.run_keys().get(self.selected).cloned();
        self.data = data;
        if let Some(index) = self
            .run_keys()
            .iter()
            .position(|candidate| Some(candidate) == key.as_ref())
        {
            self.selected = index;
        }
        self.clamp();
    }
    pub(super) fn clamp(&mut self) {
        let count = self
            .rows()
            .iter()
            .filter(|r| matches!(r, Row::Run(_)))
            .count();
        self.selected = self.selected.min(count.saturating_sub(1));
    }
}
