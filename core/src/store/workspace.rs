//! Library-local last-workspace activation. Resolving it grants no content or PDF
//! capability, and never opens a provider, recogniser or production H.I.
use super::*;
use crate::sources::Source;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitAxis {
    Horizontal,
    Vertical,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkspaceTool {
    Pen,
    Eraser,
    Selection,
    Text,
    Shape,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum PaneTarget {
    View {
        view: Id,
        tool: WorkspaceTool,
    },
    Pdf {
        source: Id,
        tool: WorkspaceTool,
    },
    /// Retain only this existing slot's private target/tool, without revealing it.
    Keep,
    Unavailable,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspacePane {
    pub slot: Id,
    pub weight: u32,
    pub target: PaneTarget,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceUpdate {
    pub panes: Vec<WorkspacePane>,
    pub active: Option<Id>,
    pub axis: SplitAxis,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum RestoredPaneState {
    View {
        view: Box<RestoredView>,
        tool: WorkspaceTool,
    },
    Pdf {
        source: Box<Source>,
        tool: WorkspaceTool,
        requires_platform_validation: bool,
    },
    Locked,
    Unavailable,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoredPane {
    pub slot: Id,
    pub weight: u32,
    pub state: RestoredPaneState,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoredWorkspace {
    pub revision: u64,
    pub axis: SplitAxis,
    pub active: Option<Id>,
    pub panes: Vec<RestoredPane>,
}
impl WorkspaceUpdate {
    fn validate(&self) -> Result<()> {
        let slots: BTreeSet<_> = self.panes.iter().map(|p| p.slot).collect();
        if self.panes.len() > 8
            || slots.len() != self.panes.len()
            || self.active.is_some_and(|id| !slots.contains(&id))
            || self
                .panes
                .iter()
                .any(|p| !(1..=1_000_000).contains(&p.weight))
        {
            return Err(Error::Invalid("workspace layout"));
        }
        Ok(())
    }
}
fn tool_name(tool: WorkspaceTool) -> &'static str {
    match tool {
        WorkspaceTool::Pen => "Pen",
        WorkspaceTool::Eraser => "Eraser",
        WorkspaceTool::Selection => "Selection",
        WorkspaceTool::Text => "Text",
        WorkspaceTool::Shape => "Shape",
    }
}
impl Store {
    pub(crate) fn stored_workspace(db: &Connection) -> Result<(u64, WorkspaceUpdate)> {
        let (revision, active, axis): (u64, Option<String>, String) = db.query_row(
            "SELECT revision,active,axis FROM workspace_state WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let axis = match axis.as_str() {
            "Horizontal" => SplitAxis::Horizontal,
            "Vertical" => SplitAxis::Vertical,
            _ => return Err(Error::Integrity),
        };
        let mut q = db.prepare("SELECT slot,ordinal,kind,view,source,weight,tool FROM workspace_panes ORDER BY ordinal")?;
        let mut panes = vec![];
        for row in q.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, usize>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, u32>(5)?,
                r.get::<_, String>(6)?,
            ))
        })? {
            let (slot, ordinal, kind, view, source, weight, tool) = row?;
            if ordinal != panes.len() || panes.len() >= 8 {
                return Err(Error::Integrity);
            }
            let tool: WorkspaceTool = serde_json::from_value(serde_json::Value::String(tool))?;
            let target = match (kind.as_str(), view, source) {
                ("View", Some(id), None) => PaneTarget::View {
                    view: Id::parse(&id)?,
                    tool,
                },
                ("Pdf", None, Some(id)) => PaneTarget::Pdf {
                    source: Id::parse(&id)?,
                    tool,
                },
                ("Unavailable", None, None) => PaneTarget::Unavailable,
                _ => return Err(Error::Integrity),
            };
            panes.push(WorkspacePane {
                slot: Id::parse(&slot)?,
                weight,
                target,
            });
        }
        let update = WorkspaceUpdate {
            panes,
            active: active.as_deref().map(Id::parse).transpose()?,
            axis,
        };
        update.validate()?;
        Ok((revision, update))
    }
    pub fn save_workspace(
        &mut self,
        mut update: WorkspaceUpdate,
        expected: u64,
        access: &Access,
    ) -> Result<u64> {
        update.validate()?;
        self.write(|db| {
            Self::require_app(db, access)?;
            let (revision, previous) = Self::stored_workspace(db)?;
            if revision != expected {
                return Err(Error::Conflict);
            }
            let next = revision
                .checked_add(1)
                .filter(|r| *r <= i64::MAX as u64)
                .ok_or(Error::ResourceLimit)?;
            let previous: BTreeMap<_, _> = previous
                .panes
                .into_iter()
                .map(|p| (p.slot, p.target))
                .collect();
            for pane in &mut update.panes {
                match &pane.target {
                    PaneTarget::Keep => {
                        pane.target = previous.get(&pane.slot).cloned().ok_or(Error::Conflict)?
                    }
                    PaneTarget::View { view, .. } => {
                        Self::read_view_db(db, *view, access)?;
                    }
                    PaneTarget::Pdf { source, .. } => {
                        Self::read_source(db, *source, access)?;
                    }
                    PaneTarget::Unavailable => {}
                }
            }
            db.execute("DELETE FROM workspace_panes", [])?;
            for (ordinal, pane) in update.panes.iter().enumerate() {
                let (kind, view, source, tool) = match pane.target {
                    PaneTarget::View { view, tool } => ("View", Some(view.to_string()), None, tool),
                    PaneTarget::Pdf { source, tool } => {
                        ("Pdf", None, Some(source.to_string()), tool)
                    }
                    PaneTarget::Unavailable => ("Unavailable", None, None, WorkspaceTool::Pen),
                    PaneTarget::Keep => return Err(Error::Integrity),
                };
                db.execute(
                    "INSERT INTO workspace_panes VALUES(?1,?2,?3,?4,?5,?6,?7)",
                    rusqlite::params![
                        pane.slot.to_string(),
                        ordinal,
                        kind,
                        view,
                        source,
                        pane.weight,
                        tool_name(tool)
                    ],
                )?;
            }
            let axis = match update.axis {
                SplitAxis::Horizontal => "Horizontal",
                SplitAxis::Vertical => "Vertical",
            };
            db.execute(
                "UPDATE workspace_state SET revision=?1,active=?2,axis=?3 WHERE id=1",
                rusqlite::params![next, update.active.map(|id| id.to_string()), axis],
            )?;
            Ok(next)
        })
    }
    pub fn restore_workspace(&self, access: &Access) -> Result<RestoredWorkspace> {
        Self::require_app(&self.db, access)?;
        let (revision, update) = Self::stored_workspace(&self.db)?;
        let mut panes = vec![];
        for pane in update.panes {
            let result = match pane.target {
                PaneTarget::View { view, tool } => {
                    Self::read_view_db(&self.db, view, access).map(|v| RestoredPaneState::View {
                        view: Box::new(v),
                        tool,
                    })
                }
                PaneTarget::Pdf { source, tool } => Self::read_source(&self.db, source, access)
                    .map(|s| RestoredPaneState::Pdf {
                        source: Box::new(s),
                        tool,
                        requires_platform_validation: true,
                    }),
                PaneTarget::Unavailable => Ok(RestoredPaneState::Unavailable),
                PaneTarget::Keep => return Err(Error::Integrity),
            };
            let state = match result {
                Ok(state) => state,
                Err(Error::Denied) => RestoredPaneState::Locked,
                Err(Error::Unavailable) => RestoredPaneState::Unavailable,
                Err(error) => return Err(error),
            };
            panes.push(RestoredPane {
                slot: pane.slot,
                weight: pane.weight,
                state,
            });
        }
        Ok(RestoredWorkspace {
            revision,
            axis: update.axis,
            active: update.active,
            panes,
        })
    }
}
