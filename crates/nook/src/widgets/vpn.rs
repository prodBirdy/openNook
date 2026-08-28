//! VPN Nook pane: status, service name, interface, session clock.

use crate::island::ui::{nook_display, nook_empty, nook_pane};
use crate::theme;
use gpui::{div, prelude::*, px, rgba, FontWeight};
use nook_core::vpn::VpnSnapshot;
use std::time::SystemTime;

pub(crate) fn vpn_card(snap: &VpnSnapshot) -> impl IntoElement {
    if !snap.connected && snap.interface.is_empty() {
        return nook_pane("nook-vpn")
            .w_full()
            .child(nook_empty("shield", "No VPN"));
    }

    let elapsed = snap
        .elapsed_label(SystemTime::now())
        .unwrap_or_else(|| if snap.connected { "Connected".into() } else { "Off".into() });
    let name = snap.display_name();
    let detail = if snap.tunnel_count > 1 {
        format!("{} · {} tunnels", snap.interface, snap.tunnel_count)
    } else if snap.interface.is_empty() {
        name.clone()
    } else {
        snap.interface.clone()
    };

    nook_pane("nook-vpn")
        .w_full()
        .pr(px(4.))
        .child(
            div()
                .flex_1()
                .min_h(px(0.))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(10.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w(px(0.))
                        .child(nook_display(elapsed).text_color(if snap.connected {
                            theme::LABEL
                        } else {
                            theme::SECONDARY_LABEL
                        }))
                        .child(
                            div()
                                .text_size(px(11.))
                                .line_height(px(14.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme::SECONDARY_LABEL)
                                .child(name),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .line_height(px(14.))
                                .text_color(theme::TERTIARY_LABEL)
                                .child(detail),
                        ),
                )
                .child(status_dot(snap.connected)),
        )
}

fn status_dot(on: bool) -> impl IntoElement {
    div()
        .size(px(8.))
        .rounded_full()
        .flex_shrink_0()
        .bg(if on {
            theme::SUCCESS
        } else {
            rgba(0xffffff33)
        })
}
