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
    let app = Application::new().with_assets(icons::Assets);
    // LaunchServices delivers opennook:// here. The callback is sync and may
    // fire before the island exists, so we only enqueue — never exec a shell.
    app.on_open_urls(|urls| {
        nook_core::automation::ingest_open_urls(&urls);
    });
    app.run(|cx: &mut App| {
        gpui_component::init(cx);
        nook_core::init();
        platform::install();
        platform::install_status_item();
        let task = cx.register_url_scheme("opennook");
        cx.foreground_executor()
            .spawn(async move {
                if let Err(err) = task.await {
                    log::debug!("register_url_scheme: {err}");
                }
            })
            .detach();
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        open_island(cx);
        cx.activate(false);
    });
}
