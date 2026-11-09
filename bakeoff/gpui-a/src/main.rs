//! gpui-a: the bake-off browser with GPUI drawing the window, CEF pages as
//! native child views, and the command bar as a second window glued over
//! the page (fix A). See docs/bakeoff.md.

mod browser;
mod command_bar;
mod demo;
mod overlay;

use browser::{BrowserHandle, BrowserWindow};
use gpui::{
    App, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, point, prelude::*, px,
    size,
};
use jaal_bakeoff_shared::bench::mark;
use jaal_bakeoff_shared::bench_modes as bench;
use jaal_gpui_cef::text_input;
use std::path::PathBuf;

/// `JAAL_PROFILE_DIR`, or `target/profiles/gpui-a` in the repo.
fn profile_dir() -> PathBuf {
    let dir = std::env::var_os("JAAL_PROFILE_DIR").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/profiles/gpui-a"),
        PathBuf::from,
    );
    std::fs::create_dir_all(&dir).expect("create profile dir");
    // CEF wants an absolute path.
    dir.canonicalize().expect("profile dir")
}

fn main() {
    mark("main");
    let bench = bench::BenchConfig::from_env();
    let urls = bench::startup_urls(bench.tabs, bench::url_argument(std::env::args()));

    // CEF first: it loads the framework, adds CefAppProtocol to GPUI's
    // NSApplication subclass and creates NSApp, all before GPUI's platform
    // exists. Keep the loader alive until exit.
    let _cef = jaal_gpui_cef::start(&profile_dir());

    Application::new().run(move |cx: &mut App| {
        text_input::bind_keys(cx);
        browser::bind_keys(cx);

        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("jaal (gpui-a)".into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(14.), px(14.))),
            }),
            window_min_size: Some(size(px(640.), px(420.))),
            ..Default::default()
        };
        let window: BrowserHandle = cx
            .open_window(options, |window, cx| {
                cx.new(|cx| BrowserWindow::new(urls, bench, window, cx))
            })
            .expect("open the main window");

        // The close button quits through the same path as Cmd+Q, which
        // closes the CEF browsers first.
        let _ = window.update(cx, |_, window, cx| {
            window.on_window_should_close(cx, |window, cx| {
                if let Some(Some(browser)) = window.root::<BrowserWindow>() {
                    browser.update(cx, |b, cx| b.quit(cx));
                }
                false
            });
        });
        cx.activate(true);
    });
}
