//! Notification shelf: recent banners captured from other apps.

use crate::icons::lucide_color;
use crate::island::ui::{
    label, nook_display, nook_empty, nook_icon_btn, nook_pane, nook_row, scroll_body,
};
use crate::island::Island;
use crate::platform;
use crate::theme;
use gpui::{
    div, img, prelude::*, px, rgba, AnyElement, Context, CursorStyle, Image, MouseButton,
    MouseDownEvent, ObjectFit, SharedString,
};
use nook_core::notifications::{relative_age, NotificationEvent};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn compact_left(latest: Option<&NotificationEvent>) -> AnyElement {
    if let Some(event) = latest {
        if let Some(icon) = app_icon(&event.bundle_id, &event.app_name) {
            return icon;
        }
    }
    lucide_color("bell", theme::COMPACT_FACE, theme::LABEL).into_any_element()
}

pub(crate) fn compact_right(unread: usize, latest: Option<&NotificationEvent>) -> AnyElement {
    if unread > 0 {
        return label(unread.to_string(), theme::BODY, true).into_any_element();
    }
    if let Some(event) = latest {
        let text = if event.title.is_empty() {
            event.app_name.clone()
        } else {
            event.title.clone()
        };
        return label(text, theme::BODY, true).into_any_element();
    }
    label("0", theme::BODY, true).into_any_element()
}

pub(crate) fn notifications_card(
    events: &[NotificationEvent],
    cx: &mut Context<Island>,
) -> impl IntoElement {
    let unread = events.iter().filter(|e| e.unread).count();
    let body = if events.is_empty() {
        nook_empty("bell", "No notifications").into_any_element()
    } else {
        let mut list = div().flex().flex_col().w_full();
        for event in events.iter().take(12) {
            list = list.child(notification_row(event, cx));
        }
        scroll_body("notify-scroll", list).into_any_element()
    };

    nook_pane("nook-notifications")
        .w_full()
        .child(
            div()
                .flex()
                .items_end()
                .gap(px(16.))
                .flex_shrink_0()
                .child(nook_display(if unread > 0 {
                    unread.to_string()
                } else {
                    events.len().to_string()
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .pb(px(4.))
                        .child(nook_icon_btn("eye", "notify-read", cx, |this, _, _, cx| {
                            nook_core::notifications::mark_all_read();
                            this.refresh_notifications();
                            cx.notify();
                        }))
                        .child(nook_icon_btn(
                            "trash-2",
                            "notify-clear",
                            cx,
                            |this, _, _, cx| {
                                nook_core::notifications::clear();
                                this.refresh_notifications();
                                cx.notify();
                            },
                        )),
                ),
        )
        .child(body)
}

fn notification_row(event: &NotificationEvent, cx: &mut Context<Island>) -> impl IntoElement {
    let id = event.id.clone();
    let title = if event.title.is_empty() {
        event.app_name.clone()
    } else {
        event.title.clone()
    };
    let detail = if event.body.is_empty() {
        event.subtitle.clone()
    } else {
        event.body.clone()
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let age = relative_age(event.delivered_at, now);
    let unread = event.unread;

    nook_row(SharedString::from(format!("notify-{}", event.id)))
        .min_h(px(theme::HIT_MIN))
        .gap(px(8.))
        .cursor(CursorStyle::PointingHand)
        .hover(|s| s.bg(rgba(0xFFFFFF0D)))
        .active(|s| s.opacity(0.85))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                nook_core::notifications::dismiss(&id);
                this.refresh_notifications();
                cx.notify();
            }),
        )
        .child(
            div()
                .size(px(22.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    app_icon(&event.bundle_id, &event.app_name).unwrap_or_else(|| {
                        lucide_color("bell", 14.0, theme::LABEL).into_any_element()
                    }),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(px(1.))
                .child(label(title, theme::CALLOUT, unread))
                .when(!detail.is_empty(), |d| {
                    d.child(label(detail, theme::SUBHEADLINE, false))
                }),
        )
        .child(label(age, theme::FOOTNOTE, false))
}

fn app_icon(bundle_id: &str, app_name: &str) -> Option<AnyElement> {
    let key = if bundle_id.is_empty() {
        app_name.to_string()
    } else {
        bundle_id.to_string()
    };
    if key.is_empty() {
        return None;
    }
    let png = cached_icon(&key, bundle_id, app_name)?;
    let image = std::sync::Arc::new(Image::from_bytes(gpui::ImageFormat::Png, png));
    Some(
        img(image)
            .size(px(18.))
            .rounded(px(4.))
            .object_fit(ObjectFit::Fill)
            .into_any_element(),
    )
}

fn cached_icon(key: &str, bundle_id: &str, app_name: &str) -> Option<Vec<u8>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Vec<u8>>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(guard) = cache.lock() {
        if let Some(hit) = guard.get(key) {
            return hit.clone();
        }
    }
    let bid = (!bundle_id.is_empty()).then_some(bundle_id);
    let name = (!app_name.is_empty()).then_some(app_name);
    let loaded = platform::app_icon_png(bid, name);
    if let Ok(mut guard) = cache.lock() {
        guard.insert(key.to_string(), loaded.clone());
    }
    loaded
}
