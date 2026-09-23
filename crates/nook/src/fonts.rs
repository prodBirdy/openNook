//! Bundled island type: Inter + Roboto Mono (NOOK-MOCK-0923 export).

use gpui::App;
use std::borrow::Cow;

const INTER: &[u8] = include_bytes!("assets/fonts/InterVariable.ttf");
const ROBOTO_MONO: &[u8] = include_bytes!("assets/fonts/RobotoMonoVariable.ttf");

/// Register the export typefaces so Linux (no SF Pro) and Mac match the mock.
pub fn load(cx: &mut App) {
    let fonts = vec![Cow::Borrowed(INTER), Cow::Borrowed(ROBOTO_MONO)];
    if let Err(err) = cx.text_system().add_fonts(fonts) {
        log::warn!("nook fonts: {err}");
    }
}
