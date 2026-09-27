mod dotmatrix;
mod icons;
mod island;
mod motion;
mod notify;
mod platform;
mod theme;
mod weather_shader;
mod widgets;

use gpui::{actions, App, Application, KeyBinding};
use island::open_island;
use nook_core::automation::{push_action, ExternalAction};

actions!(nook, [Quit, CloseWindow, OpenSettings]);

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let app = Application::new().with_assets(icons::Assets);
    // LaunchServices delivers opennook:// here. The callback is sync and may
    // fire before the island exists, so we only enqueue — never exec a shell.
    app.on_open_urls(|urls| {
        nook_core::automation::ingest_open_urls(&urls);
    });
    app.run(|cx: &mut App| {
        nook_core::init();
        platform::install();
        let task = cx.register_url_scheme("opennook");
        cx.foreground_executor()
            .spawn(async move {
                if let Err(err) = task.await {
                    log::debug!("register_url_scheme: {err}");
                }
            })
            .detach();
        cx.on_action(|_: &Quit, cx| {
            nook_core::high_alert::release_all();
            cx.quit();
        });
        cx.on_action(|_: &OpenSettings, _cx| {
            push_action(ExternalAction::OpenSettings);
        });
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-w", CloseWindow, None),
            KeyBinding::new("cmd-,", OpenSettings, None),
        ]);
        open_island(cx);
        #[cfg(target_os = "macos")]
        {
            // Reinstalls keep the onboarded flag, so the grant is requested at
            // launch instead of lazily.
            if std::env::current_exe()
                .ok()
                .and_then(|p| p.to_str().map(|s| s.contains(".app/Contents/MacOS/")))
                .unwrap_or(false)
                && !platform::ax_process_trusted(false)
            {
                let _ = platform::ax_process_trusted(true);
            }
        }
        cx.activate(false);
    });
}
