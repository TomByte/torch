use super::*;

fn frames(reveal: Reveal, preference: MotionPreference) -> Vec<RevealFrame> {
    let steps = 10;
    (0..=steps)
        .map(|step| reveal.frame(reveal.duration() * step / steps, preference))
        .collect()
}

#[test]
fn appearing_surfaces_settle_exactly_from_transparent() {
    for reveal in [Reveal::Arrive, Reveal::Pop, Reveal::Swap] {
        let frames = frames(reveal, MotionPreference::Full);
        assert_eq!(frames[0].opacity, 0.);
        assert_eq!(
            frames.last(),
            Some(&RevealFrame {
                opacity: 1.,
                offset: 0.,
            })
        );
        assert!(frames.windows(2).all(|pair| {
            pair[1].opacity >= pair[0].opacity && pair[1].offset >= pair[0].offset
        }));
    }
}

#[test]
fn panels_drop_further_than_menus_and_swaps_stay_in_place() {
    let start = |reveal: Reveal| reveal.frame(Duration::ZERO, MotionPreference::Full).offset;
    assert!(start(Reveal::Arrive) < start(Reveal::Pop));
    assert!(start(Reveal::Pop) < 0.);
    assert_eq!(start(Reveal::Swap), 0.);
}

#[test]
fn leaving_is_quicker_than_arriving_and_ends_invisible() {
    assert!(Reveal::Leave.duration() < Reveal::Arrive.duration());
    let end = Reveal::Leave.frame(Reveal::Leave.duration(), MotionPreference::Full);
    assert_eq!(end.opacity, 0.);
    assert!(end.offset < 0.);
}

#[test]
fn reduced_motion_keeps_the_fade_without_moving() {
    for reveal in [Reveal::Arrive, Reveal::Pop, Reveal::Swap, Reveal::Leave] {
        let frames = frames(reveal, MotionPreference::Reduced);
        assert!(frames.iter().all(|frame| frame.offset == 0.));
        assert_ne!(frames[0].opacity, frames[frames.len() - 1].opacity);
    }
}
