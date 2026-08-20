mod icons;
mod island;
mod platform;
mod theme;

use gpui::{actions, App, Application, KeyBinding};
use island::open_island;

actions!(nook, [Quit]);

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    Application::new()
        .with_assets(icons::Assets)
        .run(|cx: &mut App| {
        platform::install();
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        open_island(cx);
        cx.activate(false);
    });
}
