//! Expanded widget cards. One file per widget so each can be edited on its own.

mod agents;
mod battery;
mod calendar;
mod high_alert;
mod meeting;
mod messages;
mod notes;
mod notes_editor;
mod notifications;
mod observe;
mod obsidian;
mod quick_add;
mod recorder;
mod reminders;
mod speed;
mod sysstats;
mod terminal;
mod timers;
mod vpn;
mod weather;

pub(crate) use agents::{
    agents_card, compact_left as agents_compact_left, compact_right as agents_compact_right,
};
pub(crate) use battery::{battery_card, tint as battery_tint};
pub(crate) use calendar::calendar_card;
pub(crate) use high_alert::high_alert_card;
pub(crate) use meeting::{
    compact_left as meeting_compact_left, compact_right as meeting_compact_right, meeting_card,
};
pub(crate) use messages::{
    compact_left as messages_compact_left, compact_right as messages_compact_right, messages_card,
};
pub(crate) use notes::notes_card;
pub(crate) use notes_editor::{NotesEditor, NotesEditorEvent};
pub(crate) use notifications::{
    compact_left as notifications_compact_left, compact_right as notifications_compact_right,
    notifications_card,
};
pub(crate) use observe::{
    observe_big_view, observe_card, ObserveHover, OBSERVE_EXPANDED_BODY,
};
pub(crate) use obsidian::obsidian_card;
pub(crate) use quick_add::{QuickAdd, QuickAddEvent};
pub(crate) use recorder::{
    compact_left as recorder_compact_left, compact_right as recorder_compact_right, recorder_card,
    COMPACT_EXTRA as RECORDER_COMPACT_EXTRA, COMPACT_HOVER_EXTRA as RECORDER_COMPACT_HOVER_EXTRA,
};
pub(crate) use reminders::reminders_card;
pub(crate) use speed::speed_card;
pub(crate) use sysstats::sysstats_card;
pub(crate) use terminal::{terminal_card, terminal_pane_min_height, TerminalEvent, TerminalView};
pub(crate) use timers::{compact_left as timer_compact_left, timer_card};
pub(crate) use vpn::vpn_card;
pub(crate) use weather::{compact_weather, weather_card};
