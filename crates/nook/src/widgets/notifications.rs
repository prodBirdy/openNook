//! Notification shelf: recent banners captured from other apps.

use crate::icons::lucide_color;
use crate::island::ui::{label, nook_empty, nook_pane, scroll_body};
use crate::island::Island;
use crate::platform;
use crate::theme;
use gpui::{
    div, img, linear_color_stop, linear_gradient, prelude::*, px, rgba, AnyElement, Context,
    CursorStyle, Image, MouseButton, MouseDownEvent, ObjectFit, SharedString,
};
use nook_core::notifications::{relative_age, NotificationEvent};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn compact_left(latest: Option<&NotificationEvent>) -> AnyElement {
    if let Some(event) = latest {
        if let Some(icon) = app_icon(&event.bundle_id, &event.app_name, 18.) {
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
    let body = if events.is_empty() {
        nook_empty("bell", "No notifications").into_any_element()
    } else {
        let mut list = div().flex().flex_col().w_full().gap(px(8.)).pb(px(24.));
        for event in events.iter().take(30) {
            list = list.child(notification_row(event, cx));
        }
        // Scroll area with a soft fade at the bottom so the last card reads
        // as "more below" instead of being clipped.
        div()
            .relative()
            .flex_1()
            .min_h(px(0.))
            .w_full()
            .child(scroll_body("notify-scroll", list))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(36.))
                    .bg(linear_gradient(
                        180.0,
                        linear_color_stop(rgba(0x00000000), 0.0),
                        linear_color_stop(rgba(0x000000CC), 1.0),
                    )),
            )
            .into_any_element()
    };

    nook_pane("nook-notifications").w_full().child(body)
}

fn notification_row(event: &NotificationEvent, cx: &mut Context<Island>) -> impl IntoElement {
    let dismiss_id = event.id.clone();
    let read_id = event.id.clone();
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

    div()
        .id(SharedString::from(format!("notify-{}", event.id)))
        .w_full()
        .flex()
        .items_start()
        .gap(px(10.))
        .px(px(12.))
        .py(px(10.))
        .rounded(px(16.))
        .bg(if unread {
            rgba(0xFFFFFF14)
        } else {
            rgba(0xFFFFFF0A)
        })
        .hover(|s| s.bg(rgba(0xFFFFFF1C)))
        .cursor(CursorStyle::PointingHand)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                nook_core::notifications::mark_read(&read_id);
                this.refresh_notifications();
                cx.notify();
            }),
        )
        .child(
            div()
                .size(px(34.))
                .flex_shrink_0()
                .mt(px(4.))
                .rounded(px(8.))
                .bg(rgba(0xFFFFFF10))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    app_icon(&event.bundle_id, &event.app_name, 24.).unwrap_or_else(|| {
                        lucide_color("bell", 16.0, theme::LABEL).into_any_element()
                    }),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(label(title, theme::BODY, true).flex_1().min_w(px(0.)))
                        .child(label(age, theme::SUBHEADLINE, false).flex_shrink_0()),
                )
                .when(!event.app_name.is_empty(), |d| {
                    d.child(label(event.app_name.clone(), theme::SUBHEADLINE, false))
                })
                .when(!detail.is_empty(), |d| {
                    d.child(
                        div()
                            .text_color(theme::TEXT)
                            .text_size(px(theme::CALLOUT.size))
                            .line_height(px(theme::CALLOUT.leading))
                            .line_clamp(2)
                            .child(SharedString::from(detail)),
                    )
                }),
        )
        .child(
            div()
                .id(SharedString::from(format!("notify-x-{}", event.id)))
                .size(px(24.))
                .flex_shrink_0()
                .mt(px(2.))
                .rounded_full()
                .bg(rgba(0xFFFFFF14))
                .hover(|s| s.bg(rgba(0xFFFFFF26)))
                .active(|s| s.opacity(0.7))
                .flex()
                .items_center()
                .justify_center()
                .child(lucide_color("x", 12.0, theme::LABEL))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        nook_core::notifications::dismiss(&dismiss_id);
                        this.refresh_notifications();
                        cx.notify();
                    }),
                ),
        )
}

fn app_icon(bundle_id: &str, app_name: &str, size: f32) -> Option<AnyElement> {
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
            .size(px(size))
            .rounded(px(size * 0.22))
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
