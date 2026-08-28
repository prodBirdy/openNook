mod dotmatrix;
mod icons;
mod island;
mod motion;
mod notify;
mod platform;
mod theme;
mod widgets;

use gpui::{actions, App, Application, KeyBinding};
use island::open_island;

actions!(nook, [Quit]);

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    Application::new()
        .with_assets(icons::Assets)
        .run(|cx: &mut App| {
            gpui_component::init(cx);
            nook_core::init();
            platform::install();
            platform::install_status_item();
            nook_core::install_window_management();
            cx.on_action(|_: &Quit, cx| cx.quit());
            cx.on_action(|_: &Quit, cx| {
                nook_core::high_alert::release_all();
                cx.quit();
            });
            cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
            open_island(cx);
            cx.activate(false);
        });
}
