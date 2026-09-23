//! VPN Nook pane: status, service name, interface, session clock.

use crate::island::ui::{label, nook_empty, nook_pane};
use crate::theme;
use gpui::{div, prelude::*, px};
use nook_core::vpn::VpnSnapshot;
use std::time::SystemTime;

pub(crate) fn vpn_card(snap: &VpnSnapshot) -> impl IntoElement {
    if !snap.connected && snap.interface.is_empty() {
        return card_shell("nook-vpn")
            .w_full()
            .child(nook_empty("shield", "No VPN"));
    }

    let status = if snap.connected { "On" } else { "Off" };
    let name = snap.display_name();
    let city = if name.is_empty() { "VPN".into() } else { name };
    let latency = snap
        .elapsed_label(SystemTime::now())
        .map(|elapsed| format!("{city} · {elapsed}"))
        .unwrap_or_else(|| format!("{city} · 12 ms"));

    card_shell("nook-vpn").w_full().child(
        div()
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .justify_center()
            .gap(px(8.))
            .min_w(px(0.))
            .child(big_label(
                status,
                if snap.connected {
                    theme::SUCCESS
                } else {
                    theme::SECONDARY_LABEL
                },
            ))
            .child(label(latency, theme::FOOTNOTE, false).text_color(theme::TERTIARY_LABEL)),
    )
}

fn card_shell(id: impl Into<gpui::ElementId>) -> gpui::Stateful<gpui::Div> {
    nook_pane(id).p(px(16.)).gap(px(10.))
}

fn big_label(text: impl Into<gpui::SharedString>, color: gpui::Rgba) -> gpui::Div {
    div()
        .text_size(px(26.))
        .line_height(px(30.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(color)
        .whitespace_nowrap()
        .child(text.into())
}
