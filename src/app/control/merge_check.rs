use std::path::PathBuf;

use super::changes::Place;
use super::{Asker, Handled, conflict, json, malformed};
use crate::app::App;
use crate::app::merge_check::MergeAnswer;
use crate::changes::merge::{Check, Checked};
use crate::control::ChangesMergeCheck;
use crate::control::changes::{MergeChecked, MergeInfo};
use crate::control::contract::Refusal;
use crate::project::Hierarchy;

impl App {
    pub(super) fn changes_merge_check_request(
        &mut self,
        client: u64,
        asker: Asker,
        request: &ChangesMergeCheck,
    ) -> Handled {
        let place = self.changes_place(asker, request.workspace)?;
        let check = self.requested_check(&place, request)?;
        let key = self.clients.requests.key();
        self.spawn_merge_check(place.id, MergeAnswer::Request(key), check);
        self.pend_reading(client, key);
        Ok(None)
    }

    fn requested_check(&self, place: &Place, request: &ChangesMergeCheck) -> Result<Check, Refusal> {
        let check = Check { dir: place.dir.clone(), base: place.base.clone(), with_base: true, others: Vec::new() };
        match (request.with, &request.base) {
            (Some(_), Some(_)) => Err(malformed("take --with or --base, not both")),
            (Some(with), None) => {
                let other = self.other_checkout(place, with)?;
                Ok(Check { with_base: false, others: vec![other], ..check })
            }
            (None, Some(base)) => Ok(Check { base: Some(base.clone()), ..check }),
            (None, None) => Ok(Check { others: self.shared_checkouts(place.id), ..check }),
        }
    }

    fn other_checkout(&self, place: &Place, with: u64) -> Result<(u64, PathBuf), Refusal> {
        let other = self.git_workspace(with)?;
        let at = |id| self.projects.workspace_index(id);
        let same_project = matches!((at(with), at(place.id)), (Some(a), Some(b)) if a.project() == b.project());
        if !same_project {
            return Err(conflict(format!("workspace {with} is in another project")));
        }
        if other.path == place.dir {
            let message = format!("workspace {with} shares this workspace's folder, so they are one checkout");
            return Err(conflict(message));
        }
        Ok((with, other.path.clone()))
    }

    pub(in crate::app) fn shared_checkouts(&self, workspace: u64) -> Vec<(u64, PathBuf)> {
        let paths = self.shared_with(workspace).into_iter().map(|(id, _)| id);
        paths.filter_map(|id| self.projects.workspace_by(id).map(|w| (id, w.path.clone()))).collect()
    }

    pub(in crate::app) fn merge_check_replied(&mut self, key: u64, workspace: u64, checked: &Checked) {
        let checks: Vec<MergeInfo> = checked.merges.iter().map(MergeInfo::from).collect();
        self.answer_read(key, |app| {
            app.usable_workspace(workspace)?;
            Ok(json(&MergeChecked { workspace, checks }))
        });
    }
}
