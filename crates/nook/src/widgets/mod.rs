//! Expanded widget cards. One file per widget so each can be edited on its own.

mod agents;
mod battery;
mod calendar;
mod messages;
mod mixer;
mod high_alert;
mod notes;
mod notes_editor;
mod observe;
mod obsidian;
mod quick_add;
mod reminders;
mod speed;
mod terminal;
mod timers;
mod weather;
mod vpn;

pub(crate) use agents::{
    agents_card, compact_left as agents_compact_left, compact_right as agents_compact_right,
};
pub(crate) use battery::battery_card;
pub(crate) use calendar::calendar_card;
pub(crate) use messages::{
    compact_left as messages_compact_left, compact_right as messages_compact_right, messages_card,
};
pub(crate) use mixer::mixer_card;
pub(crate) use high_alert::high_alert_card;
pub(crate) use notes::notes_card;
pub(crate) use notes_editor::{NotesEditor, NotesEditorEvent};
pub(crate) use observe::{observe_card, ObserveHover};
pub(crate) use obsidian::obsidian_card;
pub(crate) use quick_add::{QuickAdd, QuickAddEvent};
pub(crate) use reminders::reminders_card;
pub(crate) use speed::speed_card;
pub(crate) use terminal::terminal_card;
pub(crate) use timers::{compact_left as timer_compact_left, timer_card};
pub(crate) use weather::{compact_weather, weather_card};
pub(crate) use vpn::vpn_card;
