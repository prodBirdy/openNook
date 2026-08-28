//! Expanded widget cards. One file per widget so each can be edited on its own.

mod agents;
mod calendar;
mod messages;
mod notes;
mod notes_editor;
mod observe;
mod reminders;
mod speed;
mod timers;

pub(crate) use agents::{
    agents_card, compact_left as agents_compact_left, compact_right as agents_compact_right,
};
pub(crate) use calendar::calendar_card;
pub(crate) use messages::{
    compact_left as messages_compact_left, compact_right as messages_compact_right, messages_card,
    messages_peek,
};
pub(crate) use notes::notes_card;
pub(crate) use notes_editor::{NotesEditor, NotesEditorEvent};
pub(crate) use observe::{observe_card, ObserveHover};
pub(crate) use reminders::reminders_card;
pub(crate) use speed::speed_card;
pub(crate) use timers::{compact_left as timer_compact_left, timer_card};
