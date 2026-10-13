use serde::{Deserialize, Serialize};

use crate::agents::ask::Ask;
#[cfg(not(target_os = "macos"))]
use crate::error::Error;
use crate::error::Result;

pub(crate) mod card;
pub(crate) mod motion;
#[cfg(target_os = "macos")]
mod panel;
pub(crate) mod rinco;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Board {
    pub(crate) cards: Vec<Card>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Card {
    pub(crate) pane: u64,
    pub(crate) title: String,
    pub(crate) place: String,
    pub(crate) since: Option<u64>,
    pub(crate) kind: Kind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Kind {
    Done,
    Waiting,
    Asks(Ask),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Action {
    Open(u64),
    Answer { pane: u64, ask: Ask, key: char },
}

impl Action {
    pub(crate) fn pane(&self) -> u64 {
        match self {
            Self::Open(pane) | Self::Answer { pane, .. } => *pane,
        }
    }
}

pub(crate) const fn available() -> bool {
    cfg!(target_os = "macos")
}

#[cfg_attr(target_os = "macos", expect(clippy::unnecessary_wraps, reason = "it fails outside macOS"))]
pub(crate) fn run() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        panel::run();
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    Err(Error::Control("the notch is only on macOS".into()))
}

#[cfg(test)]
mod tests;
