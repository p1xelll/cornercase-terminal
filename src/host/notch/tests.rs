use super::*;
use crate::agents::ask::Choice;

#[test]
fn cards_and_actions_cross_the_pipe_as_json_lines() {
    let ask = Ask {
        question: true,
        title: "Which colour?".into(),
        detail: "Colour".into(),
        choices: vec![Choice { label: "Red".into(), key: '1' }],
    };
    let action = Action::Answer { pane: 4, ask: ask.clone(), key: '1' };
    let card = Card {
        pane: 4,
        agent: "claude".into(),
        title: "Colour preference".into(),
        place: "shop › main".into(),
        prompt: Some("ask me\nwhich colour".into()),
        since: Some(1_791_820_339),
        kind: Kind::Asks(ask),
    };
    let usage = vec![Usage { label: "5h".into(), percent: 11, tone: card::Tone::Done, resets_at: None }];
    let board = Board { cards: vec![card], usage };

    let line = serde_json::to_string(&(board.clone(), action.clone())).expect("encode");

    assert!(!line.contains('\n'));
    assert_eq!(serde_json::from_str::<(Board, Action)>(&line).expect("decode"), (board, action));
}
