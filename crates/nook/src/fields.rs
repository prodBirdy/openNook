//! Mods' text fields (`InputState` and `Input` in nook/ui). The text lives here, in the island's
//! state, so it survives the mod's renders and every keystroke stays on the UI thread; what
//! happens to a field goes back to its mod as events.

use nook_mods::Mods;
use rune::TextEdit;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::BTreeMap;

pub struct Field {
    /// The mod that made it.
    pub owner: String,
    pub edit: TextEdit,
    /// What the mod was last told: the text, and whether the field had the keyboard. Rune edits
    /// `edit` through the field's accessor and says nothing else, so a render compares.
    told: RefCell<(String, bool)>,
}

#[derive(Default)]
pub struct Fields {
    map: BTreeMap<u64, Field>,
    /// What an accessor gets for a field that has gone (dropped while a key was on its way).
    scratch: TextEdit,
}

/// What the island does after a mod's request.
#[derive(Debug, PartialEq, Eq)]
pub enum Keys {
    None,
    Request,
    Release,
}

impl Fields {
    pub fn get(&self, id: u64) -> Option<&Field> {
        self.map.get(&id)
    }

    pub fn edit_mut(&mut self, id: u64) -> &mut TextEdit {
        match self.map.get_mut(&id) {
            Some(f) => &mut f.edit,
            None => &mut self.scratch,
        }
    }

    /// Whether any field has the keyboard.
    pub fn typing(&self) -> bool {
        self.map.values().any(|f| f.edit.focused)
    }

    /// A mod's request (`Host::input`).
    pub fn apply(&mut self, from: &str, id: u64, op: &Value) -> Keys {
        let value = op.get("value").and_then(Value::as_str).unwrap_or_default();
        match op.get("op").and_then(Value::as_str).unwrap_or_default() {
            "new" => {
                self.map.insert(id, Field { owner: from.to_owned(), edit: TextEdit::new(value), told: RefCell::new((value.to_owned(), false)) });
                Keys::None
            }
            "drop" => {
                let had = self.map.remove(&id).is_some_and(|f| f.edit.focused);
                if had && !self.typing() { Keys::Release } else { Keys::None }
            }
            op => {
                let Some(f) = self.map.get_mut(&id).filter(|f| f.owner == from) else { return Keys::None };
                match op {
                    "set" => {
                        f.edit.replace_text(value);
                        f.told.borrow_mut().0 = value.to_owned();
                        Keys::None
                    }
                    "mask" => {
                        f.edit.set_secure(!value.is_empty());
                        Keys::None
                    }
                    "focus" => {
                        f.edit.focused = true;
                        Keys::Request
                    }
                    "blur" if f.edit.focused => {
                        f.edit.focused = false;
                        if self.typing() { Keys::None } else { Keys::Release }
                    }
                    _ => Keys::None,
                }
            }
        }
    }

    /// Tells field `id`'s mod what changed since it was last told; true when the field has just
    /// taken the keyboard (the panel should ask for it).
    pub fn sync(&self, id: u64, mods: &Mods) -> bool {
        let Some(f) = self.map.get(&id) else { return false };
        let mut told = f.told.borrow_mut();
        if told.0 != f.edit.text {
            told.0 = f.edit.text.clone();
            mods.input_event(&f.owner, id, "change", json!({ "value": told.0 }));
        }
        if told.1 != f.edit.focused {
            told.1 = f.edit.focused;
            mods.input_event(&f.owner, id, if told.1 { "focus" } else { "blur" }, json!({}));
            return told.1;
        }
        false
    }

    /// Takes the keyboard off every field (the island closed, or the panel lost the keyboard);
    /// true if one had it.
    pub fn blur_all(&mut self, mods: Option<&Mods>) -> bool {
        let mut any = false;
        for (id, f) in &mut self.map {
            if f.edit.focused {
                f.edit.focused = false;
                any = true;
            }
            let mut told = f.told.borrow_mut();
            if told.1 {
                told.1 = false;
                if let Some(m) = mods {
                    m.input_event(&f.owner, *id, "blur", json!({}));
                }
            }
        }
        any
    }
}

/// A key as `on_key_down` handlers get it: `{ key, keystroke, key_char, modifiers }`, spelled
/// as herdr's (and GPUI's) are: `"escape"`, `"cmd-shift-s"`.
pub fn key_json(k: &rune::KeyEvent) -> Value {
    use rune::Key;
    let (key, typed) = match &k.key {
        Key::Text(t) => (t.to_lowercase(), Some(t.clone())),
        Key::Command(c) => (c.to_string(), None),
        Key::Enter => ("enter".into(), None),
        Key::Backspace => ("backspace".into(), None),
        Key::Delete => ("delete".into(), None),
        Key::Left => ("left".into(), None),
        Key::Right => ("right".into(), None),
        Key::Up => ("up".into(), None),
        Key::Down => ("down".into(), None),
        Key::Home => ("home".into(), None),
        Key::End => ("end".into(), None),
        Key::PageUp => ("pageup".into(), None),
        Key::PageDown => ("pagedown".into(), None),
        Key::Escape => ("escape".into(), None),
        Key::Tab => ("tab".into(), None),
        Key::Function(n) => (format!("f{n}"), None),
        other => (format!("{other:?}").to_lowercase(), None),
    };
    let m = k.mods;
    let mut chord: Vec<&str> = Vec::new();
    for (on, name) in [(m.ctrl, "ctrl"), (m.alt, "alt"), (m.shift, "shift"), (m.cmd, "cmd")] {
        if on {
            chord.push(name);
        }
    }
    let keystroke = if chord.is_empty() { key.clone() } else { format!("{}-{key}", chord.join("-")) };
    json!({
        "key": key,
        "keystroke": keystroke,
        "key_char": typed,
        "modifiers": { "cmd": m.cmd, "shift": m.shift, "alt": m.alt, "ctrl": m.ctrl },
    })
}
