use std::time::UNIX_EPOCH;

use super::App;
use super::toast::Toast;
use crate::agents::activity::Status;
use crate::agents::ask::Ask;
use crate::host::notch::card as notch_card;
use crate::host::notch::{Action, Board, Card, Kind};
use crate::log;
use crate::term::Term;
use crate::ui;

pub(super) const CHANGED: &str = "the agent no longer asks that; answer it here";

#[derive(Debug)]
pub(super) struct Notch {
    pub(super) cards: Vec<(Card, u64)>,
    pub(super) looking: bool,
}

impl Default for Notch {
    fn default() -> Self {
        Self { cards: Vec::new(), looking: true }
    }
}

impl App {
    pub(crate) fn set_looking(&mut self, looking: bool) {
        if self.agents.notch.looking != looking {
            log::info!("app", "window focus", looking = looking);
            self.agents.notch.looking = looking;
        }
    }

    #[cfg(test)]
    pub(crate) fn looking(&self) -> bool {
        self.agents.notch.looking
    }

    pub(crate) fn notch_cards(&self, focused: bool) -> Option<Board> {
        let visible = self.visible_tab();
        self.config.notch.then(|| {
            let hidden = |tab: u64| !(focused && visible == Some(tab));
            let cards =
                self.agents.notch.cards.iter().filter(|(_, tab)| hidden(*tab)).map(|(card, _)| card.clone()).collect();
            Board { cards }
        })
    }

    pub(super) fn build_notch(&mut self) {
        let mut cards = Vec::new();
        for project in self.projects.iter().filter(|p| self.config.notch && !p.closing) {
            for workspace in project.workspaces.iter().filter(|w| w.open()) {
                let place = format!("{} › {}", self.project_label(project), workspace.label());
                for tab in &workspace.tabs {
                    for term in &tab.panes {
                        if let Some(card) = card(term, tab.name.as_deref(), &place) {
                            cards.push((card, tab.id));
                        }
                    }
                }
            }
        }
        cards.sort_by_key(|(card, _)| (notch_card::rank(card), std::cmp::Reverse(card.since)));
        self.agents.notch.cards = cards;
    }

    pub(crate) fn notch_action(&mut self, action: Action) {
        match action {
            Action::Open(pane) => {
                log::info!("app", "notch opens a pane", pane = pane);
                self.jump_to_pane(pane);
            }
            Action::Answer { pane, ask, key } => {
                let term = self
                    .projects
                    .iter_mut()
                    .flat_map(|p| p.workspaces.iter_mut())
                    .find_map(|w| w.tabs.iter_mut().flat_map(|t| t.panes.iter_mut()).find(|term| term.id == pane));
                let fresh = term.filter(|term| {
                    term.agent.status() == Some(Status::Waiting)
                        && asked(term).as_ref() == Some(&ask)
                        && ask.choices.iter().any(|c| c.key == key)
                });
                let answered = fresh.is_some_and(|term| term.write(key.to_string().as_bytes()));
                log::info!("app", "notch answer", pane = pane, sent = answered, question = ask.question);
                if !answered {
                    self.jump_to_pane(pane);
                    self.toast = Some(Toast::new(CHANGED, ui::ToastIcon::Agent(Status::Waiting)));
                }
            }
        }
    }
}

fn card(term: &Term, name: Option<&str>, place: &str) -> Option<Card> {
    term.agent.agent().filter(|agent| agent.notch())?;
    let kind = match term.agent.status()? {
        Status::Done => Kind::Done,
        Status::Waiting => asked(term).map_or(Kind::Waiting, Kind::Asks),
        Status::Working | Status::Idle => return None,
    };
    let title = name.or(term.context.title()).unwrap_or(place).to_string();
    let since = term.agent.changed().and_then(|at| at.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs());
    Some(Card { pane: term.id, title, place: place.to_string(), since, kind })
}

fn asked(term: &Term) -> Option<Ask> {
    term.context.ask(&term.emulator.screen_text().ok()?)
}
