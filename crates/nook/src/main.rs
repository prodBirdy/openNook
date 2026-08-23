mod dotmatrix;
mod icons;
mod island;
mod motion;
mod platform;
mod theme;
mod widgets;

use gpui::{actions, App, Application, KeyBinding};
use island::open_island;

actions!(nook, [Quit, OpenSettings]);

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    Application::new()
        .with_assets(icons::Assets)
        .run(|cx: &mut App| {
            gpui_component::init(cx);
            nook_core::init();
            platform::install();
            platform::install_status_item();
            cx.on_action(|_: &Quit, cx| cx.quit());
            cx.on_action(|_: &OpenSettings, _cx| platform::request_open_settings());
            // cmd-* is the Mac extra; ctrl-* is what Linux/Windows keyboards send.
            cx.bind_keys([
                KeyBinding::new("cmd-q", Quit, None),
                KeyBinding::new("ctrl-q", Quit, None),
                KeyBinding::new("cmd-,", OpenSettings, None),
                KeyBinding::new("ctrl-,", OpenSettings, None),
            ]);
            open_island(cx);
            cx.activate(false);
        });
}
