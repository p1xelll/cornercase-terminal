use rstest::rstest;

use super::*;

const MOODS: [Mood; 6] = [Mood::Idle, Mood::Done, Mood::Working, Mood::Error, Mood::Question, Mood::Needs];

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

#[test]
fn every_frame_is_a_sixteen_by_sixteen_grid_of_palette_colours() {
    let kit = kit().expect("the kit parses");
    for (name, animation) in &kit.states {
        for frame in &animation.frames {
            assert_eq!(frame.rows.len(), SIZE, "{name}");
            for row in &frame.rows {
                assert_eq!(row.chars().count(), SIZE, "{name}: {row}");
                assert!(row.chars().all(|c| c == '.' || colour(c).is_some()), "{name}: {row}");
            }
        }
    }
}

#[rstest]
#[case::idle(Mood::Idle, &[2600, 120, 260, 120], true)]
#[case::working(Mood::Working, &[240, 240, 240, 240], true)]
#[case::needs(Mood::Needs, &[170, 170, 170, 450], true)]
#[case::question(Mood::Question, &[900, 650, 400, 650], true)]
#[case::done(Mood::Done, &[110, 150, 110, 150, 2200], false)]
#[case::error(Mood::Error, &[60, 60, 60, 60, 1800], false)]
fn each_state_keeps_the_designs_timing(#[case] mood: Mood, #[case] timing: &[u64], #[case] looping: bool) {
    let animation = &kit().expect("the kit parses").states[mood.key()];
    let found: Vec<u64> = animation.frames.iter().map(|f| f.ms).collect();

    assert_eq!((found.as_slice(), animation.looping), (timing, looping));
}

#[test]
fn a_loop_starts_over() {
    assert_eq!([0, 2599, 2600, 2719, 2720, 3100].map(|n| index(Mood::Idle, ms(n))), [0, 0, 1, 1, 2, 0].map(Some));
}

#[test]
fn a_one_shot_holds_its_last_frame() {
    assert_eq!([0, 110, 520, 2719, 60_000].map(|n| index(Mood::Done, ms(n))), [0, 1, 4, 4, 4].map(Some));
}

#[test]
fn the_most_urgent_state_leads() {
    let order = [Mood::Needs, Mood::Question, Mood::Error, Mood::Working, Mood::Done, Mood::Idle];
    for (i, mood) in order.iter().enumerate() {
        assert_eq!(urgent(order[i..].iter().rev().copied()), *mood);
    }
    assert_eq!(urgent([]), Mood::Idle);
}

#[test]
fn every_state_has_frames_and_its_own_antenna() {
    let mut antennas: Vec<u32> = MOODS.iter().map(|m| m.antenna()).collect();
    antennas.dedup();

    assert!(MOODS.iter().all(|m| frame(*m, Duration::ZERO).is_some()));
    assert_eq!(antennas.len(), MOODS.len());
}
