use serde::{Deserialize, Serialize};

use super::rinco::Mood;
use super::{Action, Card, Kind};

const MAX_ANCESTORS: usize = 32;
const MAX_FULL: usize = 3;
const GREEN: u32 = 0x58_e6_a0;
const YELLOW: u32 = 0xf5_d7_6e;
const PURPLE: u32 = 0xb2_94_ff;
const RED: u32 = 0xff_6b_8b;
const CYAN: u32 = 0x45_d9_e8;
const PINK: u32 = 0xff_8a_a3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Press {
    Open(u64),
    Answer(Action),
    Dismiss,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Button {
    pub(crate) label: String,
    pub(crate) press: Press,
    pub(crate) fill: u32,
    pub(crate) text: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Tone {
    Done,
    Waiting,
    Question,
    Critical,
}

impl Tone {
    pub(crate) fn rgb(self) -> u32 {
        match self {
            Self::Done => GREEN,
            Self::Waiting => YELLOW,
            Self::Question => PURPLE,
            Self::Critical => RED,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    Picked(String),
    Running(String),
    Denied,
}

impl Outcome {
    pub(crate) fn line(&self) -> (String, u32) {
        match self {
            Self::Picked(option) => (format!("✓ You picked {option}"), GREEN),
            Self::Running(command) => (format!("◐ Running {command}…"), CYAN),
            Self::Denied => ("✕ Denied. Claude will try another way.".into(), PINK),
        }
    }

    pub(crate) fn mood(&self) -> Mood {
        match self {
            Self::Picked(_) => Mood::Done,
            Self::Running(_) => Mood::Working,
            Self::Denied => Mood::Error,
        }
    }

    pub(crate) fn monospaced(&self) -> bool {
        matches!(self, Self::Running(_))
    }
}

pub(crate) fn mood(card: &Card) -> Mood {
    match &card.kind {
        Kind::Asks(ask) if ask.question => Mood::Question,
        Kind::Asks(_) | Kind::Waiting => Mood::Needs,
        Kind::Done => Mood::Done,
        Kind::Working => Mood::Working,
    }
}

pub(crate) fn rank(card: &Card) -> u8 {
    match &card.kind {
        Kind::Asks(_) => 0,
        Kind::Waiting => 1,
        Kind::Done => 2,
        Kind::Working => 3,
    }
}

pub(crate) fn needs_you(card: &Card) -> bool {
    !matches!(card.kind, Kind::Working)
}

pub(crate) fn full(cards: &[Card], selected: Option<u64>, answered: &[u64]) -> Vec<bool> {
    let wanted = |card: &Card| {
        matches!(card.kind, Kind::Asks(_)) || answered.contains(&card.pane) || selected == Some(card.pane)
    };
    let mut full: Vec<bool> = cards.iter().map(wanted).collect();
    if !full.contains(&true)
        && let Some(first) = cards.iter().position(needs_you)
    {
        full[first] = true;
    }
    let mut shown = 0;
    for slot in &mut full {
        shown += usize::from(*slot);
        *slot &= shown <= MAX_FULL;
    }
    full
}

pub(crate) fn subtitle(card: &Card) -> String {
    match &card.prompt {
        Some(prompt) if card.title == card.place => format!("You: {prompt}"),
        Some(prompt) => format!("{} · You: {prompt}", card.place),
        None if card.title == card.place => String::new(),
        None => card.place.clone(),
    }
}

pub(crate) fn status(card: &Card) -> (String, u32) {
    match &card.kind {
        Kind::Done => ("Done — click to jump".into(), GREEN),
        Kind::Waiting => ("Needs you — click to jump".into(), YELLOW),
        Kind::Working => ("Working".into(), CYAN),
        Kind::Asks(ask) if ask.question => (ask.title.clone(), PURPLE),
        Kind::Asks(ask) if ask.detail.is_empty() => (format!("Wants: {}", ask.title), YELLOW),
        Kind::Asks(ask) => (format!("Wants to run: {}", ask.detail), YELLOW),
    }
}

pub(crate) fn outcome(card: &Card, key: char) -> Option<Outcome> {
    let Kind::Asks(ask) = &card.kind else { return None };
    let choice = ask.choices.iter().position(|c| c.key == key)?;
    Some(if ask.question {
        Outcome::Picked(ask.choices[choice].label.clone())
    } else if choice == 0 {
        Outcome::Running(if ask.detail.is_empty() { ask.title.clone() } else { ask.detail.clone() })
    } else {
        Outcome::Denied
    })
}

pub(crate) fn buttons(card: &Card) -> Vec<Button> {
    let button = |label: &str, press, (fill, text)| Button { label: label.to_string(), press, fill, text };
    let plain = (0x2e_2d_33, 0xff_ff_ff);
    let open = button("open", Press::Open(card.pane), plain);
    match &card.kind {
        Kind::Done => vec![open, button("dismiss", Press::Dismiss, plain)],
        Kind::Waiting | Kind::Working => vec![open],
        Kind::Asks(ask) => ask
            .choices
            .iter()
            .enumerate()
            .map(|(i, choice)| {
                let action = Action::Answer { pane: card.pane, ask: ask.clone(), key: choice.key };
                let colours = match (ask.question, i) {
                    (true, _) => (0x2a_21_50, 0xc9_b8_ff),
                    (false, 0) => (0x1d_3a_2c, GREEN),
                    (false, _) => (0x45_20_2b, PINK),
                };
                button(&choice.label, Press::Answer(action), colours)
            })
            .chain([open])
            .collect(),
    }
}

pub(crate) fn ago(secs: u64) -> String {
    match secs {
        0..60 => "now".into(),
        60..3_600 => format!("{}m", secs / 60),
        3_600..86_400 => format!("{}h", secs / 3_600),
        _ => format!("{}d", secs / 86_400),
    }
}

pub(crate) fn left(secs: i64) -> String {
    let minutes = (secs.max(0) + 59) / 60;
    match (minutes / 1_440, minutes / 60 % 24, minutes % 60) {
        (0, 0, minutes) => format!("{minutes}m"),
        (0, hours, minutes) => format!("{hours}h{minutes}m"),
        (days, hours, _) => format!("{days}d{hours}h"),
    }
}

pub(crate) fn usage_label(label: &str) -> Option<&'static str> {
    match label {
        "session (5h)" => Some("5h"),
        "week" => Some("7d"),
        _ => None,
    }
}

pub(crate) fn terminal(from: i32, parent: impl Fn(i32) -> Option<i32>, app: impl Fn(i32) -> bool) -> Option<i32> {
    let mut pid = from;
    for _ in 0..MAX_ANCESTORS {
        if pid <= 1 {
            return None;
        }
        if app(pid) {
            return Some(pid);
        }
        pid = parent(pid)?;
    }
    None
}

#[cfg(test)]
mod tests;
