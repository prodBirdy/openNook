//! Live data mods read (media, agents, apps, island, theme, layout, shared): one JSON value
//! per topic, with a revision that moves when the value changes.

use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Default)]
pub struct Store {
    topics: Mutex<HashMap<String, (u64, Value)>>,
    /// Called after a topic changes (wakes the mods' thread).
    on_change: Mutex<Option<Box<dyn Fn() + Send>>>,
}

impl Store {
    pub fn get(&self, topic: &str) -> (u64, Value) {
        self.topics.lock().unwrap().get(topic).cloned().unwrap_or((0, Value::Null))
    }

    pub fn revision(&self, topic: &str) -> u64 {
        self.topics.lock().unwrap().get(topic).map_or(0, |t| t.0)
    }

    /// Replaces a topic's value; returns whether it changed.
    pub fn publish(&self, topic: &str, value: Value) -> bool {
        {
            let mut topics = self.topics.lock().unwrap();
            let entry = topics.entry(topic.to_owned()).or_insert((0, Value::Null));
            if entry.0 != 0 && entry.1 == value {
                return false;
            }
            entry.0 += 1;
            entry.1 = value;
        }
        if let Some(f) = &*self.on_change.lock().unwrap() {
            f();
        }
        true
    }

    pub fn on_change(&self, f: impl Fn() + Send + 'static) {
        *self.on_change.lock().unwrap() = Some(Box::new(f));
    }
}
