use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::Duration;

use serde::Deserialize;

const KIT: &str = include_str!("rinco.json");
pub(crate) const SIZE: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Mood {
    Idle,
    Done,
    Working,
    Error,
    Question,
    Needs,
}

impl Mood {
    fn key(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Done => "done",
            Self::Working => "working",
            Self::Error => "error",
            Self::Question => "question",
            Self::Needs => "needs",
        }
    }

    pub(crate) fn antenna(self) -> u32 {
        match self {
            Self::Idle => 0x0b_8f_a3,
            Self::Done => 0x58_e6_a0,
            Self::Working => 0x45_d9_e8,
            Self::Error => 0xff_6b_8b,
            Self::Question => 0xc9_b8_ff,
            Self::Needs => 0xf5_d7_6e,
        }
    }
}

#[derive(Debug, Deserialize)]
struct Kit {
    palette: BTreeMap<char, String>,
    states: BTreeMap<String, Animation>,
}

#[derive(Debug, Deserialize)]
struct Animation {
    #[serde(rename = "loop")]
    looping: bool,
    frames: Vec<Frame>,
}

#[derive(Debug, Deserialize)]
struct Frame {
    ms: u64,
    rows: Vec<String>,
}

fn kit() -> Option<&'static Kit> {
    static PARSED: OnceLock<Option<Kit>> = OnceLock::new();
    PARSED.get_or_init(|| serde_json::from_str(KIT).ok()).as_ref()
}

pub(crate) fn urgent(moods: impl IntoIterator<Item = Mood>) -> Mood {
    moods.into_iter().max().unwrap_or(Mood::Idle)
}

pub(crate) fn frame(mood: Mood, elapsed: Duration) -> Option<&'static [String]> {
    Some(&kit()?.states.get(mood.key())?.frames[index(mood, elapsed)?].rows)
}

pub(crate) fn index(mood: Mood, elapsed: Duration) -> Option<usize> {
    let animation = kit()?.states.get(mood.key())?;
    let total: u64 = animation.frames.iter().map(|f| f.ms).sum();
    let mut at = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
    if !animation.looping && at >= total {
        return animation.frames.len().checked_sub(1);
    }
    at %= total.max(1);
    animation.frames.iter().position(|f| {
        let inside = at < f.ms;
        at = at.saturating_sub(f.ms);
        inside
    })
}

pub(crate) fn colour(pixel: char) -> Option<u32> {
    let hex = kit()?.palette.get(&pixel)?;
    u32::from_str_radix(hex.trim_start_matches('#'), 16).ok()
}

#[cfg(test)]
mod tests;
