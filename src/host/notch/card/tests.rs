use rstest::rstest;

use super::*;
use crate::agents::ask::{Ask, Choice};

fn card(kind: Kind) -> Card {
    Card {
        pane: 7,
        agent: "Claude Code".into(),
        title: "Fix the auth bug".into(),
        place: "shop › main".into(),
        prompt: Some("fix the auth bug in middleware".into()),
        since: None,
        kind,
    }
}

fn asks(question: bool) -> Kind {
    let choices = if question {
        vec![Choice { label: "Red".into(), key: '1' }, Choice { label: "Green".into(), key: '2' }]
    } else {
        vec![Choice { label: "allow".into(), key: '1' }, Choice { label: "deny".into(), key: '4' }]
    };
    Kind::Asks(Ask {
        question,
        title: if question { "Which colour?" } else { "Bash command" }.into(),
        detail: if question { "Colour" } else { "npm test -- auth" }.into(),
        choices,
    })
}

fn on(pane: u64, kind: Kind) -> Card {
    Card { pane, ..card(kind) }
}

#[rstest]
#[case::done(Kind::Done)]
#[case::waiting(Kind::Waiting)]
#[case::working(Kind::Working)]
#[case::permission(asks(false))]
#[case::question(asks(true))]
fn every_card_opens_its_own_pane(#[case] kind: Kind) {
    let opens: Vec<Press> = buttons(&card(kind)).into_iter().filter(|b| b.label == "open").map(|b| b.press).collect();

    assert_eq!(opens, vec![Press::Open(7)]);
}

#[test]
fn a_permission_offers_allow_in_green_deny_in_pink_then_open() {
    let found: Vec<(String, u32, u32)> =
        buttons(&card(asks(false))).into_iter().map(|b| (b.label, b.fill, b.text)).collect();

    assert_eq!(
        found,
        [
            ("allow".into(), 0x1d_3a_2c, 0x58_e6_a0),
            ("deny".into(), 0x45_20_2b, 0xff_8a_a3),
            ("open".into(), 0x2e_2d_33, 0xff_ff_ff)
        ]
    );
}

#[test]
fn a_choice_answers_with_its_key() {
    let presses: Vec<Press> = buttons(&card(asks(false))).into_iter().map(|b| b.press).collect();

    assert!(matches!(&presses[1], Press::Answer(Action::Answer { pane: 7, key: '4', .. })));
}

#[rstest]
#[case::done(Kind::Done, "Done — click to jump", 0x58_e6_a0)]
#[case::waiting(Kind::Waiting, "Needs you — click to jump", 0xf5_d7_6e)]
#[case::permission(asks(false), "Wants to run: npm test -- auth", 0xf5_d7_6e)]
#[case::question(asks(true), "Which colour?", 0xb2_94_ff)]
fn the_status_line_says_what_the_agent_wants(#[case] kind: Kind, #[case] text: &str, #[case] colour: u32) {
    assert_eq!(status(&card(kind)), (text.to_string(), colour));
}

#[rstest]
#[case::permission(asks(false), Mood::Needs)]
#[case::waiting(Kind::Waiting, Mood::Needs)]
#[case::question(asks(true), Mood::Question)]
#[case::done(Kind::Done, Mood::Done)]
#[case::working(Kind::Working, Mood::Working)]
fn rinco_shows_what_each_card_is_about(#[case] kind: Kind, #[case] expected: Mood) {
    assert_eq!(mood(&card(kind)), expected);
}

#[rstest]
#[case::picked(asks(true), '2', Outcome::Picked("Green".into()), "✓ You picked Green", Mood::Done)]
#[case::allowed(asks(false), '1', Outcome::Running("npm test -- auth".into()), "◐ Running npm test -- auth…", Mood::Working)]
#[case::denied(asks(false), '4', Outcome::Denied, "✕ Denied. Claude will try another way.", Mood::Error)]
fn an_answer_shows_what_it_did(
    #[case] kind: Kind,
    #[case] key: char,
    #[case] expected: Outcome,
    #[case] line: &str,
    #[case] then: Mood,
) {
    let found = outcome(&card(kind), key).expect("an outcome");

    assert_eq!((found.line().0.as_str(), found.mood()), (line, then));
    assert_eq!(found, expected);
}

#[test]
fn asking_cards_are_full_and_the_rest_are_rows() {
    let cards = [on(1, Kind::Done), on(2, asks(true)), on(3, asks(false)), on(4, Kind::Working)];

    assert_eq!(full(&cards, None, &[]), [false, true, true, false]);
}

#[test]
fn without_a_question_the_first_card_that_needs_you_is_full() {
    let cards = [on(1, Kind::Working), on(2, Kind::Done), on(3, Kind::Waiting)];

    assert_eq!(full(&cards, None, &[]), [false, true, false]);
}

#[test]
fn a_selected_or_answered_card_is_full_and_three_at_most() {
    let cards = [on(1, asks(true)), on(2, asks(false)), on(3, Kind::Done), on(4, asks(true))];

    assert_eq!(full(&cards, Some(3), &[]), [true, true, true, false]);
    assert_eq!(full(&[on(5, Kind::Done), on(6, Kind::Done)], None, &[6]), [false, true]);
}

#[test]
fn working_agents_alone_do_not_need_you() {
    assert_eq!((needs_you(&card(Kind::Working)), needs_you(&card(Kind::Done))), (false, true));
}

#[test]
fn the_subtitle_names_the_place_and_your_last_prompt() {
    let mut card = card(Kind::Done);
    let both = subtitle(&card);
    card.title.clone_from(&card.place);
    let prompt = subtitle(&card);
    card.prompt = None;

    assert_eq!(
        (both.as_str(), prompt.as_str(), subtitle(&card).as_str()),
        ("shop › main · You: fix the auth bug in middleware", "You: fix the auth bug in middleware", "")
    );
}

#[test]
fn questions_come_first_then_waiting_then_done_then_working() {
    let mut ranks: Vec<u8> = [Kind::Working, Kind::Done, asks(true), Kind::Waiting].map(|k| rank(&card(k))).to_vec();
    ranks.sort_unstable();

    assert_eq!(ranks, [0, 1, 2, 3]);
}

#[rstest]
#[case::just_now(30, "now")]
#[case::minutes(28 * 60, "28m")]
#[case::hours(5 * 3_600 + 100, "5h")]
#[case::days(3 * 86_400, "3d")]
fn ago_is_short(#[case] secs: u64, #[case] text: &str) {
    assert_eq!(ago(secs), text);
}

#[rstest]
#[case::minutes(12 * 60, "12m")]
#[case::hours_and_minutes(4 * 3_600 + 61, "4h2m")]
#[case::days(2 * 86_400 + 3 * 3_600, "2d3h")]
#[case::past(-5, "0m")]
fn left_is_short(#[case] secs: i64, #[case] text: &str) {
    assert_eq!(left(secs), text);
}

#[test]
fn only_the_session_and_the_week_make_the_header() {
    assert_eq!(["session (5h)", "week", "week · Fable"].map(usage_label), [Some("5h"), Some("7d"), None]);
}

#[test]
fn the_terminal_is_the_first_app_up_the_tree() {
    assert_eq!(terminal(10, |pid| Some(pid - 1), |pid| pid == 6), Some(6));
}

#[test]
fn a_parent_that_cannot_be_read_ends_the_search() {
    assert_eq!(terminal(10, |pid| (pid > 8).then_some(pid - 1), |pid| pid == 6), None);
}

#[test]
fn no_app_up_to_launchd_means_none() {
    assert_eq!(terminal(10, |pid| Some(pid - 1), |_| false), None);
}
