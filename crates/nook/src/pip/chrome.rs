//! Hover controls and the shared floating-window surface.

use super::{PipKind, PipView};
use crate::icons::lucide_color;
use crate::island::Island;
use crate::theme;
use gpui::{
    div, prelude::*, px, Animation, AnimationExt, AnyElement, Context, CursorStyle, MouseButton,
    MouseDownEvent, Rgba, SharedString, Window,
};
use std::cell::Cell;
use std::time::Duration;

thread_local! {
    static LAST_HEIGHT: Cell<[f32; 2]> = const { Cell::new([f32::NAN, f32::NAN]) };
    static FADE: Cell<[(bool, u64); 2]> = const { Cell::new([(false, 0), (false, 0)]) };
}

pub const LYRICS_W: f32 = 300.0;
pub const MINI_W: f32 = 260.0;
pub const MINI_H: f32 = 36.0;
pub const OBSERVE_W: f32 = 300.0;

const RADIUS: f32 = 18.0;
const PAD_TOP: f32 = 10.0;
const PAD_X: f32 = 14.0;
const PAD_BOTTOM: f32 = 12.0;
const GAP: f32 = 6.0;
const CTRL: f32 = 22.0;
const ICON: f32 = 12.0;
const FADE_MS: u64 = 150;
/// `#FFFFFF1F` — 1 px inner stroke. AppKit paints the drop shadow.
const STROKE: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 31.0 / 255.0,
};
/// `#0A84FF33` — Keep on Top fill while pinned.
const PIN_ON_BG: Rgba = Rgba {
    r: 0.039,
    g: 0.518,
    b: 1.0,
    a: 51.0 / 255.0,
};

pub fn is_mini(view: &PipView) -> bool {
    view.kind == PipKind::Lyrics && view.mini
}

pub fn surface(view: &PipView, content: AnyElement) -> AnyElement {
    let mini = is_mini(view);
    div()
        .relative()
        .size_full()
        .overflow_hidden()
        .rounded(px(RADIUS))
        .bg(theme::ISLAND)
        .border_1()
        .border_color(STROKE)
        .when(mini, |d| {
            d.h(px(MINI_H))
                .px(px(8.))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
        })
        .when(!mini, |d| {
            d.flex()
                .flex_col()
                .pt(px(PAD_TOP))
                .px(px(PAD_X))
                .pb(px(PAD_BOTTOM))
                .gap(px(GAP))
        })
        .child(content)
        .into_any_element()
}

/// Trailing header cluster. Callers mount this only while `view.hovered`.
pub fn hover_controls(view: &PipView, reduce_motion: bool, cx: &mut Context<PipView>) -> AnyElement {
    let pinned = view.keep_on_top();
    let row = div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(GAP))
        .child(chrome_btn(
            "pip-keep",
            "pin",
            if pinned {
                theme::accent()
            } else {
                theme::LABEL
            },
            if pinned {
                PIN_ON_BG
            } else {
                theme::FILL_SECONDARY
            },
            cx,
            |this, window, cx| this.toggle_keep_on_top(window, cx),
        ))
        .child(chrome_btn(
            "pip-back",
            "picture-in-picture-2",
            theme::LABEL,
            theme::FILL_SECONDARY,
            cx,
            |this, window, cx| this.back_to_notch(window, cx),
        ))
        .child(chrome_btn(
            "pip-close",
            "x",
            theme::LABEL,
            theme::FILL_SECONDARY,
            cx,
            |this, window, cx| this.close_window(window, cx),
        ));
    if reduce_motion {
        return row.into_any_element();
    }
    let fade = fade_generation(view);
    row.with_animation(
        SharedString::from(format!("pip-hover-{fade}")),
        Animation::new(Duration::from_millis(FADE_MS)).with_easing(gpui::ease_out_quint()),
        |el, t| el.opacity(t),
    )
    .into_any_element()
}

fn chrome_btn(
    id: &'static str,
    icon: &'static str,
    icon_color: Rgba,
    bg: Rgba,
    cx: &mut Context<PipView>,
    on_click: impl Fn(&mut PipView, &mut Window, &mut Context<PipView>) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .size(px(CTRL))
        .rounded_full()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(bg)
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.75))
        .child(lucide_color(icon, ICON, icon_color))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                on_click(this, window, cx);
            }),
        )
}

/// Resize only when the hover transport (or mini) actually changes the height.
/// The first frame is recorded so open does not fight P1's initial size.
pub fn note_content_height(
    view: &PipView,
    height: f32,
    window: &mut Window,
    cx: &mut Context<PipView>,
) {
    touch_fade(view);
    let idx = kind_index(view.kind);
    let prev = LAST_HEIGHT.with(|cell| cell.get()[idx]);
    if prev.is_finite() && (prev - height).abs() < 0.5 {
        return;
    }
    LAST_HEIGHT.with(|cell| {
        let mut heights = cell.get();
        heights[idx] = height;
        cell.set(heights);
    });
    if !prev.is_finite() {
        return;
    }
    cx.defer_in(window, move |view, window, cx| {
        view.set_content_height(height, window, cx);
    });
}

pub fn desired_height(view: &PipView, island: &Island) -> f32 {
    match view.kind {
        PipKind::Lyrics => super::lyrics::desired_height(view, island),
        PipKind::Observe => super::observe::desired_height(view, island),
    }
}

fn kind_index(kind: PipKind) -> usize {
    match kind {
        PipKind::Lyrics => 0,
        PipKind::Observe => 1,
    }
}

/// Bumps when hover begins so the 150 ms opacity fade restarts after a hide.
fn touch_fade(view: &PipView) {
    let idx = kind_index(view.kind);
    FADE.with(|cell| {
        let mut slots = cell.get();
        let (was, gen) = slots[idx];
        let gen = if view.hovered && !was {
            gen.wrapping_add(1)
        } else {
            gen
        };
        slots[idx] = (view.hovered, gen);
        cell.set(slots);
    });
}

fn fade_generation(view: &PipView) -> u64 {
    FADE.with(|cell| cell.get()[kind_index(view.kind)].1)
}
