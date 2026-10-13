use super::*;
use crate::agents::ask::Ask;
use crate::agents::claude::fake::FakeClaude;
use crate::host::notch::{Action, Card, Kind};

const ASKING_CLAUDE: &str = r#"#!/bin/sh
s="$1/sessions/$$.json"
p=$(printf '%s' "$PWD" | tr -c 'a-zA-Z0-9' '-')
mkdir -p "$1/projects/$p"
printf '{"type":"ai-title","aiTitle":"Fix the auth bug"}\n' > "$1/projects/$p/s1.jsonl"
say() { printf '{"pid":%s,"sessionId":"s1","cwd":"%s","status":"%s"%s}' $$ "$PWD" "$1" "$2" > "$d/$$.next"; mv "$d/$$.next" "$s"; }
d="$1"
if [ "$2" = shell ]; then
  say shell ""
  while [ -d "$1" ] && [ ! -e "$1/quit" ]; do sleep 0.02; done
  rm -f "$s"; exit
fi
say busy ""
while [ ! -e "$1/ask" ]; do sleep 0.02; done
cat "$1/ask"
say waiting ',"waitingFor":"permission prompt"'
stty -icanon -echo min 1
dd bs=1 count=1 of="$1/typed" 2>/dev/null
mv "$1/typed" "$1/answer"
printf '\033[2J\033[H'
say busy ""
while [ -d "$1" ] && [ ! -e "$1/quit" ]; do sleep 0.02; done
rm -f "$s"
"#;
const PROMPT: [&str; 10] = [
    "──────────────────────────",
    " Bash command",
    "╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌",
    " touch made-by-claude",
    "╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌",
    " Do you want to proceed?",
    " ❯ 1. Yes",
    "   2. No",
    "",
    " Esc to cancel · Tab to amend",
];

struct Asking {
    app: App,
    rx: Receiver<AppEvent>,
    claude: FakeClaude,
    _dirs: Vec<TempDir>,
}

impl Asking {
    fn start(hidden: bool) -> Self {
        let (mut app, rx, dirs) = app_with(1);
        app.config.notch = true;
        let claude = FakeClaude::running(ASKING_CLAUDE);
        claude.start(&mut app);
        pump_until(&mut app, &rx, "claude runs", |a| runs(a, agents::Known::Claude.name()));
        if hidden {
            app.add_tab(0, 0, AREA).expect("add a tab");
        }
        std::fs::write(claude.dir().join("ask"), PROMPT.join("\n") + "\n").expect("ask");
        let mut asking = Self { app, rx, claude, _dirs: dirs };
        watch_until(&mut asking.app, &asking.rx, "claude asks", |a| {
            a.agents.notch.cards.iter().any(|(card, _)| matches!(card.kind, Kind::Asks(_)))
        });
        asking
    }

    fn card(&self) -> Card {
        self.app.notch_cards(false).and_then(|board| board.cards.into_iter().next()).expect("a card")
    }

    fn ask(&self) -> Ask {
        match self.card().kind {
            Kind::Asks(ask) => ask,
            other => panic!("not asking: {other:?}"),
        }
    }

    fn answer(&self) -> Option<String> {
        std::fs::read_to_string(self.claude.dir().join("answer")).ok()
    }
}

#[test]
fn a_permission_prompt_offers_allow_and_deny() {
    let asking = Asking::start(true);

    let ask = asking.ask();
    let choices: Vec<(&str, char)> = ask.choices.iter().map(|c| (c.label.as_str(), c.key)).collect();
    assert_eq!(
        (ask.title.as_str(), ask.detail.as_str(), choices),
        ("Bash command", "touch made-by-claude", vec![("allow", '1'), ("deny", '2')])
    );
}

#[test]
fn a_card_names_the_conversation_and_when_it_began() {
    let asking = Asking::start(true);

    let card = asking.card();

    assert_eq!((card.title.as_str(), card.since.is_some()), ("Fix the auth bug", true));
}

#[test]
fn an_agent_left_with_a_background_shell_gets_no_card_next_to_one_that_asks() {
    let mut asking = Asking::start(true);
    let line = asking.claude.command_line("shell");
    type_line(&mut asking.app, &line);

    watch_until(&mut asking.app, &asking.rx, "the second claude has a background shell", |a| {
        a.term().is_some_and(|t| t.agent.background_shell())
    });

    let kinds: Vec<bool> = asking
        .app
        .notch_cards(false)
        .map_or_else(Vec::new, |b| b.cards.iter().map(|c| matches!(c.kind, Kind::Asks(_))).collect());
    assert_eq!(kinds, [true]);
}

#[test]
fn an_answer_types_the_digit_of_the_choice() {
    let mut asking = Asking::start(true);
    let ask = asking.ask();

    asking.app.notch_action(Action::Answer { pane: asking.card().pane, ask, key: '2' });

    wait_until("claude reads the answer", || asking.answer().is_some());
    assert_eq!(asking.answer().as_deref(), Some("2"));
}

#[test]
fn an_answer_to_something_else_jumps_to_the_pane_instead() {
    let mut asking = Asking::start(true);
    let pane = asking.card().pane;
    let ask = Ask { title: "Edit file".into(), ..asking.ask() };

    asking.app.notch_action(Action::Answer { pane, ask, key: '1' });

    let shown = asking.app.tab().and_then(Tab::pane).map(|t| t.id);
    assert_eq!((shown, asking.answer()), (Some(pane), None));
    assert_eq!(toast(&asking.app), Some(crate::app::notch::CHANGED));
}

#[test]
fn open_shows_the_pane() {
    let mut asking = Asking::start(true);
    let pane = asking.card().pane;

    asking.app.notch_action(Action::Open(pane));

    assert_eq!(asking.app.tab().and_then(Tab::pane).map(|t| t.id), Some(pane));
}

#[test]
fn the_tab_you_look_at_has_no_card_while_its_window_has_the_focus() {
    let asking = Asking::start(false);

    let cards = |focused| asking.app.notch_cards(focused).map(|board| board.cards.len());
    assert_eq!((cards(true), cards(false)), (Some(0), Some(1)));
}

#[test]
fn turned_off_it_shows_nothing() {
    let mut asking = Asking::start(true);

    asking.app.config.notch = false;
    tick(&mut asking.app);

    assert_eq!((asking.app.notch_cards(false), asking.app.agents.notch.cards.len()), (None, 0));
}

#[test]
fn the_card_goes_once_the_agent_takes_the_answer() {
    let mut asking = Asking::start(true);
    let ask = asking.ask();
    asking.app.notch_action(Action::Answer { pane: asking.card().pane, ask, key: '1' });

    watch_until(&mut asking.app, &asking.rx, "the card goes once the agent is at work", |a| {
        a.agents.notch.cards.is_empty()
    });
}
