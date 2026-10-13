use super::*;

#[test]
fn names_and_order_keep_the_saved_stats_positions() {
    assert_eq!(Known::ALL.map(Known::name), ["claude", "codex", "gemini", "opencode", "grok"]);
}

#[test]
fn only_claude_code_shows_in_the_notch() {
    let shown: Vec<_> = Known::ALL.into_iter().filter(|agent| agent.notch()).map(Known::name).collect();

    assert_eq!(shown, ["claude"]);
}
