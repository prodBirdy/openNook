//! Sliding text: a label that carousels when its string is wider than the box
//! it was given, so the tail is readable instead of ellipsised away.
//!
//! HIG › Typography asks that text stay legible and HIG › Motion asks that
//! motion be purposeful, so the slide is not continuous: each pass starts with
//! the head of the string parked long enough to read it, then travels at a
//! reading pace and wraps seamlessly. A string that already fits never moves
//! and never asks for a frame — the cost is paid only where text is actually
//! truncated.

use crate::theme;
use gpui::{
    canvas, point, prelude::*, px, App, Bounds, Canvas, ContentMask, Hsla, Pixels, ShapedLine,
    SharedString, Window,
};
use std::sync::OnceLock;
use std::time::Instant;

/// Seconds the head of the string sits still at the start of every pass.
const DWELL: f32 = 1.8;
/// Travel speed in points per second — slow enough to read at a glance.
const SPEED: f32 = 30.0;
/// Blank run between the tail of one pass and the head of the next.
const GAP: f32 = 48.0;
/// Slack before a string counts as overflowing. Shaping and layout round
/// differently, so an exact comparison makes flush text twitch.
const SLOP: f32 = 0.5;

/// Shared phase clock. Every marquee reads the same origin, so two of them in
/// one card (a title over a subtitle) travel in lockstep rather than beating
/// against each other.
fn clock() -> f32 {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    ORIGIN.get_or_init(Instant::now).elapsed().as_secs_f32()
}

/// Where a pass sits at time `now`, given the full cycle distance `travel`.
/// Exposed for tests: the wrap has to land exactly on `travel` or the seam
/// between the two painted copies shows.
fn offset_at(now: f32, travel: f32) -> f32 {
    let scroll = travel / SPEED;
    let cycle = DWELL + scroll;
    let t = now.rem_euclid(cycle);
    if t < DWELL {
        0.0
    } else {
        ((t - DWELL) * SPEED).min(travel)
    }
}

/// Drop-in for [`super::ui::label`] wherever the box can be narrower than the
/// string: same typography and color roles, but the text slides instead of
/// truncating. Carries no intrinsic width — give it `w_full` or `flex_1` the
/// way the ellipsising label was given one.
pub(crate) fn slide_label(
    text: impl Into<SharedString>,
    style: theme::Text,
    strong: bool,
) -> Canvas<ShapedLine> {
    let text = text.into();
    let color: Hsla = if strong {
        theme::TEXT
    } else {
        theme::TEXT_MUTED
    }
    .into();
    let weight = if strong {
        style.emphasized
    } else {
        style.weight
    };
    let size = px(style.size);
    let leading = px(style.leading);

    canvas(
        move |_bounds, window, _cx| {
            // Inherit family and fallbacks from the root's font stack, then
            // override only what the text style names.
            let mut text_style = window.text_style();
            text_style.font_size = size.into();
            text_style.font_weight = weight;
            text_style.color = color;
            let run = text_style.to_run(text.len());
            window
                .text_system()
                .shape_line(text.clone(), size, &[run], None)
        },
        move |bounds, line, window, cx| paint_slide(bounds, line, leading, window, cx),
    )
    // Height only. Width is the caller's, and it must stay shrinkable: in a
    // flex row a `w_full` marquee has to give way to the icon beside it the
    // same way the ellipsising label did.
    .h(leading)
}

fn paint_slide(
    bounds: Bounds<Pixels>,
    line: ShapedLine,
    leading: Pixels,
    window: &mut Window,
    cx: &mut App,
) {
    let avail: f32 = bounds.size.width.into();
    let width: f32 = line.width.into();
    if width <= avail + SLOP || avail <= 0.0 {
        let _ = line.paint(bounds.origin, leading, window, cx);
        return;
    }

    let travel = width + GAP;
    let offset = offset_at(clock(), travel);
    let x = bounds.origin.x - px(offset);
    let y = bounds.origin.y;
    let right = bounds.origin.x + bounds.size.width;

    window.with_content_mask(Some(ContentMask { bounds }), |window| {
        let _ = line.paint(point(x, y), leading, window, cx);
        // The trailing copy is what makes the wrap seamless; it only exists
        // once the gap has opened far enough to expose the right edge.
        let follower = x + px(travel);
        if follower < right {
            let _ = line.paint(point(follower, y), leading, window, cx);
        }
    });

    // Only overflowing text drives the compositor. Text that fits returned above.
    window.request_animation_frame();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parks_through_the_dwell() {
        assert_eq!(offset_at(0.0, 200.0), 0.0);
        assert_eq!(offset_at(DWELL - 0.01, 200.0), 0.0);
        assert!(offset_at(DWELL + 0.5, 200.0) > 0.0);
    }

    #[test]
    fn wrap_lands_exactly_on_travel() {
        let travel = 200.0;
        let cycle = DWELL + travel / SPEED;
        // The instant before the wrap the pass has covered the whole distance,
        // which is where the follower copy is sitting at offset 0.
        assert!((offset_at(cycle - 0.001, travel) - travel).abs() < 0.1);
        assert_eq!(offset_at(cycle, travel), 0.0);
    }

    #[test]
    fn every_cycle_repeats() {
        let travel = 137.0;
        let cycle = DWELL + travel / SPEED;
        for t in [0.0, 0.9, 2.5, 4.0] {
            assert!((offset_at(t, travel) - offset_at(t + cycle * 3.0, travel)).abs() < 0.01);
        }
    }

    #[test]
    fn never_runs_past_the_wrap() {
        let travel = 90.0;
        let mut t = 0.0;
        while t < 40.0 {
            let o = offset_at(t, travel);
            assert!((0.0..=travel).contains(&o), "t={t} -> {o}");
            t += 0.017;
        }
    }
}
